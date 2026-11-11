mod completion;
mod database;
mod input;
mod navigation;
mod templates;

use crate::db::{DatabaseClient, QueryResult};
use crate::error::{Result, SqliError};
use crate::session::{ConnStatus, DbSession};
use crate::templates::{Template, TemplateStore};
use ratatui::layout::Rect;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tui_textarea::TextArea;

use std::process::Command;

use crate::cassandra::CassandraClient;
use crate::clickhouse::ClickHouseClient;
use crate::mysql::MySqlClient;
use crate::postgres::PostgresClient;

pub enum PendingOperation {
    ListDatabases {
        receiver: oneshot::Receiver<Result<Vec<String>>>,
        conn_name: String,
    },
    Connect {
        receiver: oneshot::Receiver<Result<(Arc<DbSession>, Vec<String>)>>,
        conn_name: String,
        db_name: String,
    },
    Query {
        receiver: oneshot::Receiver<Result<QueryResult>>,
        start: std::time::Instant,
        /// Handle to the spawned task so the query can be aborted on cancel.
        handle: tokio::task::JoinHandle<()>,
    },
    RefreshTables {
        receiver: oneshot::Receiver<Result<Vec<String>>>,
    },
}

/// Shown wherever a reconnect is reported: the server-side session is new.
const SESSION_RESET_NOTE: &str = "(session state was reset)";

fn run_password_command(cmd: &str) -> std::io::Result<String> {
    let output = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", cmd]).output()?
    } else {
        Command::new("sh").args(["-c", cmd]).output()?
    };

    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "Command failed with status: {}",
            output.status
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Debug, PartialEq)]
pub enum Mode {
    Normal,
    Command,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ViewState {
    ConnectionList,
    DatabaseList,
    DatabaseView,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Focus {
    Sidebar,
    Query,
    Output,
}

#[derive(Debug, Clone, Default)]
pub enum PopupState {
    #[default]
    None,
    TemplateList {
        selected: usize,
        filter: String,
        searching: bool,
    },
    SaveTemplate {
        name: String,
        connections: String, // comma-separated; empty = global
        editing_connections: bool,
    },
    ConfirmDelete {
        index: usize,
        name: String,
        filter: String, // Preserve filter to restore on cancel
    },
    RecordDetail {
        row_index: usize,
        selected_field: usize,
        scroll: usize,
    },
    Help {
        scroll: usize,
    },
    Completion {
        suggestions: Vec<crate::completion::Suggestion>,
        selected: usize,
        /// Position in query where completion starts (to replace text)
        word_start: usize,
    },
}

#[derive(Debug, Clone)]
pub enum DatabaseType {
    Postgres,
    MySql,
    Cassandra,
    ClickHouse,
    Sqlite,
}

impl DatabaseType {
    pub fn as_str(&self) -> &str {
        match self {
            DatabaseType::Postgres => "PostgreSQL",
            DatabaseType::MySql => "MySQL",
            DatabaseType::Cassandra => "Cassandra",
            DatabaseType::ClickHouse => "ClickHouse",
            DatabaseType::Sqlite => "SQLite",
        }
    }

    pub fn short_label(&self) -> &str {
        match self {
            DatabaseType::Postgres => "pg",
            DatabaseType::MySql => "my",
            DatabaseType::Cassandra => "cs",
            DatabaseType::ClickHouse => "ch",
            DatabaseType::Sqlite => "sq",
        }
    }

    pub fn default_database(&self) -> &'static str {
        match self {
            DatabaseType::Postgres => "postgres",
            DatabaseType::MySql => "",
            DatabaseType::Cassandra => "",
            DatabaseType::ClickHouse => "default",
            DatabaseType::Sqlite => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseConn {
    pub name: String,
    pub db_type: DatabaseType,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub password_cmd: Option<String>,
    pub database: Option<String>,
    pub path: Option<String>,
    pub tls: bool,
    pub readonly: bool,
    pub group: Option<String>,
    /// Protocol for ClickHouse: "native" (default) or "http"
    pub protocol: Option<String>,
}

impl DatabaseConn {
    /// Resolve the password: the output of `password_cmd` when configured,
    /// otherwise the `password` field. A failing command falls back to
    /// `password` if one is set, and is an error otherwise.
    fn resolve_password_blocking(&self) -> Result<String> {
        if let Some(ref cmd) = self.password_cmd {
            match run_password_command(cmd) {
                Ok(pwd) => return Ok(pwd),
                Err(e) => {
                    crate::debug_log!("Failed to run password_cmd '{}': {}", cmd, e);
                    if self.password.is_none() {
                        return Err(SqliError::Connection(format!("password_cmd failed: {}", e)));
                    }
                }
            }
        }
        Ok(self.password.clone().unwrap_or_default())
    }

    /// [`Self::resolve_password_blocking`] off the async worker threads, since
    /// `password_cmd` can take a while (or wait on a prompt).
    pub async fn resolve_password(&self) -> Result<String> {
        let conn = self.clone();
        tokio::task::spawn_blocking(move || conn.resolve_password_blocking())
            .await
            .map_err(|e| SqliError::Other(format!("password_cmd task failed: {}", e)))?
    }

    /// Resolve credentials and open a client for `database`.
    pub async fn create_client(&self, database: &str) -> Result<DatabaseClient> {
        let password = self.resolve_password().await?;
        self.connect(database, &password).await
    }

    /// Open a client for `database` with an already resolved password.
    pub async fn connect(&self, database: &str, password: &str) -> Result<DatabaseClient> {
        let client = match self.db_type {
            DatabaseType::Postgres => {
                let client = PostgresClient::connect(
                    &self.host,
                    self.port,
                    &self.user,
                    password,
                    database,
                    self.readonly,
                )
                .await?;
                DatabaseClient::Postgres(client)
            }
            DatabaseType::MySql => {
                let client = MySqlClient::connect(
                    &self.host,
                    self.port,
                    &self.user,
                    password,
                    database,
                    self.readonly,
                )
                .await?;
                DatabaseClient::MySql(client)
            }
            DatabaseType::Cassandra => {
                // Cassandra/CQL has no read-only session mode; read-only access
                // must be enforced via a read-only DB role/permissions.
                let client =
                    CassandraClient::connect(&self.host, self.port, &self.user, password, database)
                        .await?;
                DatabaseClient::Cassandra(client)
            }
            DatabaseType::ClickHouse => {
                let use_http = self
                    .protocol
                    .as_ref()
                    .is_some_and(|p| p.eq_ignore_ascii_case("http"));
                let client = ClickHouseClient::connect(
                    &self.host,
                    self.port,
                    &self.user,
                    password,
                    database,
                    self.tls,
                    use_http,
                    self.readonly,
                )
                .await?;
                DatabaseClient::ClickHouse(client)
            }
            DatabaseType::Sqlite => {
                let path = self.path.as_deref().unwrap_or_default();
                if self.host.is_empty() {
                    // Local SQLite
                    let client = crate::sqlite::SqliteClient::connect(path, self.readonly).await?;
                    DatabaseClient::Sqlite(client)
                } else {
                    // Remote SQLite via SSH
                    let client = crate::ssh_sqlite::SshSqliteClient::connect(
                        &self.host,
                        path,
                        self.readonly,
                    )
                    .await?;
                    DatabaseClient::SshSqlite(client)
                }
            }
        };
        Ok(client)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VisualSelect {
    /// Select individual cells in a single column
    Cell { anchor: usize },
    /// Select entire rows
    Line { anchor: usize },
}

/// On-screen placement of one visible result-table column, captured during render
/// so mouse hit-testing (column resize) matches exactly what was drawn.
#[derive(Debug, Clone, Copy)]
pub struct ColumnSegment {
    /// Index into the full result column list.
    pub index: usize,
    /// Left screen x of the column.
    pub x: u16,
    /// Rendered (possibly clipped) width on screen.
    pub width: u16,
    /// Logical full width of the column (the resize baseline).
    pub full_width: usize,
}

impl ColumnSegment {
    /// Screen x of the column's right edge (the draggable boundary).
    pub fn right_edge(&self) -> u16 {
        self.x.saturating_add(self.width)
    }
}

/// An in-progress column resize drag.
#[derive(Debug, Clone, Copy)]
pub struct ColumnDrag {
    pub col_index: usize,
    pub start_x: u16,
    pub start_width: usize,
}

/// Rectangles captured during the last render, so mouse hit-testing and
/// page-size math read the exact same geometry the renderer used instead of
/// re-deriving it from terminal size (which drifts and causes off-by-one bugs).
/// Reset every frame: a region that was not drawn cannot be hit.
#[derive(Debug, Default, Clone, Copy)]
pub struct ViewLayout {
    /// Item rows of the connection / database picker list.
    pub picker: Option<Rect>,
    /// Index of the first picker item on screen (the list's scroll offset).
    pub picker_offset: usize,
    /// Sidebar pane outer area (None when the sidebar is hidden).
    pub sidebar: Option<Rect>,
    /// Sidebar list content area (inner of the panel block) for row hit-testing.
    pub sidebar_list: Option<Rect>,
    /// Query input pane outer area.
    pub query: Option<Rect>,
    /// Output pane outer area.
    pub output: Option<Rect>,
    /// Output table content area: header occupies the top row, data rows follow.
    pub output_table: Option<Rect>,
}

#[derive(Debug, Default)]
pub struct SidebarState {
    pub tables: Vec<String>,
    pub selected: usize,
    /// Scroll offset (first visible list row, including the "Tables" header) from the
    /// last render. Persisted so click hit-testing matches what is on screen.
    pub offset: usize,
}

impl SidebarState {
    /// Total list rows, including the leading "Tables" header row.
    pub fn item_count(&self) -> usize {
        self.tables.len() + 1
    }

    /// List row index of the current selection (the "Tables" header occupies row 0).
    pub fn selected_row(&self) -> usize {
        self.selected + 1
    }

    /// Recompute the scroll offset so the selected row stays visible within
    /// `visible_height` list rows, scrolling as little as possible. The result is
    /// stored on `self.offset` and returned so the renderer and click hit-testing
    /// agree on what is on screen.
    pub fn scroll_to_selected(&mut self, visible_height: usize) -> usize {
        if visible_height == 0 {
            self.offset = 0;
            return 0;
        }

        let selected = self.selected_row();
        let mut offset = self.offset;
        if selected < offset {
            offset = selected;
        } else if selected >= offset + visible_height {
            offset = selected + 1 - visible_height;
        }

        let max_offset = self.item_count().saturating_sub(visible_height);
        self.offset = offset.min(max_offset);
        self.offset
    }
}

pub struct Tab {
    pub id: usize,
    pub name: String,
    pub connections: Vec<DatabaseConn>,
    pub selected_index: usize, // index in filtered connection list (for UI)
    pub connected_index: usize, // index in full connections list (after connection)
    pub view_state: ViewState,
    pub focus: Focus,
    /// Live connection to `current_database`; reconnects on its own.
    pub session: Option<Arc<DbSession>>,
    /// Whether `session` was up at the last poll, to announce changes once.
    pub conn_up: bool,
    /// `session.reconnects()` as of the last announcement. Catches a
    /// reconnect that finished between two polls.
    pub conn_reconnects: u64,
    pub current_database: Option<String>, // the selected database we're connected to
    pub databases: Vec<String>,           // list of databases for DatabaseList view
    pub database_selected: usize,         // selected index in database list
    pub sidebar: SidebarState,
    pub sidebar_hidden: bool, // whether the sidebar is hidden in DatabaseView
    pub column_cache: std::collections::HashMap<String, Vec<String>>, // table -> columns
    pub query_textarea: TextArea<'static>,
    pub query_result: Option<QueryResult>,
    /// Full text of the last query failure, shown in the results pane.
    pub query_error: Option<String>,
    pub result_scroll: usize,
    pub result_cursor: usize,
    pub result_h_scroll: usize,
    pub result_selected_col: usize,
    /// Manual per-column width overrides (column index -> width), set by mouse resize.
    pub result_col_overrides: std::collections::HashMap<usize, usize>,
    pub pending_g: bool,
    pub visual_select: Option<VisualSelect>,
    pub status_message: Option<String>,
    pub show_system_databases: bool,
    pub loading: bool,
    // Connection grouping
    pub connection_groups: Vec<String>, // list of group names (first is "All")
    pub selected_group: usize,          // index in connection_groups
    // Per-tab pending async operation
    pub pending_operation: Option<PendingOperation>,
}

impl Tab {
    pub fn new(id: usize, connections: Vec<DatabaseConn>) -> Self {
        // Build list of unique groups, preserving order of first appearance in config.
        // Connections are loaded via IndexMap which maintains TOML file order.
        // Connections without a group only appear under "All".
        let mut groups: Vec<String> = vec!["All".to_string()];
        for conn in &connections {
            if let Some(ref group) = conn.group
                && !groups.contains(group)
            {
                groups.push(group.clone());
            }
        }

        Self {
            id,
            name: "new".to_string(),
            connections,
            selected_index: 0,
            connected_index: 0,
            view_state: ViewState::ConnectionList,
            focus: Focus::Sidebar,
            session: None,
            conn_up: false,
            conn_reconnects: 0,
            current_database: None,
            databases: Vec::new(),
            database_selected: 0,
            sidebar: SidebarState::default(),
            sidebar_hidden: false,
            column_cache: std::collections::HashMap::new(),
            query_textarea: TextArea::default(),
            query_result: None,
            query_error: None,
            result_scroll: 0,
            result_cursor: 0,
            result_h_scroll: 0,
            result_selected_col: 0,
            result_col_overrides: std::collections::HashMap::new(),
            pending_g: false,
            visual_select: None,
            status_message: None,
            show_system_databases: false,
            loading: false,
            connection_groups: groups,
            selected_group: 0,
            pending_operation: None,
        }
    }

    /// Health of this tab's connection, if it has one.
    pub fn conn_status(&self) -> Option<ConnStatus> {
        self.session.as_ref().map(|session| session.status())
    }

    /// Announce a lost or restored connection in the status bar, once per
    /// change. Held back while an operation is running so its progress
    /// message stays visible; the indicator shows the live state meanwhile.
    fn announce_connection_changes(&mut self) {
        if self.loading {
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        let status = session.status();
        let up = status == ConnStatus::Connected;
        let reconnected = self.take_reconnected();
        if up == self.conn_up && !reconnected {
            return;
        }
        self.conn_up = up;
        self.status_message = Some(match status {
            ConnStatus::Connected => format!("Reconnected {}", SESSION_RESET_NOTE),
            ConnStatus::Reconnecting { error, .. } => format!("Connection lost: {}", error),
        });
    }

    /// Whether the connection was re-established since this was last asked.
    fn take_reconnected(&mut self) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        let reconnects = session.reconnects();
        let changed = reconnects != self.conn_reconnects;
        self.conn_reconnects = reconnects;
        changed
    }

    /// Get connections filtered by the current selected group
    pub fn filtered_connections(&self) -> Vec<&DatabaseConn> {
        if self.selected_group == 0 {
            // "All" group - return all connections
            self.connections.iter().collect()
        } else {
            let group_name = &self.connection_groups[self.selected_group];
            self.connections
                .iter()
                .filter(|c| c.group.as_ref() == Some(group_name))
                .collect()
        }
    }

    pub fn select_next(&mut self) {
        let count = self.filtered_connections().len();
        cycle_next(&mut self.selected_index, count);
    }

    pub fn select_previous(&mut self) {
        let count = self.filtered_connections().len();
        cycle_prev(&mut self.selected_index, count);
    }

    pub fn next_group(&mut self) {
        if self.connection_groups.len() > 1 {
            self.selected_group = (self.selected_group + 1) % self.connection_groups.len();
            self.selected_index = 0; // Reset selection when switching groups
        }
    }

    pub fn prev_group(&mut self) {
        if self.connection_groups.len() > 1 {
            self.selected_group = if self.selected_group == 0 {
                self.connection_groups.len() - 1
            } else {
                self.selected_group - 1
            };
            self.selected_index = 0; // Reset selection when switching groups
        }
    }

    pub fn sidebar_next(&mut self) {
        cycle_next(&mut self.sidebar.selected, self.sidebar.tables.len());
    }

    pub fn sidebar_prev(&mut self) {
        cycle_prev(&mut self.sidebar.selected, self.sidebar.tables.len());
    }

    pub fn database_next(&mut self) {
        cycle_next(&mut self.database_selected, self.databases.len());
    }

    pub fn database_prev(&mut self) {
        cycle_prev(&mut self.database_selected, self.databases.len());
    }

    /// Get the visual selection row range (start..=end) if visual mode is active
    pub fn visual_selection_range(&self) -> Option<(usize, usize)> {
        let anchor = match self.visual_select? {
            VisualSelect::Cell { anchor } => anchor,
            VisualSelect::Line { anchor } => anchor,
        };
        let start = anchor.min(self.result_cursor);
        let end = anchor.max(self.result_cursor);
        Some((start, end))
    }
}

fn cycle_next(index: &mut usize, len: usize) {
    if len > 0 {
        *index = (*index + 1) % len;
    }
}

fn cycle_prev(index: &mut usize, len: usize) {
    if len > 0 {
        *index = if *index == 0 { len - 1 } else { *index - 1 };
    }
}

pub struct Controller {
    pub mode: Mode,
    pub command_buffer: String,
    pub tabs: Vec<Tab>,
    pub current_tab: usize,
    pub quit: bool,
    pub runtime: Runtime,
    pub popup_state: PopupState,
    pub template_store: TemplateStore,
    pub template_list_cache: Vec<Template>,
    pub needs_redraw: bool,
    pub pending_escape: bool,
    pub pending_ctrl_w: bool,
    pub spinner_state: usize,
    pub clipboard: Option<arboard::Clipboard>,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    /// Geometry captured during the last render.
    pub view_layout: ViewLayout,
    /// Tab bar label areas from the last render, in tab order.
    pub tab_rects: Vec<Rect>,
    /// Visible result-table column placements from the last render (for resize hit-testing).
    pub result_columns: Vec<ColumnSegment>,
    /// Active column resize drag, if the user is currently dragging a header border.
    pub active_resize: Option<ColumnDrag>,
    /// Counter for generating unique tab IDs
    next_tab_id: usize,
}

impl Controller {
    pub fn with_connections(
        connections: Vec<DatabaseConn>,
        cli_connection: Option<DatabaseConn>,
    ) -> Self {
        let runtime = Runtime::new().expect("Failed to create tokio runtime");
        let template_store = TemplateStore::load();
        let clipboard = arboard::Clipboard::new()
            .map_err(|e| crate::debug_log!("Failed to init clipboard: {}", e))
            .ok();

        // If cli_connection is provided, use it as the only connection
        let (connections, auto_connect) = if let Some(conn) = cli_connection {
            (vec![conn], true)
        } else {
            (connections, false)
        };

        let mut controller = Self {
            mode: Mode::Normal,
            command_buffer: String::new(),
            tabs: vec![Tab::new(1, connections)],
            current_tab: 0,
            quit: false,
            runtime,
            popup_state: PopupState::default(),
            template_store,
            spinner_state: 0,
            template_list_cache: Vec::new(),
            needs_redraw: false,
            pending_escape: false,
            pending_ctrl_w: false,
            clipboard,
            last_click: None,
            view_layout: ViewLayout::default(),
            tab_rects: Vec::new(),
            result_columns: Vec::new(),
            active_resize: None,
            next_tab_id: 2,
        };

        // Auto-initiate connection if --connect was provided
        if auto_connect {
            controller.initiate_connection();
        }

        controller
    }

    pub fn current_tab(&self) -> &Tab {
        &self.tabs[self.current_tab]
    }

    pub fn current_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.current_tab]
    }

    pub fn tick_spinner(&mut self) {
        const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        self.spinner_state = (self.spinner_state + 1) % SPINNER_CHARS.len();
    }

    pub fn spinner_char(&self) -> char {
        const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        SPINNER_CHARS[self.spinner_state]
    }

    pub fn poll_pending(&mut self) {
        use chrono::Local;

        // Track if current tab connected (to reset textarea)
        let mut current_tab_connected = false;
        // Track if the current tab's result set was replaced
        let mut current_tab_result_changed = false;
        let current_tab_idx = self.current_tab;

        for (tab_idx, tab) in self.tabs.iter_mut().enumerate() {
            tab.announce_connection_changes();

            let op = match tab.pending_operation.take() {
                Some(op) => op,
                None => continue,
            };

            match op {
                PendingOperation::ListDatabases {
                    mut receiver,
                    conn_name,
                } => match receiver.try_recv() {
                    Ok(result) => {
                        tab.loading = false;
                        match result {
                            Ok(databases) => {
                                crate::debug_log!("Found {} database(s)", databases.len());
                                tab.name = conn_name;
                                // Pre-select current database if switching
                                let selected = tab
                                    .current_database
                                    .as_ref()
                                    .and_then(|cur| databases.iter().position(|d| d == cur))
                                    .unwrap_or(0);
                                tab.databases = databases;
                                tab.database_selected = selected;
                                tab.status_message = None;
                                tab.view_state = ViewState::DatabaseList;
                            }
                            Err(e) => {
                                crate::debug_log!("Failed to list databases: {}", e);
                                tab.status_message =
                                    Some(format!("Connection failed: {}", e.message()));
                            }
                        }
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {
                        tab.pending_operation = Some(PendingOperation::ListDatabases {
                            receiver,
                            conn_name,
                        });
                    }
                    Err(oneshot::error::TryRecvError::Closed) => {
                        tab.loading = false;
                        tab.status_message = Some("Connection task failed".to_string());
                    }
                },
                PendingOperation::Connect {
                    mut receiver,
                    conn_name,
                    db_name,
                } => match receiver.try_recv() {
                    Ok(result) => {
                        tab.loading = false;
                        match result {
                            Ok((session, tables)) => {
                                crate::debug_log!(
                                    "Connected to '{}', found {} table(s)",
                                    db_name,
                                    tables.len()
                                );
                                tab.name = conn_name;
                                tab.current_database = Some(db_name);
                                tab.conn_reconnects = session.reconnects();
                                tab.session = Some(session);
                                tab.conn_up = true;
                                tab.column_cache.clear();
                                tab.sidebar.tables = tables;
                                tab.sidebar.selected = 0;
                                tab.sidebar.offset = 0;
                                tab.status_message = None;
                                tab.view_state = ViewState::DatabaseView;
                                tab.focus = Focus::Query;
                                // Only reset textarea if THIS is the current tab
                                if tab_idx == current_tab_idx {
                                    current_tab_connected = true;
                                }
                            }
                            Err(e) => {
                                crate::debug_log!("Connection failed: {}", e);
                                tab.status_message =
                                    Some(format!("Connection failed: {}", e.message()));
                            }
                        }
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {
                        tab.pending_operation = Some(PendingOperation::Connect {
                            receiver,
                            conn_name,
                            db_name,
                        });
                    }
                    Err(oneshot::error::TryRecvError::Closed) => {
                        tab.loading = false;
                        tab.status_message = Some("Connection task failed".to_string());
                    }
                },
                PendingOperation::Query {
                    mut receiver,
                    start,
                    handle,
                } => match receiver.try_recv() {
                    Ok(result) => {
                        let elapsed = start.elapsed();
                        tab.loading = false;
                        tab.result_scroll = 0;
                        tab.result_cursor = 0;
                        tab.result_h_scroll = 0;
                        tab.result_selected_col = 0;
                        tab.result_col_overrides.clear();
                        tab.visual_select = None;
                        if tab_idx == current_tab_idx {
                            current_tab_result_changed = true;
                        }
                        let timestamp = Local::now().format("%H:%M:%S");
                        // The query may have had to reconnect before running.
                        let reconnected = if tab.take_reconnected() {
                            tab.conn_up = true;
                            format!(" - reconnected {}", SESSION_RESET_NOTE)
                        } else {
                            String::new()
                        };

                        match result {
                            Ok(query_result) => {
                                tab.query_error = None;
                                tab.query_result = Some(query_result.clone());
                                match &query_result {
                                    QueryResult::Select { rows, .. } => {
                                        crate::debug_log!(
                                            "Query returned {} row(s) in {:?}",
                                            rows.len(),
                                            elapsed
                                        );
                                        tab.status_message = Some(format!(
                                            "[{}] {} row(s) returned in {:.2?}{}",
                                            timestamp,
                                            rows.len(),
                                            elapsed,
                                            reconnected
                                        ));
                                    }
                                    QueryResult::Execute { rows_affected } => {
                                        crate::debug_log!(
                                            "Query affected {} row(s) in {:?}",
                                            rows_affected,
                                            elapsed
                                        );
                                        tab.status_message = Some(format!(
                                            "[{}] {} row(s) affected in {:.2?}{}",
                                            timestamp, rows_affected, elapsed, reconnected
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                let db_name = tab.current_database.as_deref().unwrap_or("unknown");
                                crate::debug_log!("Query error on database '{}': {}", db_name, e);
                                tab.query_result = None;
                                // The results pane shows the full text; the
                                // one-line status bar gets its first line.
                                let text = e.to_string();
                                let summary = text.lines().next().unwrap_or_default();
                                tab.status_message = Some(format!("[{}] {}", timestamp, summary));
                                tab.query_error = Some(e.message());
                            }
                        }
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {
                        tab.pending_operation = Some(PendingOperation::Query {
                            receiver,
                            start,
                            handle,
                        });
                    }
                    Err(oneshot::error::TryRecvError::Closed) => {
                        tab.loading = false;
                        tab.status_message = Some("Query task failed".to_string());
                    }
                },
                PendingOperation::RefreshTables { mut receiver } => match receiver.try_recv() {
                    Ok(result) => {
                        tab.loading = false;
                        match result {
                            Ok(tables) => {
                                crate::debug_log!("Refreshed tables: {} table(s)", tables.len());
                                tab.sidebar.tables = tables;
                                if tab.sidebar.selected >= tab.sidebar.tables.len() {
                                    tab.sidebar.selected =
                                        tab.sidebar.tables.len().saturating_sub(1);
                                }
                                tab.status_message = None;
                            }
                            Err(e) => {
                                crate::debug_log!("Failed to refresh tables: {}", e);
                                tab.status_message = Some(format!("Failed to refresh: {}", e));
                            }
                        }
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {
                        tab.pending_operation = Some(PendingOperation::RefreshTables { receiver });
                    }
                    Err(oneshot::error::TryRecvError::Closed) => {
                        tab.loading = false;
                        tab.status_message = Some("Refresh task failed".to_string());
                    }
                },
            }
        }

        // Reset textarea if current tab just connected
        if current_tab_connected {
            self.current_tab_mut().query_textarea = TextArea::default();
        }

        // The record popup points at a row of the result that was just replaced
        if current_tab_result_changed && matches!(self.popup_state, PopupState::RecordDetail { .. })
        {
            self.popup_state = PopupState::None;
        }
    }

    /// Make `index` the visible tab and drop UI state that belonged to the
    /// previous one (popups, half-typed key chords, an in-progress drag), so
    /// none of it leaks into a tab where it is not visible.
    pub(super) fn set_current_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.current_tab = index;
        self.popup_state = PopupState::None;
        self.pending_escape = false;
        self.pending_ctrl_w = false;
        self.active_resize = None;
    }
}

#[cfg(test)]
mod tests {
    use super::SidebarState;

    fn sidebar(table_count: usize, selected: usize, offset: usize) -> SidebarState {
        SidebarState {
            tables: (0..table_count).map(|i| format!("t{i}")).collect(),
            selected,
            offset,
        }
    }

    #[test]
    fn no_scroll_when_everything_fits() {
        let mut s = sidebar(5, 4, 0);
        // 5 tables + header = 6 rows, viewport of 10 rows shows them all.
        assert_eq!(s.scroll_to_selected(10), 0);
    }

    #[test]
    fn scrolls_to_keep_last_row_visible() {
        let mut s = sidebar(20, 19, 0);
        // 21 rows total, viewport of 10: last row (index 20) must be the bottom row.
        let offset = s.scroll_to_selected(10);
        assert_eq!(offset, 11);
        assert_eq!(offset + 10, s.item_count());
    }

    #[test]
    fn never_scrolls_past_the_end() {
        // A stale large offset is clamped so trailing rows aren't left blank.
        let mut s = sidebar(20, 19, 50);
        assert_eq!(s.scroll_to_selected(10), 11);
    }

    #[test]
    fn scrolls_up_minimally() {
        let mut s = sidebar(20, 10, 11);
        // selected row 11 is above the current top (11) edge boundary; pull it into view.
        assert_eq!(s.scroll_to_selected(10), 11);

        let mut s = sidebar(20, 5, 11);
        // selected row 6 is above the viewport; it becomes the new top row.
        assert_eq!(s.scroll_to_selected(10), 6);
    }

    #[test]
    fn zero_height_resets_offset() {
        let mut s = sidebar(20, 19, 7);
        assert_eq!(s.scroll_to_selected(0), 0);
    }
}
