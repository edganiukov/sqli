use super::{Controller, DatabaseType, PendingOperation, SidebarItem};
use crate::debug_log;
use tokio::sync::oneshot;
use tui_textarea::TextArea;

impl Controller {
    pub(super) fn connect_to_database(&mut self) {
        let tab = self.current_tab_mut();
        let conn = match tab.connections.get(tab.selected_index) {
            Some(c) => c.clone(),
            None => return,
        };

        debug_log!(
            "Connecting to {} ({}://{}:{})",
            conn.name,
            conn.db_type.as_str(),
            conn.host,
            conn.port
        );

        tab.status_message = Some("Connecting...".to_string());
        tab.loading = true;
        tab.name = conn.name.clone();
        tab.connected_db = Some(conn.name.clone());

        let include_system = tab.show_system_databases;
        let conn_name = conn.name.clone();

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let db_name = conn.db_type.default_database().to_string();
            let result = async {
                let client = conn.create_client(&db_name).await?;
                let dbs = client.list_databases(include_system).await?;
                Ok((client, dbs, db_name))
            }
            .await;
            let _ = tx.send(result);
        });

        self.pending_operation = Some(PendingOperation::Connect {
            receiver: rx,
            conn_name,
        });
        self.query_textarea = TextArea::default();
    }

    pub(super) fn load_tables_for_database(&mut self, db_name: &str) {
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

        // Check if we can reuse existing client
        let existing_client = {
            let tab = self.current_tab();
            if tab.client_database.as_ref() == Some(&connect_db) {
                tab.db_client.clone()
            } else {
                None
            }
        };

        let tab = self.current_tab_mut();
        tab.loading = true;
        tab.status_message = Some("Loading tables...".to_string());

        let db_name_clone = db_name.clone();
        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let result = async {
                if let Some(client) = existing_client {
                    let tables = client.list_tables(&schema).await?;
                    Ok((tables, None))
                } else {
                    let client = conn.create_client(&connect_db).await?;
                    let tables = client.list_tables(&schema).await?;
                    Ok((tables, Some(client)))
                }
            }
            .await;
            let _ = tx.send(result);
        });

        self.pending_operation = Some(PendingOperation::LoadTables {
            receiver: rx,
            db_name: db_name_clone,
        });
    }

    pub(super) fn execute_query(&mut self) {
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

        // Block non-read operations on readonly connections
        if conn.readonly && !Self::is_read_query(&query) {
            debug_log!("Blocked write query on readonly connection: {}", query.trim());
            self.current_tab_mut().status_message =
                Some("Connection is read-only, only SELECT queries allowed".to_string());
            return;
        }

        // Use selected database or fall back to default
        let (db_name, using_default) = match self.current_tab().current_database.clone() {
            Some(db) => (db, false),
            None => (conn.db_type.default_database().to_string(), true),
        };

        // Determine connection database (differs from query database for some DB types)
        // - Cassandra: always connect without keyspace, USE keyspace in queries
        // - Others: connect to specific database
        let connect_db = match conn.db_type {
            DatabaseType::Cassandra => String::new(),
            _ => db_name.clone(),
        };

        debug_log!(
            "Executing query on database '{}' (connect_db: '{}', host: {}:{}): {}",
            db_name,
            connect_db,
            conn.host,
            conn.port,
            query.trim().replace('\n', " ")
        );

        {
            let tab = self.current_tab_mut();
            tab.loading = true;
            if using_default {
                let msg = if db_name.is_empty() {
                    "No keyspace selected, using none...".to_string()
                } else {
                    format!("No database selected, using '{}'...", db_name)
                };
                tab.status_message = Some(msg);
            } else {
                tab.status_message = Some("Executing...".to_string());
            }
        }

        let start = std::time::Instant::now();
        let connect_db_clone = connect_db.clone();

        // Check if we can reuse existing client (compare against connect_db, not db_name)
        let existing_client = {
            let tab = self.current_tab();
            if tab.client_database.as_ref() == Some(&connect_db) {
                tab.db_client.clone()
            } else {
                None
            }
        };

        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let result = async {
                if let Some(client) = existing_client {
                    let res = client.execute_query(&query).await?;
                    Ok((res, None))
                } else {
                    let client = conn.create_client(&connect_db_clone).await?;
                    let res = client.execute_query(&query).await?;
                    Ok((res, Some(client)))
                }
            }
            .await;
            let _ = tx.send(result);
        });

        self.pending_operation = Some(PendingOperation::Query {
            receiver: rx,
            db_name: connect_db, // Track the connection database, not query database
            start,
        });
    }

    pub(super) fn refresh_databases(&mut self) {
        let conn = {
            let tab = self.current_tab();
            match tab.connections.get(tab.selected_index) {
                Some(c) => c.clone(),
                None => return,
            }
        };

        let include_system = self.current_tab().show_system_databases;
        let db_name = conn.db_type.default_database().to_string();

        // Try to reuse existing client
        let existing_client = {
            let tab = self.current_tab();
            if tab.client_database.as_ref() == Some(&db_name) {
                tab.db_client.clone()
            } else {
                None
            }
        };

        let result = self.runtime.block_on(async {
            if let Some(client) = existing_client {
                client.list_databases(include_system).await
            } else {
                let client = conn.create_client(&db_name).await?;
                client.list_databases(include_system).await
            }
        });

        let tab = self.current_tab_mut();
        match result {
            Ok(dbs) => {
                debug_log!("Refreshed database list: {} database(s)", dbs.len());
                // Preserve current database if still in list
                let current = tab.current_database.clone();
                tab.databases = dbs;
                if let Some(ref db) = current
                    && !tab.databases.contains(db)
                {
                    tab.current_database = tab.databases.first().cloned();
                }
                // Clear expanded state and tables cache for databases no longer shown
                tab.sidebar.expanded.retain(|db| tab.databases.contains(db));
                tab.sidebar
                    .tables
                    .retain(|db, _| tab.databases.contains(db));
                tab.rebuild_sidebar_items();
            }
            Err(e) => {
                debug_log!("Failed to refresh databases: {}", e);
                tab.status_message = Some(format!("Failed to refresh databases: {}", e));
            }
        }
    }

    pub(super) fn toggle_sidebar_item(&mut self) {
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
                    self.current_tab_mut()
                        .sidebar
                        .expanded
                        .insert(db_name.clone());
                    self.load_tables_for_database(&db_name);
                }
                self.current_tab_mut().rebuild_sidebar_items();
            }
            SidebarItem::Table { database, table } => {
                debug_log!(
                    "Selected table '{}' from database '{}'",
                    table,
                    database
                );
                // Set parent database as current
                self.current_tab_mut().current_database = Some(database.clone());
                // Populate query and execute
                let query = self
                    .current_tab()
                    .db_client
                    .as_ref()
                    .map(|c| c.select_table_query(&table, 50))
                    .unwrap_or_default();
                debug_log!("Generated query: {}", query);
                self.query_textarea.select_all();
                self.query_textarea.cut();
                self.query_textarea.insert_str(&query);
                self.execute_query();
                self.current_tab_mut().focus = super::Focus::Output;
            }
        }
    }

    pub(super) fn describe_table(&mut self) {
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
            self.current_tab_mut().focus = super::Focus::Output;
        }
    }

    pub fn is_read_query(query: &str) -> bool {
        let first_word = query.split_whitespace().next().unwrap_or("").to_uppercase();

        matches!(
            first_word.as_str(),
            "SELECT" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "WITH" | "USE" | "HELP" | "LIST"
        )
    }
}
