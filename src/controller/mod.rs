mod database;
mod input;
mod navigation;
mod templates;

use crate::db::{DatabaseClient, QueryResult};
use crate::error::Result;
use crate::templates::{Template, TemplateStore};
use std::collections::HashMap;
use tokio::runtime::Runtime;
use tui_textarea::TextArea;

use crate::cassandra::CassandraClient;
use crate::clickhouse::ClickHouseClient;
use crate::mysql::MySqlClient;
use crate::postgres::PostgresClient;

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
    pub password: String,
    pub tls: bool,
    pub readonly: bool,
}

impl DatabaseConn {
    pub async fn create_client(&self, database: &str) -> Result<DatabaseClient> {
        match self.db_type {
            DatabaseType::Postgres => {
                let client = PostgresClient::connect(
                    &self.host,
                    self.port,
                    &self.user,
                    &self.password,
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
                    &self.password,
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
                    &self.password,
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
                    &self.password,
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
}

impl Controller {
    pub fn new(config_path: Option<std::path::PathBuf>) -> Self {
        let connections = crate::config::load_config(config_path);
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
}
