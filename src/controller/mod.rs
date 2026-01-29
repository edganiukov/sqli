mod database;
mod input;
mod navigation;
mod templates;

use crate::db::{DatabaseClient, QueryResult};
use crate::error::Result;
use crate::templates::{Template, TemplateStore};
use std::collections::HashMap;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tui_textarea::TextArea;

use std::process::Command;

use crate::cassandra::CassandraClient;
use crate::clickhouse::ClickHouseClient;
use crate::mysql::MySqlClient;
use crate::postgres::PostgresClient;

pub enum PendingOperation {
    Connect {
        receiver: oneshot::Receiver<Result<(DatabaseClient, Vec<String>)>>,
        conn_name: String,
    },
    Query {
        receiver: oneshot::Receiver<Result<QueryResult>>,
        db_name: String,
        start: std::time::Instant,
    },
}

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
        scope: crate::templates::TemplateScope,
    },
    ConfirmDelete {
        index: usize,
        name: String,
    },
    RecordDetail {
        row_index: usize,
        scroll: usize,
    },
}

#[derive(Debug, Clone)]
pub enum DatabaseType {
    Postgres,
    MySql,
    Cassandra,
    ClickHouse,
}

impl DatabaseType {
    pub fn as_str(&self) -> &str {
        match self {
            DatabaseType::Postgres => "PostgreSQL",
            DatabaseType::MySql => "MySQL",
            DatabaseType::Cassandra => "Cassandra",
            DatabaseType::ClickHouse => "ClickHouse",
        }
    }

    pub fn default_database(&self) -> &'static str {
        match self {
            DatabaseType::Postgres => "postgres",
            DatabaseType::MySql => "",
            DatabaseType::Cassandra => "",
            DatabaseType::ClickHouse => "default",
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
    pub tls: bool,
    pub readonly: bool,
}

impl DatabaseConn {
    pub fn resolve_password(&self) -> String {
        if let Some(ref cmd) = self.password_cmd {
            match run_password_command(cmd) {
                Ok(pwd) => return pwd,
                Err(e) => {
                    crate::debug_log!("Failed to run password_cmd '{}': {}", cmd, e);
                }
            }
        }
        self.password.clone().unwrap_or_default()
    }

    pub async fn create_client(&self, database: &str) -> Result<DatabaseClient> {
        let password = self.resolve_password();
        match self.db_type {
            DatabaseType::Postgres => {
                let client = PostgresClient::connect(
                    &self.host,
                    self.port,
                    &self.user,
                    &password,
                    database,
                )
                .await?;
                Ok(DatabaseClient::Postgres(client))
            }
            DatabaseType::MySql => {
                let client = MySqlClient::connect(
                    &self.host,
                    self.port,
                    &self.user,
                    &password,
                    database,
                )
                .await?;
                Ok(DatabaseClient::MySql(client))
            }
            DatabaseType::Cassandra => {
                let client = CassandraClient::connect(
                    &self.host,
                    self.port,
                    &self.user,
                    &password,
                    database,
                )
                .await?;
                Ok(DatabaseClient::Cassandra(client))
            }
            DatabaseType::ClickHouse => {
                let client = ClickHouseClient::connect(
                    &self.host,
                    self.port,
                    &self.user,
                    &password,
                    database,
                    self.tls,
                )
                .await?;
                Ok(DatabaseClient::ClickHouse(client))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SidebarItem {
    Database(String),
    Table { database: String, table: String },
}

#[derive(Debug, Default)]
pub struct SidebarState {
    pub items: Vec<SidebarItem>,
    pub selected: usize,
    pub expanded: std::collections::HashSet<String>,
    pub tables: HashMap<String, Vec<String>>,
}

pub struct Tab {
    pub name: String,
    pub connections: Vec<DatabaseConn>,
    pub selected_index: usize,
    pub connected_db: Option<String>,
    pub view_state: ViewState,
    pub focus: Focus,
    pub db_client: Option<DatabaseClient>,
    pub sidebar: SidebarState,
    pub databases: Vec<String>,
    pub current_database: Option<String>,
    pub query_result: Option<QueryResult>,
    pub result_scroll: usize,
    pub result_cursor: usize,
    pub pending_g: bool,
    pub status_message: Option<String>,
    pub show_system_databases: bool,
    pub loading: bool,
}

impl Tab {
    pub fn new(connections: Vec<DatabaseConn>) -> Self {
        Self {
            name: "New".to_string(),
            connections,
            selected_index: 0,
            connected_db: None,
            view_state: ViewState::ConnectionList,
            focus: Focus::Sidebar,
            db_client: None,
            sidebar: SidebarState::default(),
            databases: Vec::new(),
            current_database: None,
            query_result: None,
            result_scroll: 0,
            result_cursor: 0,
            pending_g: false,
            status_message: None,
            show_system_databases: false,
            loading: false,
        }
    }

    pub fn select_next(&mut self) {
        if !self.connections.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.connections.len();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.connections.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.connections.len() - 1;
            } else {
                self.selected_index -= 1;
            }
        }
    }

    pub fn rebuild_sidebar_items(&mut self) {
        self.sidebar.items.clear();
        for db in &self.databases {
            self.sidebar.items.push(SidebarItem::Database(db.clone()));
            if self.sidebar.expanded.contains(db)
                && let Some(tables) = self.sidebar.tables.get(db)
            {
                for table in tables {
                    self.sidebar.items.push(SidebarItem::Table {
                        database: db.clone(),
                        table: table.clone(),
                    });
                }
            }
        }
    }

    pub fn sidebar_next(&mut self) {
        if !self.sidebar.items.is_empty() {
            self.sidebar.selected = (self.sidebar.selected + 1) % self.sidebar.items.len();
        }
    }

    pub fn sidebar_prev(&mut self) {
        if !self.sidebar.items.is_empty() {
            if self.sidebar.selected == 0 {
                self.sidebar.selected = self.sidebar.items.len() - 1;
            } else {
                self.sidebar.selected -= 1;
            }
        }
    }
}

pub struct Controller {
    pub mode: Mode,
    pub command_buffer: String,
    pub query_textarea: TextArea<'static>,
    pub tabs: Vec<Tab>,
    pub current_tab: usize,
    pub quit: bool,
    pub runtime: Runtime,
    pub popup_state: PopupState,
    pub template_store: TemplateStore,
    pub template_list_cache: Vec<Template>,
    pub needs_redraw: bool,
    pub pending_escape: bool,
    pub spinner_state: usize,
    pub pending_operation: Option<PendingOperation>,
}

impl Controller {
    pub fn with_connections(connections: Vec<DatabaseConn>) -> Self {
        let runtime = Runtime::new().expect("Failed to create tokio runtime");
        let template_store = TemplateStore::load();

        Self {
            mode: Mode::Normal,
            command_buffer: String::new(),
            query_textarea: TextArea::default(),
            tabs: vec![Tab::new(connections)],
            current_tab: 0,
            quit: false,
            runtime,
            popup_state: PopupState::default(),
            template_store,
            spinner_state: 0,
            pending_operation: None,
            template_list_cache: Vec::new(),
            needs_redraw: false,
            pending_escape: false,
        }
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

        let op = match self.pending_operation.take() {
            Some(op) => op,
            None => return,
        };

        match op {
            PendingOperation::Connect { mut receiver, conn_name } => {
                match receiver.try_recv() {
                    Ok(result) => {
                        let tab = self.current_tab_mut();
                        tab.loading = false;
                        match result {
                            Ok((client, dbs)) => {
                                crate::debug_log!("Found {} database(s)", dbs.len());
                                tab.current_database = dbs.first().cloned();
                                tab.databases = dbs;
                                tab.db_client = Some(client);
                                tab.rebuild_sidebar_items();
                                tab.status_message = None;
                                tab.view_state = ViewState::DatabaseView;
                                tab.focus = Focus::Sidebar;
                            }
                            Err(e) => {
                                crate::debug_log!("Connection failed: {}", e);
                                tab.status_message = Some(format!("Connection failed: {}", e));
                            }
                        }
                        self.query_textarea = TextArea::default();
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {
                        // Still pending
                        self.pending_operation = Some(PendingOperation::Connect {
                            receiver,
                            conn_name,
                        });
                    }
                    Err(oneshot::error::TryRecvError::Closed) => {
                        let tab = self.current_tab_mut();
                        tab.loading = false;
                        tab.status_message = Some("Connection task failed".to_string());
                    }
                }
            }
            PendingOperation::Query { mut receiver, db_name, start } => {
                match receiver.try_recv() {
                    Ok(result) => {
                        let elapsed = start.elapsed();
                        let tab = self.current_tab_mut();
                        tab.loading = false;
                        tab.result_scroll = 0;
                        tab.result_cursor = 0;
                        let timestamp = Local::now().format("%H:%M:%S");

                        match result {
                            Ok(query_result) => {
                                tab.query_result = Some(query_result.clone());
                                match &query_result {
                                    QueryResult::Select { rows, .. } => {
                                        crate::debug_log!(
                                            "Query returned {} row(s) in {:?}",
                                            rows.len(),
                                            elapsed
                                        );
                                        tab.status_message = Some(format!(
                                            "[{}] {} row(s) returned in {:.2?}",
                                            timestamp,
                                            rows.len(),
                                            elapsed
                                        ));
                                    }
                                    QueryResult::Execute { rows_affected } => {
                                        crate::debug_log!(
                                            "Query affected {} row(s) in {:?}",
                                            rows_affected,
                                            elapsed
                                        );
                                        tab.status_message = Some(format!(
                                            "[{}] {} row(s) affected in {:.2?}",
                                            timestamp, rows_affected, elapsed
                                        ));
                                        // TODO: refresh databases
                                    }
                                }
                            }
                            Err(e) => {
                                crate::debug_log!("Query error on database '{}': {}", db_name, e);
                                tab.query_result = None;
                                tab.status_message =
                                    Some(format!("Error [{}]: {}", db_name, e));
                            }
                        }
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {
                        // Still pending
                        self.pending_operation = Some(PendingOperation::Query {
                            receiver,
                            db_name,
                            start,
                        });
                    }
                    Err(oneshot::error::TryRecvError::Closed) => {
                        let tab = self.current_tab_mut();
                        tab.loading = false;
                        tab.status_message = Some("Query task failed".to_string());
                    }
                }
            }
        }
    }
}
