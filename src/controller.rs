use crate::cassandra::CassandraClient;
use crate::clickhouse::ClickHouseClient;
use crate::db::{DatabaseClient, QueryResult};
use crate::postgres::PostgresClient;
use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tokio::runtime::Runtime;
use tui_textarea::TextArea;

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

#[derive(Debug, Clone)]
pub enum DatabaseType {
    Postgres,
    Cassandra,
    ClickHouse,
}

impl DatabaseType {
    pub fn as_str(&self) -> &str {
        match self {
            DatabaseType::Postgres => "PostgreSQL",
            DatabaseType::Cassandra => "Cassandra",
            DatabaseType::ClickHouse => "ClickHouse",
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
}

impl DatabaseConn {
    pub async fn create_client(
        &self,
        database: &str,
    ) -> Result<DatabaseClient, Box<dyn std::error::Error + Send + Sync>> {
        match self.db_type {
            DatabaseType::Postgres => {
                let client =
                    PostgresClient::connect(&self.host, self.port, &self.user, &self.password, database)
                        .await?;
                Ok(DatabaseClient::Postgres(client))
            }
            DatabaseType::Cassandra => {
                let client =
                    CassandraClient::connect(&self.host, self.port, &self.user, &self.password, database)
                        .await?;
                Ok(DatabaseClient::Cassandra(client))
            }
            DatabaseType::ClickHouse => {
                let client =
                    ClickHouseClient::connect(&self.host, self.port, &self.user, &self.password, database)
                        .await?;
                Ok(DatabaseClient::ClickHouse(client))
            }
        }
    }

    pub fn default_database(&self) -> String {
        match self.db_type {
            DatabaseType::Postgres => PostgresClient::default_database().to_string(),
            DatabaseType::Cassandra => CassandraClient::default_database().to_string(),
            DatabaseType::ClickHouse => ClickHouseClient::default_database().to_string(),
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
            if self.sidebar.expanded.contains(db) {
                if let Some(tables) = self.sidebar.tables.get(db) {
                    for table in tables {
                        self.sidebar.items.push(SidebarItem::Table {
                            database: db.clone(),
                            table: table.clone(),
                        });
                    }
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
}

impl Controller {
    pub fn new() -> Self {
        let connections = crate::config::load_config();
        let runtime = Runtime::new().expect("Failed to create tokio runtime");

        Self {
            mode: Mode::Normal,
            command_buffer: String::new(),
            query_textarea: TextArea::default(),
            tabs: vec![Tab::new(connections)],
            current_tab: 0,
            quit: false,
            runtime,
        }
    }

    pub fn current_tab(&self) -> &Tab {
        &self.tabs[self.current_tab]
    }

    pub fn current_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.current_tab]
    }

    pub fn new_tab(&mut self) {
        // Clone connections from first tab for now
        let connections = self.tabs[0].connections.clone();
        self.tabs.push(Tab::new(connections));
        self.current_tab = self.tabs.len() - 1;
    }

    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.current_tab = (self.current_tab + 1) % self.tabs.len();
        }
    }

    pub fn previous_tab(&mut self) {
        if !self.tabs.is_empty() {
            if self.current_tab == 0 {
                self.current_tab = self.tabs.len() - 1;
            } else {
                self.current_tab -= 1;
            }
        }
    }

    pub fn close_current_tab(&mut self) {
        if self.tabs.len() == 1 {
            // Last tab, quit the app.
            self.quit = true;
        } else {
            self.tabs.remove(self.current_tab);
            if self.current_tab >= self.tabs.len() {
                self.current_tab = self.tabs.len() - 1;
            }
        }
    }

    pub fn handle_normal_mode(&mut self, key_event: KeyEvent) {
        let view_state = self.current_tab().view_state;
        match view_state {
            ViewState::ConnectionList => self.handle_connection_list_keys(key_event.code),
            ViewState::DatabaseView => self.handle_database_view_keys(key_event),
        }
    }

    fn handle_connection_list_keys(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.current_tab_mut().select_next();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.current_tab_mut().select_previous();
            }
            KeyCode::Char('t') => {
                self.new_tab();
            }
            KeyCode::Enter => {
                self.connect_to_database();
            }
            _ => {}
        }
    }

    fn connect_to_database(&mut self) {
        let tab = self.current_tab_mut();
        let conn = match tab.connections.get(tab.selected_index) {
            Some(c) => c.clone(),
            None => return,
        };

        tab.status_message = Some("Connecting...".to_string());
        tab.name = conn.name.clone();
        tab.connected_db = Some(conn.name.clone());

        let db_name = conn.default_database();
        let result = self.runtime.block_on(conn.create_client(&db_name));

        let tab = self.current_tab_mut();
        let include_system = tab.show_system_databases;
        match result {
            Ok(client) => {
                let db_client: DatabaseClient = client;
                let databases =
                    self.runtime.block_on(async { db_client.list_databases(include_system).await });

                let tab = self.current_tab_mut();
                match databases {
                    Ok(dbs) => {
                        tab.current_database = dbs.first().cloned();
                        tab.databases = dbs;
                        tab.db_client = Some(db_client);
                        tab.rebuild_sidebar_items();
                        tab.status_message = None;
                        tab.view_state = ViewState::DatabaseView;
                        tab.focus = Focus::Sidebar;
                    }
                    Err(e) => {
                        tab.status_message = Some(format!("Failed to list databases: {}", e));
                    }
                }
            }
            Err(e) => {
                tab.status_message = Some(format!("Connection failed: {}", e));
            }
        }
        self.query_textarea = TextArea::default();
    }

    fn load_tables_for_database(&mut self, db_name: &str) {
        let db_name = db_name.to_string();

        {
            let tab = self.current_tab();
            if tab.sidebar.tables.contains_key(&db_name) {
                return;
            }
        }

        let conn = {
            let tab = self.current_tab();
            match tab.connections.get(tab.selected_index) {
                Some(c) => c.clone(),
                None => return,
            }
        };

        // For Cassandra, connect without keyspace; for others, connect to the database
        let connect_db = match conn.db_type {
            DatabaseType::Cassandra => String::new(),
            _ => db_name.clone(),
        };
        // For PostgreSQL, list "public" schema; for others, use database name
        let schema = match conn.db_type {
            DatabaseType::Postgres => "public".to_string(),
            _ => db_name.clone(),
        };

        let result: Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> =
            self.runtime.block_on(async {
                let client = conn.create_client(&connect_db).await?;
                client.list_tables(&schema).await
            });

        let tab = self.current_tab_mut();
        match result {
            Ok(tables) => {
                tab.sidebar.tables.insert(db_name, tables);
            }
            Err(e) => {
                tab.status_message = Some(format!("Failed to load tables: {}", e));
            }
        }
    }

    pub fn handle_database_view_keys(&mut self, key_event: KeyEvent) {
        let focus = self.current_tab().focus;
        match focus {
            Focus::Sidebar => self.handle_sidebar_keys(key_event.code),
            Focus::Query => self.handle_query_keys(key_event),
            Focus::Output => self.handle_output_keys(key_event.code),
        }
    }

    fn handle_sidebar_keys(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.current_tab_mut().sidebar_next();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.current_tab_mut().sidebar_prev();
            }
            KeyCode::Tab => {
                self.current_tab_mut().focus = Focus::Query;
            }
            KeyCode::BackTab => {
                self.current_tab_mut().focus = Focus::Output;
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.current_tab_mut().focus = Focus::Query;
            }
            KeyCode::Enter => {
                self.toggle_sidebar_item();
            }
            KeyCode::Char('d') => {
                self.describe_table();
            }
            KeyCode::Char('r') => {
                self.refresh_databases();
                let timestamp = Local::now().format("%H:%M:%S");
                self.current_tab_mut().status_message =
                    Some(format!("[{}] Refreshed", timestamp));
            }
            KeyCode::F(5) => {
                self.execute_query();
            }
            _ => {}
        }
    }

    fn toggle_sidebar_item(&mut self) {
        let tab = self.current_tab();
        let item = match tab.sidebar.items.get(tab.sidebar.selected) {
            Some(i) => i.clone(),
            None => return,
        };

        match item {
            SidebarItem::Database(db_name) => {
                // Set as current database for queries
                self.current_tab_mut().current_database = Some(db_name.clone());

                let is_expanded = self.current_tab().sidebar.expanded.contains(&db_name);
                if is_expanded {
                    self.current_tab_mut().sidebar.expanded.remove(&db_name);
                } else {
                    self.current_tab_mut().sidebar.expanded.insert(db_name.clone());
                    self.load_tables_for_database(&db_name);
                }
                self.current_tab_mut().rebuild_sidebar_items();
            }
            SidebarItem::Table { database, table } => {
                // Set parent database as current
                self.current_tab_mut().current_database = Some(database);
                // Populate query and execute
                let query = self
                    .current_tab()
                    .db_client
                    .as_ref()
                    .map(|c| c.select_table_query(&table, 50))
                    .unwrap_or_default();
                self.query_textarea.select_all();
                self.query_textarea.cut();
                self.query_textarea.insert_str(&query);
                self.execute_query();
                self.current_tab_mut().focus = Focus::Output;
            }
        }
    }

    fn describe_table(&mut self) {
        let tab = self.current_tab();
        let item = match tab.sidebar.items.get(tab.sidebar.selected) {
            Some(i) => i.clone(),
            None => return,
        };

        if let SidebarItem::Table { database, table } = item {
            self.current_tab_mut().current_database = Some(database.clone());
            let query = self
                .current_tab()
                .db_client
                .as_ref()
                .map(|c| c.describe_table_query(&table, Some(&database)))
                .unwrap_or_default();
            self.query_textarea.select_all();
            self.query_textarea.cut();
            self.query_textarea.insert_str(&query);
            self.execute_query();
            self.current_tab_mut().focus = Focus::Output;
        }
    }

    fn handle_query_keys(&mut self, key_event: KeyEvent) {
        // Check for execute shortcut first (F5 or Ctrl+J)
        if key_event.code == KeyCode::F(5) {
            self.execute_query();
            return;
        }
        // Ctrl+J to execute (Ctrl+Enter doesn't work reliably across terminals)
        if key_event.modifiers.contains(KeyModifiers::CONTROL)
            && key_event.code == KeyCode::Char('j')
        {
            self.execute_query();
            return;
        }

        match key_event.code {
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            KeyCode::Tab => {
                self.current_tab_mut().focus = Focus::Output;
            }
            KeyCode::BackTab => {
                self.focus_sidebar();
            }
            _ => {
                self.query_textarea.input(key_event);
            }
        }
    }

    fn focus_sidebar(&mut self) {
        self.current_tab_mut().focus = Focus::Sidebar;
        self.refresh_databases();
    }

    fn handle_output_keys(&mut self, key_code: KeyCode) {
        // Use a reasonable default visible height (will be adjusted by scroll logic)
        const VISIBLE_HEIGHT: usize = 20;

        let tab = self.current_tab_mut();

        // Handle gg (scroll to top)
        if tab.pending_g {
            tab.pending_g = false;
            if key_code == KeyCode::Char('g') {
                self.scroll_to_start();
                return;
            }
        }

        match key_code {
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            KeyCode::Tab => {
                self.focus_sidebar();
            }
            KeyCode::BackTab => {
                self.current_tab_mut().focus = Focus::Query;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_cursor(1, VISIBLE_HEIGHT);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_cursor(-1, VISIBLE_HEIGHT);
            }
            KeyCode::PageDown => {
                self.move_cursor(10, VISIBLE_HEIGHT);
            }
            KeyCode::PageUp => {
                self.move_cursor(-10, VISIBLE_HEIGHT);
            }
            KeyCode::Char('g') => {
                self.current_tab_mut().pending_g = true;
            }
            KeyCode::Char('G') => {
                self.scroll_to_end();
            }
            KeyCode::F(5) => {
                self.execute_query();
            }
            _ => {}
        }
    }

    fn scroll_to_end(&mut self) {
        let tab = self.current_tab_mut();
        if let Some(QueryResult::Select { rows, .. }) = &tab.query_result {
            let max_cursor = rows.len().saturating_sub(1);
            tab.result_cursor = max_cursor;
            // Scroll so cursor is visible at bottom
            tab.result_scroll = max_cursor;
        }
    }

    fn scroll_to_start(&mut self) {
        let tab = self.current_tab_mut();
        tab.result_cursor = 0;
        tab.result_scroll = 0;
    }

    fn move_cursor(&mut self, delta: i32, visible_height: usize) {
        let tab = self.current_tab_mut();
        if let Some(QueryResult::Select { rows, .. }) = &tab.query_result {
            let max_cursor = rows.len().saturating_sub(1);

            // Move cursor
            if delta > 0 {
                tab.result_cursor = (tab.result_cursor + delta as usize).min(max_cursor);
            } else {
                tab.result_cursor = tab.result_cursor.saturating_sub((-delta) as usize);
            }

            // Adjust scroll to keep cursor visible
            if tab.result_cursor < tab.result_scroll {
                tab.result_scroll = tab.result_cursor;
            } else if tab.result_cursor >= tab.result_scroll + visible_height {
                tab.result_scroll = tab.result_cursor - visible_height + 1;
            }
        }
    }

    fn execute_query(&mut self) {
        let query: String = self.query_textarea.lines().join("\n");
        if query.trim().is_empty() {
            return;
        }

        let conn = {
            let tab = self.current_tab();
            match tab.connections.get(tab.selected_index) {
                Some(c) => c.clone(),
                None => return,
            }
        };

        // Use selected database or fall back to default
        let (db_name, using_default) = match self.current_tab().current_database.clone() {
            Some(db) => (db, false),
            None => (conn.default_database(), true),
        };

        if using_default {
            let msg = if db_name.is_empty() {
                "No keyspace selected, using none...".to_string()
            } else {
                format!("No database selected, using '{}'...", db_name)
            };
            self.current_tab_mut().status_message = Some(msg);
        } else {
            self.current_tab_mut().status_message = Some("Executing...".to_string());
        }

        let result: Result<QueryResult, Box<dyn std::error::Error + Send + Sync>> =
            self.runtime.block_on(async {
                let client = conn.create_client(&db_name).await?;
                client.execute_query(&query).await
            });

        let tab = self.current_tab_mut();
        tab.result_scroll = 0;
        tab.result_cursor = 0;
        let timestamp = Local::now().format("%H:%M:%S");
        let mut should_refresh = false;
        match result {
            Ok(query_result) => {
                tab.query_result = Some(query_result.clone());
                match &query_result {
                    QueryResult::Select { rows, .. } => {
                        tab.status_message =
                            Some(format!("[{}] {} row(s) returned", timestamp, rows.len()));
                    }
                    QueryResult::Execute { rows_affected } => {
                        tab.status_message =
                            Some(format!("[{}] {} row(s) affected", timestamp, rows_affected));
                        should_refresh = true;
                    }
                }
            }
            Err(e) => {
                tab.query_result = None;
                tab.status_message = Some(format!("Error: [{}] {}", timestamp, e));
            }
        }

        if should_refresh {
            self.refresh_databases();
        }
    }

    pub fn handle_command_mode(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.command_buffer.clear();
            }
            KeyCode::Enter => {
                self.exec_command();
                self.mode = Mode::Normal;
                self.command_buffer.clear();
            }
            KeyCode::Backspace => {
                self.command_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.command_buffer.push(c);
            }
            _ => {}
        }
    }

    fn exec_command(&mut self) {
        match self.command_buffer.as_str() {
            "q" | "quit" => self.close_current_tab(),
            "qa" | "q!" | "quitall" => self.quit = true,
            "next" => self.next_tab(),
            "prev" => self.previous_tab(),
            "new" => self.new_tab(),
            "sysdb" => self.toggle_system_databases(),
            "help" | "h" => self.show_help(),
            cmd => {
                self.current_tab_mut().status_message = Some(format!("Unknown command: {}", cmd));
            }
        }
    }

    fn show_help(&mut self) {
        let help = ":q quit | :new tab | :next/:prev tabs | :sysdb system dbs | F5/Ctrl+J exec | Tab focus | r refresh";
        self.current_tab_mut().status_message = Some(help.to_string());
    }

    fn toggle_system_databases(&mut self) {
        let tab = self.current_tab_mut();
        tab.show_system_databases = !tab.show_system_databases;
        let show = tab.show_system_databases;

        // Only refresh if we have a db_client
        if tab.db_client.is_some() {
            self.refresh_databases();
            let msg = if show {
                "Showing system databases"
            } else {
                "Hiding system databases"
            };
            self.current_tab_mut().status_message = Some(msg.to_string());
        }
    }

    fn refresh_databases(&mut self) {
        let conn = {
            let tab = self.current_tab();
            match tab.connections.get(tab.selected_index) {
                Some(c) => c.clone(),
                None => return,
            }
        };

        let include_system = self.current_tab().show_system_databases;
        let db_name = conn.default_database();

        let result: Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> =
            self.runtime.block_on(async {
                let client = conn.create_client(&db_name).await?;
                client.list_databases(include_system).await
            });

        let tab = self.current_tab_mut();
        match result {
            Ok(dbs) => {
                // Preserve current database if still in list
                let current = tab.current_database.clone();
                tab.databases = dbs;
                if let Some(ref db) = current {
                    if !tab.databases.contains(db) {
                        tab.current_database = tab.databases.first().cloned();
                    }
                }
                // Clear expanded state and tables cache for databases no longer shown
                tab.sidebar.expanded.retain(|db| tab.databases.contains(db));
                tab.sidebar.tables.retain(|db, _| tab.databases.contains(db));
                tab.rebuild_sidebar_items();
            }
            Err(e) => {
                tab.status_message = Some(format!("Failed to refresh databases: {}", e));
            }
        }
    }
}
