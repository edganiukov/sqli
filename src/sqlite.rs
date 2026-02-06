use crate::db::QueryResult;
use crate::error::{Result, SqliError};

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use rusqlite::types::Value;

pub struct SqliteClient {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl SqliteClient {
    pub async fn connect(path: &str) -> Result<Self> {
        let path = PathBuf::from(path);

        let conn =
            Connection::open(&path).map_err(|e| SqliError::Connection(format!("SQLite: {}", e)))?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| SqliError::Connection(format!("SQLite PRAGMA: {}", e)))?;

        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    pub async fn list_databases(&self, _include_system: bool) -> Result<Vec<String>> {
        // SQLite is a single-database file — return the filename as the database name
        let name = self
            .path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string());
        Ok(vec![name])
    }

    pub async fn list_tables(&self, _schema: &str) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| SqliError::Other(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name",
            )
            .map_err(|e| SqliError::Query(e.to_string()))?;

        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| SqliError::Query(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tables)
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        // Use block_in_place to signal to tokio that this is a blocking operation
        // This prevents blocking the async runtime's worker threads
        let query = query.to_string();
        let conn_guard = self
            .conn
            .lock()
            .map_err(|e| SqliError::Other(e.to_string()))?;

        tokio::task::block_in_place(|| {
            let query_upper = query.trim().to_uppercase();

            if query_upper.starts_with("SELECT")
                || query_upper.starts_with("PRAGMA")
                || query_upper.starts_with("WITH")
                || query_upper.starts_with("EXPLAIN")
            {
                let mut stmt = conn_guard
                    .prepare(&query)
                    .map_err(|e| SqliError::Query(e.to_string()))?;

                let columns: Vec<String> =
                    stmt.column_names().iter().map(|c| c.to_string()).collect();

                let rows: Vec<Vec<String>> = stmt
                    .query_map([], |row| {
                        let mut values = Vec::with_capacity(columns.len());
                        for i in 0..columns.len() {
                            values.push(Self::format_value(row.get_ref(i).ok()));
                        }
                        Ok(values)
                    })
                    .map_err(|e| SqliError::Query(e.to_string()))?
                    .filter_map(|r| r.ok())
                    .collect();

                Ok(QueryResult::Select { columns, rows })
            } else {
                let rows_affected = conn_guard
                    .execute(&query, [])
                    .map_err(|e| SqliError::Query(e.to_string()))?;

                Ok(QueryResult::Execute {
                    rows_affected: rows_affected as u64,
                })
            }
        })
    }

    pub fn select_table_query(&self, table: &str, limit: usize, _schema: Option<&str>) -> String {
        format!("SELECT * FROM \"{}\" LIMIT {}", table, limit)
    }

    pub fn describe_table_query(&self, table: &str, _schema: Option<&str>) -> String {
        format!("PRAGMA table_info(\"{}\")", table)
    }

    pub async fn list_columns(&self, table: &str, _schema: Option<&str>) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| SqliError::Other(e.to_string()))?;
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{}\")", table))
            .map_err(|e| SqliError::Query(e.to_string()))?;

        let columns: Vec<String> = stmt
            .query_map([], |row| row.get(1)) // column 1 is "name"
            .map_err(|e| SqliError::Query(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(columns)
    }

    fn format_value(value: Option<rusqlite::types::ValueRef<'_>>) -> String {
        match value {
            None => "NULL".to_string(),
            Some(v) => match Value::from(v) {
                Value::Null => "NULL".to_string(),
                Value::Integer(i) => i.to_string(),
                Value::Real(f) => f.to_string(),
                Value::Text(s) => s,
                Value::Blob(b) => {
                    if b.len() <= 32 {
                        format!("x'{}'", hex::encode(&b))
                    } else {
                        format!("<blob: {} bytes>", b.len())
                    }
                }
            },
        }
    }
}
