use crossterm::event::{KeyCode, KeyEvent};
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

#[derive(Debug, Clone)]
pub enum DatabaseType {
    Postgres,
    Cassandra,
}

impl DatabaseType {
    pub fn as_str(&self) -> &str {
        match self {
            DatabaseType::Postgres => "PostgreSQL",
            DatabaseType::Cassandra => "Cassandra",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseConn {
    pub name: String,
    pub db_type: DatabaseType,
    pub host: String,
    pub port: u16,
    pub database: String,
}

pub struct Tab {
    pub name: String,
    pub connections: Vec<DatabaseConn>,
    pub selected_index: usize,
    pub connected_db: Option<String>,
    pub view_state: ViewState,
}

impl Tab {
    pub fn new(connections: Vec<DatabaseConn>) -> Self {
        Self {
            name: "New".to_string(),
            connections,
            selected_index: 0,
            connected_db: None,
            view_state: ViewState::ConnectionList,
        }
    }

    pub fn connect_to_selected(&mut self) {
        if let Some(conn) = self.connections.get(self.selected_index) {
            self.name = conn.name.clone();
            self.connected_db = Some(conn.name.clone());
            self.view_state = ViewState::DatabaseView;
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
}

pub struct Controller {
    pub mode: Mode,
    pub command_buffer: String,
    pub query_textarea: TextArea<'static>,
    pub tabs: Vec<Tab>,
    pub current_tab: usize,
    pub quit: bool,
}

impl Controller {
    pub fn new() -> Self {
        // TODO: load from the config.
        let connections = vec![
            DatabaseConn {
                name: "Local Postgres".to_string(),
                db_type: DatabaseType::Postgres,
                host: "localhost".to_string(),
                port: 5432,
                database: "mydb".to_string(),
            },
            DatabaseConn {
                name: "Dev Cassandra".to_string(),
                db_type: DatabaseType::Cassandra,
                host: "dev.example.com".to_string(),
                port: 9042,
                database: "dev_keyspace".to_string(),
            },
        ];

        Self {
            mode: Mode::Normal,
            command_buffer: String::new(),
            query_textarea: TextArea::default(),
            tabs: vec![Tab::new(connections)],
            current_tab: 0,
            quit: false,
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
                self.current_tab_mut().connect_to_selected();
                self.query_textarea = TextArea::default();
            }
            _ => {}
        }
    }

    pub fn handle_database_view_keys(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            _ => {
                self.query_textarea.input(key_event);
            }
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
            _ => {}
        }
    }
}
