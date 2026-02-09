use super::{Controller, DatabaseType, Focus, PendingOperation};
use crate::db::{DatabaseClient, QueryResult};
use crate::debug_log;
use crate::error::SqliError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);

impl Controller {
    /// Step 1: User selects a connection - either connect directly (if database configured)
    /// or fetch list of databases for selection
    pub fn initiate_connection(&mut self) {
        let (conn, actual_index) = {
            let tab = self.current_tab();
            let filtered = tab.filtered_connections();
            let conn = match filtered.get(tab.selected_index) {
                Some(c) => (*c).clone(),
                None => return,
            };

            // Find the actual index in connections vec for later lookups
            let actual_index = tab
                .connections
                .iter()
                .position(|c| c.name == conn.name)
                .unwrap_or(0);
            (conn, actual_index)
        };

        // Store the actual index for database operations after connection
        let tab = self.current_tab_mut();
        tab.connected_index = actual_index;

        debug_log!(
            "Initiating connection to {} ({}://{}:{})",
            conn.name,
            conn.db_type.as_str(),
            conn.host,
            conn.port
        );

        // If database is configured, connect directly
        if let Some(db_name) = conn.database.clone() {
            debug_log!("Database '{}' configured, connecting directly", db_name);
            self.connect_to_database(db_name);
            return;
        }

        // SQLite is a single-file database — connect directly using the file path
        if matches!(conn.db_type, DatabaseType::Sqlite) {
            let path = conn.path.as_deref().unwrap_or(&conn.host);
            let db_name = path.rsplit('/').next().unwrap_or(path).to_string();
            debug_log!("SQLite file '{}', connecting directly", path);
            self.connect_to_database(db_name);
            return;
        }

        // Otherwise, fetch database list for selection
        tab.status_message = Some("Connecting...".to_string());
        tab.loading = true;

        let include_system = tab.show_system_databases;
        let conn_name = conn.name.clone();

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let db_name = conn.db_type.default_database().to_string();
            let result = tokio::time::timeout(CONNECTION_TIMEOUT, async {
                let (client, pwd_warning) = conn.create_client(&db_name).await?;
                if let Some(warn) = pwd_warning {
                    crate::debug_log!("Password warning for {}: {}", conn.name, warn);
                }
                client.list_databases(include_system).await
            })
            .await
            .unwrap_or_else(|_| Err(SqliError::Connection("Connection timed out".to_string())));
            let _ = tx.send(result);
        });

        self.current_tab_mut().pending_operation = Some(PendingOperation::ListDatabases {
            receiver: rx,
            conn_name,
        });
    }

    /// Open database selection popup from DatabaseView (switch databases)
    pub(super) fn open_database_select(&mut self) {
        let tab = self.current_tab();
        if tab.view_state != super::ViewState::DatabaseView {
            self.current_tab_mut().status_message = Some("Not connected to a database".to_string());
            return;
        }

        // SQLite is single-file, no database switching
        let conn = match tab.connections.get(tab.connected_index) {
            Some(c) => c.clone(),
            None => return,
        };
        if matches!(conn.db_type, DatabaseType::Sqlite) {
            self.current_tab_mut().status_message =
                Some("SQLite does not support switching databases".to_string());
            return;
        }

        let tab = self.current_tab_mut();
        tab.loading = true;
        tab.status_message = Some("Fetching databases...".to_string());

        let include_system = tab.show_system_databases;
        let conn_name = conn.name.clone();

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let db_name = conn.db_type.default_database().to_string();
            let result = tokio::time::timeout(CONNECTION_TIMEOUT, async {
                let (client, pwd_warning) = conn.create_client(&db_name).await?;
                if let Some(warn) = pwd_warning {
                    crate::debug_log!("Password warning for {}: {}", conn.name, warn);
                }
                client.list_databases(include_system).await
            })
            .await
            .unwrap_or_else(|_| Err(SqliError::Connection("Connection timed out".to_string())));
            let _ = tx.send(result);
        });
        self.current_tab_mut().pending_operation = Some(PendingOperation::ListDatabases {
            receiver: rx,
            conn_name,
        });
    }

    /// Refresh the database list (used when toggling system databases)
    pub(super) fn refresh_database_list(&mut self) {
        let tab = self.current_tab_mut();
        let conn = match tab.connections.get(tab.connected_index) {
            Some(c) => c.clone(),
            None => return,
        };

        tab.status_message = Some("Refreshing...".to_string());
        tab.loading = true;

        let include_system = tab.show_system_databases;
        let conn_name = conn.name.clone();

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let db_name = conn.db_type.default_database().to_string();
            let result = tokio::time::timeout(CONNECTION_TIMEOUT, async {
                let (client, pwd_warning) = conn.create_client(&db_name).await?;
                if let Some(warn) = pwd_warning {
                    crate::debug_log!("Password warning for {}: {}", conn.name, warn);
                }
                client.list_databases(include_system).await
            })
            .await
            .unwrap_or_else(|_| Err(SqliError::Connection("Connection timed out".to_string())));
            let _ = tx.send(result);
        });

        self.current_tab_mut().pending_operation = Some(PendingOperation::ListDatabases {
            receiver: rx,
            conn_name,
        });
    }

    /// Step 2: User selects a database from list - connect and load tables
    pub(super) fn connect_to_selected_database_from_list(&mut self) {
        let tab = self.current_tab();
        let db_name = match tab.databases.get(tab.database_selected) {
            Some(db) => db.clone(),
            None => return,
        };
        self.connect_to_database(db_name);
    }

    /// Connect to a specific database and load tables
    pub(super) fn connect_to_database(&mut self, db_name: String) {
        let tab = self.current_tab_mut();
        let conn = match tab.connections.get(tab.connected_index) {
            Some(c) => c.clone(),
            None => return,
        };

        debug_log!(
            "Connecting to database '{}' on {} ({}://{}:{})",
            db_name,
            conn.name,
            conn.db_type.as_str(),
            conn.host,
            conn.port
        );

        tab.status_message = Some(format!("Connecting to {}...", db_name));
        tab.loading = true;

        let conn_name = conn.name.clone();
        let db_name_clone = db_name.clone();

        // Determine connection database and schema for listing tables
        let connect_db = db_name.clone();
        let schema = match conn.db_type {
            DatabaseType::Postgres => "public".to_string(),
            _ => db_name.clone(),
        };

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let result = tokio::time::timeout(CONNECTION_TIMEOUT, async {
                let (client, pwd_warning) = conn.create_client(&connect_db).await?;
                if let Some(warn) = pwd_warning {
                    crate::debug_log!("Password warning for {}: {}", conn.name, warn);
                }
                let tables = client.list_tables(&schema).await?;
                Ok((client, tables))
            })
            .await
            .unwrap_or_else(|_| Err(SqliError::Connection("Connection timed out".to_string())));
            let _ = tx.send(result);
        });

        self.current_tab_mut().pending_operation = Some(PendingOperation::Connect {
            receiver: rx,
            conn_name,
            db_name: db_name_clone,
        });
    }

    pub(super) fn execute_query(&mut self) {
        let raw_query: String = self.current_tab().query_textarea.lines().join("\n");
        let query = Self::strip_comments(&raw_query);
        self.run_query(&query);
    }

    /// Strip SQL comments while respecting string literals.
    /// Handles:
    /// - Single-line comments (-- ...)
    /// - Multi-line comments (/* ... */)
    /// - String literals ('...' and "...")
    fn strip_comments(query: &str) -> String {
        let mut result = String::with_capacity(query.len());
        let mut chars = query.chars().peekable();
        let mut in_single_quote = false;
        let mut in_double_quote = false;

        while let Some(ch) = chars.next() {
            // Handle string literals (SQL escapes quotes by doubling: '' or "")
            if ch == '\'' && !in_double_quote {
                if in_single_quote && chars.peek() == Some(&'\'') {
                    // Escaped quote inside string: '' — push both and stay in string
                    result.push(ch);
                    result.push(chars.next().unwrap());
                    continue;
                }
                in_single_quote = !in_single_quote;
                result.push(ch);
                continue;
            }
            if ch == '"' && !in_single_quote {
                if in_double_quote && chars.peek() == Some(&'"') {
                    // Escaped quote inside identifier: "" — push both and stay in string
                    result.push(ch);
                    result.push(chars.next().unwrap());
                    continue;
                }
                in_double_quote = !in_double_quote;
                result.push(ch);
                continue;
            }

            // Skip comments only when not in a string
            if !in_single_quote && !in_double_quote {
                // Single-line comment: --
                if ch == '-' && chars.peek() == Some(&'-') {
                    // Skip until end of line
                    for c in chars.by_ref() {
                        if c == '\n' {
                            result.push('\n'); // Preserve line structure
                            break;
                        }
                    }
                    continue;
                }
                // Multi-line comment: /* ... */
                if ch == '/' && chars.peek() == Some(&'*') {
                    chars.next(); // consume '*'
                    // Skip until */
                    let mut found_end = false;
                    while let Some(c) = chars.next() {
                        if c == '*' && chars.peek() == Some(&'/') {
                            chars.next(); // consume '/'
                            found_end = true;
                            break;
                        }
                        // Preserve newlines for line counting
                        if c == '\n' {
                            result.push('\n');
                        }
                    }
                    if !found_end {
                        // Unclosed comment - just skip to end
                    }
                    continue;
                }
            }

            result.push(ch);
        }

        result
    }

    pub(super) fn refresh_tables(&mut self) {
        let tab = self.current_tab();
        let client = match &tab.db_client {
            Some(c) => Arc::clone(c),
            None => return,
        };

        let conn = match tab.connections.get(tab.connected_index) {
            Some(c) => c.clone(),
            None => return,
        };

        let db_name = match &tab.current_database {
            Some(db) => db.clone(),
            None => return,
        };

        let schema = match conn.db_type {
            DatabaseType::Postgres => "public".to_string(),
            _ => db_name.clone(),
        };

        let tab = self.current_tab_mut();
        tab.loading = true;
        tab.status_message = Some("Refreshing...".to_string());

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let result = tokio::time::timeout(CONNECTION_TIMEOUT, client.list_tables(&schema))
                .await
                .unwrap_or_else(|_| Err(SqliError::Query("Refresh timed out".to_string())));
            let _ = tx.send(result);
        });

        self.current_tab_mut().pending_operation =
            Some(PendingOperation::RefreshTables { receiver: rx });
    }

    pub(super) fn select_table(&mut self) {
        if let Some(query) = self.table_query(|c, t, db| c.select_table_query(t, 50, Some(db))) {
            self.run_query(&query);
            self.current_tab_mut().focus = Focus::Output;
        }
    }

    pub(super) fn describe_table(&mut self) {
        if let Some(query) = self.table_query(|c, t, db| c.describe_table_query(t, Some(db))) {
            self.run_query(&query);
            self.current_tab_mut().focus = Focus::Output;
        }
    }

    /// Helper to generate a query for the currently selected table
    fn table_query<F>(&self, f: F) -> Option<String>
    where
        F: FnOnce(&crate::db::DatabaseClient, &str, &str) -> String,
    {
        let tab = self.current_tab();
        let table = tab.sidebar.tables.get(tab.sidebar.selected)?;
        let db_name = tab.current_database.as_ref()?;
        let client = tab.db_client.as_ref()?;
        Some(f(client, table, db_name))
    }

    /// Split query text into individual statements by semicolon.
    /// Handles semicolons inside string literals (both single and double quotes).
    fn split_queries(query: &str) -> Vec<String> {
        let mut statements = Vec::new();
        let mut current = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut chars = query.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '\'' if !in_double_quote => {
                    if in_single_quote && chars.peek() == Some(&'\'') {
                        // Escaped quote: '' — push both and stay in string
                        current.push(ch);
                        current.push(chars.next().unwrap());
                    } else {
                        in_single_quote = !in_single_quote;
                        current.push(ch);
                    }
                }
                '"' if !in_single_quote => {
                    if in_double_quote && chars.peek() == Some(&'"') {
                        // Escaped quote: "" — push both and stay in identifier
                        current.push(ch);
                        current.push(chars.next().unwrap());
                    } else {
                        in_double_quote = !in_double_quote;
                        current.push(ch);
                    }
                }
                ';' if !in_single_quote && !in_double_quote => {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        statements.push(trimmed);
                    }
                    current.clear();
                }
                _ => {
                    current.push(ch);
                }
            }
        }

        // Don't forget the last statement
        let trimmed = current.trim().to_string();
        if !trimmed.is_empty() {
            statements.push(trimmed);
        }

        statements
    }

    /// Execute a query string (may contain multiple statements separated by `;`)
    fn run_query(&mut self, query: &str) {
        let statements = Self::split_queries(query);
        if statements.is_empty() {
            return;
        }

        let tab = self.current_tab();
        let conn = match tab.connections.get(tab.connected_index) {
            Some(c) => c.clone(),
            None => return,
        };

        // Check readonly for all statements
        if conn.readonly {
            for stmt in &statements {
                if !Self::is_read_query(stmt) {
                    self.current_tab_mut().status_message =
                        Some("Connection is read-only, only select queries allowed".to_string());
                    return;
                }
            }
        }

        let client = match &tab.db_client {
            Some(c) => Arc::clone(c),
            None => {
                self.current_tab_mut().status_message = Some("Not connected".to_string());
                return;
            }
        };

        debug_log!(
            "Executing {} statement(s): {}",
            statements.len(),
            query.trim().replace('\n', " ")
        );

        {
            let tab = self.current_tab_mut();
            tab.loading = true;
            tab.status_message = Some("Executing...".to_string());
        }

        let start = std::time::Instant::now();

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let result = Self::execute_statements(&client, statements).await;
            let _ = tx.send(result);
        });

        self.current_tab_mut().pending_operation = Some(PendingOperation::Query {
            receiver: rx,
            start,
        });
    }

    /// Execute multiple statements sequentially, combining results
    async fn execute_statements(
        client: &DatabaseClient,
        statements: Vec<String>,
    ) -> crate::error::Result<QueryResult> {
        let mut last_select: Option<QueryResult> = None;
        let mut total_rows_affected: u64 = 0;

        for stmt in statements {
            let result = client.execute_query(&stmt).await?;
            match result {
                QueryResult::Select { .. } => {
                    last_select = Some(result);
                }
                QueryResult::Execute { rows_affected } => {
                    total_rows_affected += rows_affected;
                }
            }
        }

        // Return last SELECT result if any, otherwise return combined Execute result
        Ok(last_select.unwrap_or(QueryResult::Execute {
            rows_affected: total_rows_affected,
        }))
    }

    pub fn is_read_query(query: &str) -> bool {
        let first_word = query.split_whitespace().next().unwrap_or("").to_uppercase();

        matches!(
            first_word.as_str(),
            "SELECT" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "WITH" | "USE" | "HELP" | "LIST"
        )
    }
}
