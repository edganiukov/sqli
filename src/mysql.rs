use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};

use mysql_async::prelude::*;
use mysql_async::{Conn, Opts, OptsBuilder, SslOpts, Value};
use tokio::sync::{Mutex, MutexGuard};

use crate::db::{QueryResult, Tls};
use crate::error::Result;
use crate::format as fmt;
use crate::sql;

pub struct MySqlClient {
    /// One dedicated connection, not a pool. A pooled connection is reset each
    /// time it is handed out, which silently ends an open transaction and
    /// forgets `USE`, session variables and temporary tables between
    /// statements: `BEGIN; UPDATE ...; ROLLBACK` used to leave the update
    /// committed.
    conn: Mutex<Conn>,
    /// Set while a statement is in flight. Still set once the connection is
    /// free again means the statement never ran to completion (its task was
    /// cancelled), so the connection may be mid-protocol and must not be
    /// reused.
    in_flight: AtomicBool,
    /// Kept to open the short-lived side connection that cancels a statement.
    opts: Opts,
    connection_id: u32,
}

/// Exclusive use of the connection for one operation. Call [`InUse::finished`]
/// when the operation has run to completion, successfully or not.
struct InUse<'a> {
    conn: MutexGuard<'a, Conn>,
    in_flight: &'a AtomicBool,
}

impl InUse<'_> {
    fn finished(self) {
        self.in_flight.store(false, Ordering::Release);
    }
}

impl Deref for InUse<'_> {
    type Target = Conn;

    fn deref(&self) -> &Conn {
        &self.conn
    }
}

impl DerefMut for InUse<'_> {
    fn deref_mut(&mut self) -> &mut Conn {
        &mut self.conn
    }
}

impl MySqlClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
        database: &str,
        readonly: bool,
        tls: Tls,
    ) -> Result<Self> {
        let mut opts = OptsBuilder::default()
            .ip_or_hostname(host)
            .tcp_port(port)
            .user(Some(user))
            .pass(Some(password))
            .db_name(if database.is_empty() {
                None
            } else {
                Some(database)
            });

        opts = match tls {
            Tls::Off => opts,
            Tls::Verified => opts.ssl_opts(SslOpts::default()),
            Tls::Unverified => opts.ssl_opts(
                SslOpts::default()
                    .with_danger_accept_invalid_certs(true)
                    .with_danger_skip_domain_validation(true),
            ),
        };

        // Runs on every new connection, so read-only mode is enforced
        // server-side again after a reconnect.
        if readonly {
            opts = opts.init(vec!["SET SESSION TRANSACTION READ ONLY"]);
        }

        let opts = Opts::from(opts);
        let conn = Conn::new(opts.clone()).await?;
        let connection_id = conn.id();

        Ok(Self {
            conn: Mutex::new(conn),
            in_flight: AtomicBool::new(false),
            opts,
            connection_id,
        })
    }

    async fn conn(&self) -> InUse<'_> {
        let conn = self.conn.lock().await;
        self.in_flight.store(true, Ordering::Release);
        InUse {
            conn,
            in_flight: &self.in_flight,
        }
    }

    /// True when a statement was abandoned midway, leaving the connection in
    /// an unknown protocol state. (While a statement is merely running, the
    /// connection is locked and this is false.)
    pub fn is_closed(&self) -> bool {
        self.in_flight.load(Ordering::Acquire) && self.conn.try_lock().is_ok()
    }

    pub async fn ping(&self) -> Result<()> {
        let mut conn = self.conn().await;
        let result = conn.ping().await;
        conn.finished();
        Ok(result?)
    }

    /// Stop the statement running on this connection. MySQL has no in-band
    /// cancel, so this asks from a second connection.
    pub async fn cancel_running(&self) -> Result<()> {
        let mut side = Conn::new(self.opts.clone()).await?;
        side.query_drop(format!("KILL QUERY {}", self.connection_id))
            .await?;
        side.disconnect().await?;
        Ok(())
    }

    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>> {
        const SYSTEM_DATABASES: &[&str] =
            &["information_schema", "mysql", "performance_schema", "sys"];

        let mut conn = self.conn().await;
        let databases = conn.query::<String, _>("SHOW DATABASES").await;
        conn.finished();

        Ok(databases?
            .into_iter()
            .filter(|db| include_system || !SYSTEM_DATABASES.contains(&db.as_str()))
            .collect())
    }

    pub async fn list_tables(&self, database: &str) -> Result<Vec<String>> {
        let query = format!(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = {} ORDER BY table_name",
            sql::literal_backslash(database)
        );
        let mut conn = self.conn().await;
        let tables = conn.query::<String, _>(query).await;
        conn.finished();
        Ok(tables?)
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        let mut conn = self.conn().await;
        let result = Self::run_statement(&mut conn, query).await;
        conn.finished();
        result
    }

    async fn run_statement(conn: &mut Conn, query: &str) -> Result<QueryResult> {
        // Text protocol, which accepts every statement (prepared statements
        // reject USE, LOCK TABLES and others). Whether rows come back is read
        // from the response instead of guessed from the first keyword.
        let mut result = conn.query_iter(query).await?;

        let columns: Vec<String> = result
            .columns_ref()
            .iter()
            .map(|c| c.name_str().to_string())
            .collect();

        if columns.is_empty() {
            let rows_affected = result.affected_rows();
            result.drop_result().await?;
            return Ok(QueryResult::Execute { rows_affected });
        }

        let rows: Vec<mysql_async::Row> = result.collect().await?;
        // A statement can produce more result sets (CALL); only the first is shown.
        result.drop_result().await?;

        let data_rows: Vec<Vec<String>> = rows
            .iter()
            .map(|row| {
                (0..columns.len())
                    .map(|i| Self::format_value(row.get::<Value, _>(i)))
                    .collect()
            })
            .collect();

        Ok(QueryResult::Select {
            columns,
            rows: data_rows,
        })
    }

    /// `` `database`.`table` ``, or just the table when no database is known.
    fn qualified(table: &str, database: Option<&str>) -> String {
        match database {
            Some(db) if !db.is_empty() => {
                format!("{}.{}", sql::ident(db, '`'), sql::ident(table, '`'))
            }
            _ => sql::ident(table, '`'),
        }
    }

    pub fn select_table_query(&self, table: &str, limit: usize, database: Option<&str>) -> String {
        format!(
            "SELECT * FROM {} LIMIT {}",
            Self::qualified(table, database),
            limit
        )
    }

    pub fn describe_table_query(&self, table: &str, database: Option<&str>) -> String {
        format!("DESCRIBE {}", Self::qualified(table, database))
    }

    pub async fn list_columns(&self, table: &str, database: Option<&str>) -> Result<Vec<String>> {
        let query = format!("SHOW COLUMNS FROM {}", Self::qualified(table, database));
        let mut conn = self.conn().await;
        let rows = conn.query::<mysql_async::Row, _>(query).await;
        conn.finished();
        Ok(rows?.iter().filter_map(|r| r.get::<String, _>(0)).collect())
    }

    fn format_value(value: Option<Value>) -> String {
        match value {
            None | Some(Value::NULL) => "NULL".to_string(),
            Some(Value::Bytes(b)) => String::from_utf8_lossy(&b).to_string(),
            Some(Value::Int(i)) => i.to_string(),
            Some(Value::UInt(u)) => u.to_string(),
            Some(Value::Float(f)) => f.to_string(),
            Some(Value::Double(d)) => d.to_string(),
            Some(Value::Date(y, m, d, h, min, s, _us)) => {
                if h == 0 && min == 0 && s == 0 {
                    fmt::date(y as i32, m as u32, d as u32)
                } else {
                    fmt::datetime(y as i32, m as u32, d as u32, h as u32, min as u32, s as u32)
                }
            }
            Some(Value::Time(neg, d, h, m, s, us)) => {
                let sign = if neg { "-" } else { "" };
                let total_hours = d * 24 + h as u32;
                if us == 0 {
                    format!("{}{}:{:02}:{:02}", sign, total_hours, m, s)
                } else {
                    format!("{}{}:{:02}:{:02}.{:06}", sign, total_hours, m, s, us)
                }
            }
        }
    }
}
