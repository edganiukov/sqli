use super::{Controller, DatabaseType, Focus, PendingOperation};
use crate::db::{DatabaseClient, QueryResult};
use crate::debug_log;
use crate::error::SqliError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tui_textarea::TextArea;

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);

impl Controller {
    /// Step 1: User selects a connection - either connect directly (if database configured)
    /// or fetch list of databases for selection
    pub(super) fn initiate_connection(&mut self) {
        let tab = self.current_tab_mut();
        let conn = match tab.connections.get(tab.selected_index) {
            Some(c) => c.clone(),
            None => return,
        };

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

        // Otherwise, fetch database list for selection
        tab.status_message = Some("Connecting...".to_string());
        tab.loading = true;

        let include_system = tab.show_system_databases;
        let conn_name = conn.name.clone();

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let db_name = conn.db_type.default_database().to_string();
            let result = tokio::time::timeout(CONNECTION_TIMEOUT, async {
                let client = conn.create_client(&db_name).await?;
                client.list_databases(include_system).await
            })
            .await
            .unwrap_or_else(|_| Err(SqliError::Connection("Connection timed out".to_string())));
            let _ = tx.send(result);
        });

        self.pending_operation = Some(PendingOperation::ListDatabases {
            receiver: rx,
            conn_name,
        });
    }

    /// Refresh the database list (used when toggling system databases)
    pub(super) fn refresh_database_list(&mut self) {
        let tab = self.current_tab_mut();
        let conn = match tab.connections.get(tab.selected_index) {
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
                let client = conn.create_client(&db_name).await?;
                client.list_databases(include_system).await
            })
            .await
            .unwrap_or_else(|_| Err(SqliError::Connection("Connection timed out".to_string())));
            let _ = tx.send(result);
        });

        self.pending_operation = Some(PendingOperation::ListDatabases {
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
    fn connect_to_database(&mut self, db_name: String) {
        let tab = self.current_tab_mut();
        let conn = match tab.connections.get(tab.selected_index) {
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
                let client = conn.create_client(&connect_db).await?;
                let tables = client.list_tables(&schema).await?;
                Ok((client, tables))
            })
            .await
            .unwrap_or_else(|_| Err(SqliError::Connection("Connection timed out".to_string())));
            let _ = tx.send(result);
        });

        self.pending_operation = Some(PendingOperation::Connect {
            receiver: rx,
            conn_name,
            db_name: db_name_clone,
        });
        self.query_textarea = TextArea::default();
    }

    pub(super) fn execute_query(&mut self) {
        let query: String = self
            .query_textarea
            .lines()
            .iter()
            .filter(|line| !line.trim_start().starts_with("--"))
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.run_query(&query);
    }

    pub(super) fn refresh_tables(&mut self) {
        let tab = self.current_tab();
        let client = match &tab.db_client {
            Some(c) => Arc::clone(c),
            None => return,
        };

        let conn = match tab.connections.get(tab.selected_index) {
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

        self.pending_operation = Some(PendingOperation::RefreshTables { receiver: rx });
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

    /// Split query text into individual statements by semicolon
    fn split_queries(query: &str) -> Vec<String> {
        query
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Execute a query string (may contain multiple statements separated by `;`)
    fn run_query(&mut self, query: &str) {
        let statements = Self::split_queries(query);
        if statements.is_empty() {
            return;
        }

        let tab = self.current_tab();
        let conn = match tab.connections.get(tab.selected_index) {
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

        self.pending_operation = Some(PendingOperation::Query {
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
