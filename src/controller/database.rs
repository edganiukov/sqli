use super::{Controller, DatabaseType, Focus, PendingOperation};
use crate::debug_log;
use std::sync::Arc;
use tokio::sync::oneshot;
use tui_textarea::TextArea;

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
            let result = async {
                let client = conn.create_client(&db_name).await?;
                client.list_databases(include_system).await
            }
            .await;
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
        let connect_db = match conn.db_type {
            DatabaseType::Cassandra => String::new(),
            _ => db_name.clone(),
        };
        let schema = match conn.db_type {
            DatabaseType::Postgres => "public".to_string(),
            _ => db_name.clone(),
        };

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let result = async {
                let client = conn.create_client(&connect_db).await?;
                let tables = client.list_tables(&schema).await?;
                Ok((client, tables))
            }
            .await;
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
        let query: String = self.query_textarea.lines().join("\n");
        if query.trim().is_empty() {
            return;
        }

        let tab = self.current_tab();
        let conn = match tab.connections.get(tab.selected_index) {
            Some(c) => c.clone(),
            None => return,
        };

        // Block non-read operations on readonly connections
        if conn.readonly && !Self::is_read_query(&query) {
            debug_log!(
                "Blocked write query on readonly connection: {}",
                query.trim()
            );
            self.current_tab_mut().status_message =
                Some("Connection is read-only, only SELECT queries allowed".to_string());
            return;
        }

        let client = match &tab.db_client {
            Some(c) => Arc::clone(c),
            None => {
                self.current_tab_mut().status_message = Some("Not connected".to_string());
                return;
            }
        };

        let db_name = tab.current_database.clone().unwrap_or_default();

        debug_log!(
            "Executing query on database '{}': {}",
            db_name,
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
            let result = client.execute_query(&query).await;
            let _ = tx.send(result);
        });

        self.pending_operation = Some(PendingOperation::Query {
            receiver: rx,
            start,
        });
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
            let result = client.list_tables(&schema).await;
            let _ = tx.send(result);
        });

        self.pending_operation = Some(PendingOperation::RefreshTables { receiver: rx });
    }

    pub(super) fn select_table(&mut self) {
        let tab = self.current_tab();
        let table = match tab.sidebar.tables.get(tab.sidebar.selected) {
            Some(t) => t.clone(),
            None => return,
        };

        let db_name = match &tab.current_database {
            Some(db) => db.clone(),
            None => return,
        };

        debug_log!("Selected table '{}' from database '{}'", table, db_name);

        let query = tab
            .db_client
            .as_ref()
            .map(|c| c.select_table_query(&table, 50, Some(&db_name)))
            .unwrap_or_default();

        debug_log!("Generated query: {}", query);
        self.query_textarea.select_all();
        self.query_textarea.cut();
        self.query_textarea.insert_str(&query);
        self.execute_query();
        self.current_tab_mut().focus = Focus::Output;
    }

    pub(super) fn describe_table(&mut self) {
        let tab = self.current_tab();
        let table = match tab.sidebar.tables.get(tab.sidebar.selected) {
            Some(t) => t.clone(),
            None => return,
        };

        let db_name = match &tab.current_database {
            Some(db) => db.clone(),
            None => return,
        };

        let query = tab
            .db_client
            .as_ref()
            .map(|c| c.describe_table_query(&table, Some(&db_name)))
            .unwrap_or_default();

        self.query_textarea.select_all();
        self.query_textarea.cut();
        self.query_textarea.insert_str(&query);
        self.execute_query();
        self.current_tab_mut().focus = Focus::Output;
    }

    pub fn is_read_query(query: &str) -> bool {
        let first_word = query.split_whitespace().next().unwrap_or("").to_uppercase();

        matches!(
            first_word.as_str(),
            "SELECT" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "WITH" | "USE" | "HELP" | "LIST"
        )
    }
}
