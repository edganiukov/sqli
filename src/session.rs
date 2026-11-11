//! A database session that keeps itself connected.
//!
//! A [`Session`] owns the client behind one tab. A background supervisor pings
//! it while idle and re-establishes it (with backoff) when the link drops, and
//! every operation goes through [`Session::run`], which checks the connection
//! before using it. The UI reads [`Session::status`] to show connection health.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::time::Instant;

use crate::controller::DatabaseConn;
use crate::db::DatabaseClient;
use crate::error::{Result, SqliError};

/// Idle time between keepalive pings.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
/// How often the supervisor wakes to notice a connection the server closed.
const SUPERVISOR_TICK: Duration = Duration::from_secs(1);
/// A connection unused for this long is pinged before a query is sent on it.
const STALE_AFTER: Duration = Duration::from_secs(10);
const PING_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A query that finds a reconnect attempt this recent reuses its failure
/// instead of making the caller wait through another one.
const RECENT_FAILURE: Duration = Duration::from_millis(500);

/// Connection health as shown in the status bar.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnStatus {
    Connected,
    /// The link is down and is being re-established.
    Reconnecting {
        /// Attempt in progress or last failed; 0 until the first one starts.
        attempt: u32,
        /// Why the connection was lost, or why the last attempt failed.
        error: String,
        /// Time until the next automatic attempt, when one is scheduled.
        retry_in: Option<Duration>,
    },
}

/// A live connection the session can probe.
pub trait Link: Send + Sync + 'static {
    /// Whether the connection is known to be dead without doing any I/O.
    fn is_closed(&self) -> bool;
    fn ping(&self) -> impl Future<Output = Result<()>> + Send;
}

/// Opens connections for a session.
pub trait Connector: Send + Sync + 'static {
    type Link: Link;

    /// Open a new connection. `fresh_credentials` asks for credentials to be
    /// re-resolved rather than reused from an earlier attempt.
    fn connect(&self, fresh_credentials: bool) -> impl Future<Output = Result<Self::Link>> + Send;
}

impl Link for DatabaseClient {
    fn is_closed(&self) -> bool {
        DatabaseClient::is_closed(self)
    }

    async fn ping(&self) -> Result<()> {
        DatabaseClient::ping(self).await
    }
}

/// Connects to one database of a configured connection.
pub struct DbConnector {
    conn: DatabaseConn,
    database: String,
    /// Resolved once and reused, so reconnecting does not re-run `password_cmd`
    /// (which may prompt) on every attempt.
    password: Mutex<Option<String>>,
}

impl Connector for DbConnector {
    type Link = DatabaseClient;

    async fn connect(&self, fresh_credentials: bool) -> Result<DatabaseClient> {
        let cached = if fresh_credentials {
            None
        } else {
            lock(&self.password).clone()
        };
        let password = match cached {
            Some(password) => password,
            None => {
                let password = self.conn.resolve_password().await?;
                *lock(&self.password) = Some(password.clone());
                password
            }
        };
        self.conn.connect(&self.database, &password).await
    }
}

/// The session type used by tabs.
pub type DbSession = Session<DbConnector>;

impl Session<DbConnector> {
    /// Connect to `database` and start keeping the connection alive.
    pub async fn open(conn: DatabaseConn, database: String) -> Result<Arc<Self>> {
        Self::start(DbConnector {
            conn,
            database,
            password: Mutex::new(None),
        })
        .await
    }
}

struct State<L> {
    link: Arc<L>,
    /// Bumped on every reconnect, so a failure reported against a client that
    /// has since been replaced is ignored.
    generation: u64,
    health: Health,
    last_ok: Instant,
    /// Statement-level guess at whether a transaction is open on `link`.
    in_transaction: bool,
    /// A reconnect discarded an open transaction and nobody was told yet.
    transaction_lost: bool,
}

enum Health {
    Connected,
    Down {
        attempt: u32,
        error: String,
        last_failure: Option<Instant>,
        retry_at: Option<Instant>,
    },
}

pub struct Session<C: Connector = DbConnector> {
    connector: C,
    state: Mutex<State<C::Link>>,
    /// Serializes reconnect attempts between the supervisor and queries.
    reconnecting: tokio::sync::Mutex<()>,
    /// Operations in flight. Keepalive pings are skipped while busy: on a
    /// single-connection backend they would queue behind the running query.
    busy: AtomicUsize,
    /// Asks the supervisor to ping now instead of at the next interval.
    verify: AtomicBool,
    wake: Arc<Notify>,
}

impl<C: Connector> Session<C> {
    /// Open the first connection and spawn the supervisor that keeps it alive.
    /// The supervisor stops once the last `Arc<Session>` is dropped.
    pub async fn start(connector: C) -> Result<Arc<Self>> {
        let link = connector.connect(false).await?;
        let session = Arc::new(Self {
            connector,
            state: Mutex::new(State {
                link: Arc::new(link),
                generation: 0,
                health: Health::Connected,
                last_ok: Instant::now(),
                in_transaction: false,
                transaction_lost: false,
            }),
            reconnecting: tokio::sync::Mutex::new(()),
            busy: AtomicUsize::new(0),
            verify: AtomicBool::new(false),
            wake: Arc::new(Notify::new()),
        });
        tokio::spawn(supervise(
            Arc::downgrade(&session),
            Arc::clone(&session.wake),
        ));
        Ok(session)
    }

    pub fn status(&self) -> ConnStatus {
        match &self.state().health {
            Health::Connected => ConnStatus::Connected,
            Health::Down {
                attempt,
                error,
                retry_at,
                ..
            } => ConnStatus::Reconnecting {
                attempt: *attempt,
                error: error.clone(),
                retry_in: retry_at.map(|at| at.saturating_duration_since(Instant::now())),
            },
        }
    }

    /// How many times the connection has been re-established. A change means
    /// server-side session state (settings, temp tables) started over.
    pub fn reconnects(&self) -> u64 {
        self.state().generation
    }

    /// The current client, for work that needs no round trip (building SQL).
    /// Use [`Session::run`] for anything that talks to the server.
    pub fn client(&self) -> Arc<C::Link> {
        Arc::clone(&self.state().link)
    }

    /// Run `op` on a live connection, reconnecting first if the current one is
    /// down, and feed the outcome back into health tracking.
    ///
    /// A connection that dies while `op` is running fails `op`; it is not
    /// retried, because the statement may already have taken effect.
    pub async fn run<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: FnOnce(Arc<C::Link>) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let (link, generation) = self.acquire().await?;
        let _busy = Busy::enter(&self.busy);
        let result = op(link).await;
        match &result {
            Ok(_) => self.mark_ok(generation),
            Err(e) if e.is_connection() => {
                if self.mark_lost(generation, e.message()) {
                    self.wake.notify_one();
                }
            }
            Err(_) => {
                // Not obviously a connection problem, but backends classify
                // errors unevenly: have the supervisor check right away.
                self.verify.store(true, Ordering::Relaxed);
                self.wake.notify_one();
            }
        }
        result
    }

    /// Record a statement that just succeeded, to track open transactions.
    pub fn track_statement(&self, sql: &str) {
        if let Some(open) = transaction_effect(sql) {
            self.state().in_transaction = open;
        }
    }

    /// True (once) if a reconnect threw away a transaction that was open. The
    /// caller should refuse to run more statements as if it were still open.
    pub fn take_transaction_lost(&self) -> bool {
        std::mem::take(&mut self.state().transaction_lost)
    }

    fn state(&self) -> MutexGuard<'_, State<C::Link>> {
        lock(&self.state)
    }

    fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed) > 0
    }

    /// A client believed healthy, plus its generation.
    async fn acquire(&self) -> Result<(Arc<C::Link>, u64)> {
        let (link, generation, connected, idle) = {
            let state = self.state();
            (
                Arc::clone(&state.link),
                state.generation,
                matches!(state.health, Health::Connected),
                state.last_ok.elapsed(),
            )
        };

        if connected {
            if link.is_closed() {
                self.mark_lost(generation, "connection closed by the server".to_string());
            } else if idle < STALE_AFTER || self.is_busy() {
                return Ok((link, generation));
            } else {
                // Idle for a while: probe before sending, so a statement is
                // never written to a connection that silently died.
                match ping(&*link).await {
                    Ok(()) => {
                        self.mark_ok(generation);
                        return Ok((link, generation));
                    }
                    Err(e) => {
                        self.mark_lost(generation, e.message());
                    }
                }
            }
        }

        self.reconnect(generation, Attempt::ForQuery).await
    }

    /// Replace the client that was current at `generation`. If another task
    /// already replaced it, that newer client is returned instead.
    async fn reconnect(&self, generation: u64, kind: Attempt) -> Result<(Arc<C::Link>, u64)> {
        let for_query = kind == Attempt::ForQuery;
        let _attempting = self.reconnecting.lock().await;

        let scheduled_retry;
        let attempt = {
            let mut state = self.state();
            if state.generation != generation {
                return Ok((Arc::clone(&state.link), state.generation));
            }
            let (previous, error, last_failure, retry_at) = match &state.health {
                Health::Down {
                    attempt,
                    error,
                    last_failure,
                    retry_at,
                } => (*attempt, error.clone(), *last_failure, *retry_at),
                Health::Connected => (0, String::new(), None, None),
            };
            // An attempt failed moments ago: report that instead of making
            // the query wait through another one.
            if for_query && last_failure.is_some_and(|t| t.elapsed() < RECENT_FAILURE) {
                return Err(SqliError::Connection(error));
            }
            // An extra attempt on behalf of a query leaves the supervisor's
            // next scheduled retry where it was.
            scheduled_retry = if for_query { retry_at } else { None };
            let attempt = previous + 1;
            state.health = Health::Down {
                attempt,
                error,
                last_failure,
                retry_at: scheduled_retry,
            };
            attempt
        };

        // The first attempt reuses cached credentials; the second refreshes
        // them once in case they rotated (short-lived tokens).
        let outcome =
            match tokio::time::timeout(RECONNECT_TIMEOUT, self.connector.connect(attempt == 2))
                .await
            {
                Ok(outcome) => outcome,
                Err(_) => Err(SqliError::Connection(
                    "connection attempt timed out".to_string(),
                )),
            };

        let mut state = self.state();
        match outcome {
            Ok(link) => {
                state.link = Arc::new(link);
                state.generation += 1;
                state.health = Health::Connected;
                state.last_ok = Instant::now();
                if std::mem::take(&mut state.in_transaction) {
                    state.transaction_lost = true;
                }
                Ok((Arc::clone(&state.link), state.generation))
            }
            Err(e) => {
                state.health = Health::Down {
                    attempt,
                    error: e.message(),
                    last_failure: Some(Instant::now()),
                    retry_at: scheduled_retry,
                };
                Err(e)
            }
        }
    }

    fn mark_ok(&self, generation: u64) {
        let mut state = self.state();
        if state.generation == generation {
            state.last_ok = Instant::now();
        }
    }

    /// Flag the client of `generation` as dead. Returns false if it was
    /// already flagged or has been replaced since.
    fn mark_lost(&self, generation: u64, error: String) -> bool {
        let mut state = self.state();
        if state.generation != generation || !matches!(state.health, Health::Connected) {
            return false;
        }
        state.health = Health::Down {
            attempt: 0,
            error,
            last_failure: None,
            retry_at: None,
        };
        true
    }

    /// One supervisor pass. Returns how long to sleep before the next one.
    async fn supervise_once(&self, next_ping: &mut Instant) -> Duration {
        let (link, generation, connected) = {
            let state = self.state();
            (
                Arc::clone(&state.link),
                state.generation,
                matches!(state.health, Health::Connected),
            )
        };

        if connected {
            if link.is_closed() {
                self.mark_lost(generation, "connection closed by the server".to_string());
            } else {
                let due =
                    self.verify.swap(false, Ordering::Relaxed) || Instant::now() >= *next_ping;
                if !due || self.is_busy() {
                    return SUPERVISOR_TICK;
                }
                *next_ping = Instant::now() + KEEPALIVE_INTERVAL;
                match ping(&*link).await {
                    Ok(()) => {
                        self.mark_ok(generation);
                        return SUPERVISOR_TICK;
                    }
                    Err(e) => {
                        self.mark_lost(generation, e.message());
                    }
                }
            }
        }

        match self.reconnect(generation, Attempt::Scheduled).await {
            Ok(_) => {
                *next_ping = Instant::now() + KEEPALIVE_INTERVAL;
                SUPERVISOR_TICK
            }
            Err(_) => {
                let mut state = self.state();
                let Health::Down {
                    attempt, retry_at, ..
                } = &mut state.health
                else {
                    // A query reconnected in the meantime.
                    return SUPERVISOR_TICK;
                };
                let delay = backoff(*attempt);
                *retry_at = Some(Instant::now() + delay);
                delay
            }
        }
    }
}

/// Who is asking for a reconnect attempt.
#[derive(Clone, Copy, PartialEq)]
enum Attempt {
    /// The supervisor, on its backoff schedule.
    Scheduled,
    /// A query that found the connection down and cannot wait for the schedule.
    ForQuery,
}

/// Keepalive loop. Holds only a weak reference between passes so the session
/// (and its connection) is released as soon as its tab lets go of it.
async fn supervise<C: Connector>(session: Weak<Session<C>>, wake: Arc<Notify>) {
    let mut next_ping = Instant::now() + KEEPALIVE_INTERVAL;
    let mut delay = SUPERVISOR_TICK;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = wake.notified() => {}
        }
        let Some(session) = session.upgrade() else {
            return;
        };
        delay = session.supervise_once(&mut next_ping).await;
    }
}

async fn ping<L: Link>(link: &L) -> Result<()> {
    match tokio::time::timeout(PING_TIMEOUT, link.ping()).await {
        Ok(result) => result,
        Err(_) => Err(SqliError::Connection(
            "no response to keepalive ping".to_string(),
        )),
    }
}

/// Delay before the next attempt after `failed_attempts` consecutive failures:
/// 1s, 2s, 4s, ... capped at [`MAX_BACKOFF`].
fn backoff(failed_attempts: u32) -> Duration {
    let exponent = failed_attempts.saturating_sub(1).min(5);
    Duration::from_secs(1 << exponent).min(MAX_BACKOFF)
}

/// Whether a statement opens (`Some(true)`) or ends (`Some(false)`) a
/// transaction, judged by its leading keyword.
fn transaction_effect(sql: &str) -> Option<bool> {
    let mut words = sql.split_whitespace();
    let first = words.next()?.trim_end_matches(';').to_ascii_uppercase();
    match first.as_str() {
        "BEGIN" | "START" => Some(true),
        "COMMIT" | "END" | "ABORT" => Some(false),
        // `ROLLBACK TO SAVEPOINT` stays inside the transaction.
        "ROLLBACK" => match words.next() {
            Some(next) if next.eq_ignore_ascii_case("TO") => None,
            _ => Some(false),
        },
        _ => None,
    }
}

/// Lock a std mutex, ignoring poisoning: the guarded state stays consistent
/// because no critical section here can panic midway through an update.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Counts an in-flight operation for as long as it is alive, including when
/// the operation's task is aborted.
struct Busy<'a>(&'a AtomicUsize);

impl<'a> Busy<'a> {
    fn enter(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared switches a test flips to simulate the server.
    #[derive(Default)]
    struct Server {
        /// Connections opened so far.
        connects: AtomicUsize,
        /// While set, new connections are refused.
        down: AtomicBool,
        /// Values of `fresh_credentials` seen, in order.
        fresh: Mutex<Vec<bool>>,
    }

    struct FakeLink {
        id: usize,
        closed: AtomicBool,
        /// While set, pings fail (a half-open connection).
        dead: AtomicBool,
        pings: AtomicUsize,
    }

    impl Link for FakeLink {
        fn is_closed(&self) -> bool {
            self.closed.load(Ordering::Relaxed)
        }

        async fn ping(&self) -> Result<()> {
            self.pings.fetch_add(1, Ordering::Relaxed);
            if self.dead.load(Ordering::Relaxed) {
                Err(SqliError::Connection("broken pipe".to_string()))
            } else {
                Ok(())
            }
        }
    }

    struct FakeConnector(Arc<Server>);

    impl Connector for FakeConnector {
        type Link = FakeLink;

        async fn connect(&self, fresh_credentials: bool) -> Result<FakeLink> {
            lock(&self.0.fresh).push(fresh_credentials);
            if self.0.down.load(Ordering::Relaxed) {
                return Err(SqliError::Connection("connection refused".to_string()));
            }
            let id = self.0.connects.fetch_add(1, Ordering::Relaxed);
            Ok(FakeLink {
                id,
                closed: AtomicBool::new(false),
                dead: AtomicBool::new(false),
                pings: AtomicUsize::new(0),
            })
        }
    }

    async fn session() -> (Arc<Session<FakeConnector>>, Arc<Server>) {
        let server = Arc::new(Server::default());
        let session = Session::start(FakeConnector(Arc::clone(&server)))
            .await
            .unwrap();
        (session, server)
    }

    /// Let spawned tasks (the supervisor) run up to their next await point.
    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    async fn link_id(session: &Session<FakeConnector>) -> Result<usize> {
        session.run(|link| async move { Ok(link.id) }).await
    }

    #[tokio::test(start_paused = true)]
    async fn supervisor_replaces_a_connection_the_server_closed() {
        let (session, server) = session().await;
        session.client().closed.store(true, Ordering::Relaxed);

        tokio::time::sleep(SUPERVISOR_TICK * 2).await;
        settle().await;

        assert_eq!(session.status(), ConnStatus::Connected);
        assert_eq!(session.client().id, 1);
        assert_eq!(server.connects.load(Ordering::Relaxed), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn idle_keepalive_detects_a_half_open_connection() {
        let (session, _server) = session().await;
        session.client().dead.store(true, Ordering::Relaxed);

        // Nothing notices before the keepalive interval elapses...
        tokio::time::sleep(KEEPALIVE_INTERVAL / 2).await;
        settle().await;
        assert_eq!(session.client().id, 0);

        // ...then the ping fails and the connection is replaced.
        tokio::time::sleep(KEEPALIVE_INTERVAL).await;
        settle().await;
        assert_eq!(session.status(), ConnStatus::Connected);
        assert_eq!(session.client().id, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn query_on_a_closed_connection_reconnects_first() {
        let (session, _server) = session().await;
        session.client().closed.store(true, Ordering::Relaxed);

        assert_eq!(link_id(&session).await.unwrap(), 1);
        assert_eq!(session.status(), ConnStatus::Connected);
    }

    #[tokio::test(start_paused = true)]
    async fn stale_connection_is_pinged_before_use_but_a_fresh_one_is_not() {
        let (session, _server) = session().await;

        link_id(&session).await.unwrap();
        assert_eq!(session.client().pings.load(Ordering::Relaxed), 0);

        tokio::time::sleep(STALE_AFTER + Duration::from_secs(1)).await;
        session.client().dead.store(true, Ordering::Relaxed);

        // The probe fails, so the query runs on a new connection instead.
        assert_eq!(link_id(&session).await.unwrap(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn retries_with_backoff_until_the_server_returns() {
        let (session, server) = session().await;
        server.down.store(true, Ordering::Relaxed);
        session.client().closed.store(true, Ordering::Relaxed);

        // Attempts land at ~1s, 2s, 4s and 8s; the fourth is the last before 10s.
        tokio::time::sleep(Duration::from_secs(10)).await;
        settle().await;
        match session.status() {
            ConnStatus::Reconnecting {
                attempt,
                error,
                retry_in,
            } => {
                assert_eq!(attempt, 4);
                assert_eq!(error, "connection refused");
                assert!(retry_in.is_some());
            }
            other => panic!("expected reconnecting, got {other:?}"),
        }
        // Only the second attempt asked for fresh credentials.
        assert_eq!(*lock(&server.fresh), vec![false, false, true, false, false]);

        server.down.store(false, Ordering::Relaxed);
        tokio::time::sleep(MAX_BACKOFF).await;
        settle().await;
        assert_eq!(session.status(), ConnStatus::Connected);
        assert_eq!(session.client().id, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn query_fails_fast_while_the_server_is_down() {
        let (session, server) = session().await;
        server.down.store(true, Ordering::Relaxed);
        session.client().closed.store(true, Ordering::Relaxed);

        let err = link_id(&session).await.unwrap_err();
        assert!(err.is_connection());
        assert!(matches!(
            session.status(),
            ConnStatus::Reconnecting { attempt: 1, .. }
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn query_attempt_keeps_the_scheduled_retry() {
        let (session, server) = session().await;
        server.down.store(true, Ordering::Relaxed);
        session.client().closed.store(true, Ordering::Relaxed);

        // Supervisor attempts at 1s and 2s; the next is scheduled for 4s.
        tokio::time::sleep(Duration::from_millis(2500)).await;
        settle().await;
        assert!(link_id(&session).await.is_err());

        match session.status() {
            ConnStatus::Reconnecting {
                attempt, retry_in, ..
            } => {
                assert_eq!(attempt, 3);
                assert_eq!(retry_in, Some(Duration::from_millis(1500)));
            }
            other => panic!("expected reconnecting, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn reconnects_are_counted() {
        let (session, _server) = session().await;
        assert_eq!(session.reconnects(), 0);
        session.client().closed.store(true, Ordering::Relaxed);
        link_id(&session).await.unwrap();
        assert_eq!(session.reconnects(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn connection_error_in_a_query_triggers_a_reconnect() {
        let (session, _server) = session().await;

        let result: Result<()> = session
            .run(|_| async { Err(SqliError::Connection("connection reset".to_string())) })
            .await;
        assert!(result.is_err());
        assert!(matches!(
            session.status(),
            ConnStatus::Reconnecting { attempt: 0, .. }
        ));

        settle().await;
        assert_eq!(session.status(), ConnStatus::Connected);
        assert_eq!(session.client().id, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn statement_error_keeps_the_connection() {
        let (session, server) = session().await;

        let result: Result<()> = session
            .run(|_| async { Err(SqliError::Query("syntax error".to_string())) })
            .await;
        assert!(result.is_err());

        // The supervisor verifies with a ping right away and finds it healthy.
        settle().await;
        assert_eq!(session.status(), ConnStatus::Connected);
        assert_eq!(session.client().pings.load(Ordering::Relaxed), 1);
        assert_eq!(server.connects.load(Ordering::Relaxed), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_is_skipped_while_a_query_is_running() {
        let (session, _server) = session().await;

        let running = Arc::clone(&session);
        let query = tokio::spawn(async move {
            running
                .run(|link| async move {
                    tokio::time::sleep(KEEPALIVE_INTERVAL * 3).await;
                    Ok(link.pings.load(Ordering::Relaxed))
                })
                .await
        });

        assert_eq!(query.await.unwrap().unwrap(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_reports_a_lost_transaction_once() {
        let (session, _server) = session().await;
        session.track_statement("BEGIN");
        session.client().closed.store(true, Ordering::Relaxed);

        link_id(&session).await.unwrap();
        assert!(session.take_transaction_lost());
        assert!(!session.take_transaction_lost());
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_without_open_transaction_is_silent() {
        let (session, _server) = session().await;
        session.track_statement("begin;");
        session.track_statement("COMMIT");
        session.client().closed.store(true, Ordering::Relaxed);

        link_id(&session).await.unwrap();
        assert!(!session.take_transaction_lost());
    }

    #[tokio::test(start_paused = true)]
    async fn supervisor_stops_when_the_session_is_dropped() {
        let (session, server) = session().await;
        let weak = Arc::downgrade(&session);
        drop(session);

        tokio::time::sleep(SUPERVISOR_TICK * 2).await;
        settle().await;
        assert!(weak.upgrade().is_none());
        assert_eq!(server.connects.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let secs: Vec<u64> = (1..=8).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[test]
    fn transaction_keywords() {
        assert_eq!(transaction_effect("BEGIN"), Some(true));
        assert_eq!(transaction_effect("start transaction"), Some(true));
        assert_eq!(transaction_effect("commit;"), Some(false));
        assert_eq!(transaction_effect("ROLLBACK"), Some(false));
        assert_eq!(transaction_effect("rollback to savepoint a"), None);
        assert_eq!(transaction_effect("SELECT 1"), None);
        assert_eq!(transaction_effect(""), None);
    }
}
