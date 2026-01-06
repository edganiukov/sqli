use crate::db::QueryResult;
use scylla::frame::response::result::CqlValue;
use scylla::{Session, SessionBuilder};
use std::error::Error;
use std::sync::Arc;

pub struct CassandraClient {
    session: Arc<Session>,
}

impl CassandraClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
        keyspace: &str,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let addr = format!("{}:{}", host, port);

        let mut builder = SessionBuilder::new().known_node(&addr);

        if !user.is_empty() {
            builder = builder.user(user, password);
        }

        let session = builder.build().await?;

        // Use keyspace if provided
        if !keyspace.is_empty() {
            session.query_unpaged(format!("USE {}", keyspace), &[]).await?;
        }

        Ok(Self {
            session: Arc::new(session),
        })
    }

    /// List all keyspaces (equivalent to databases in PostgreSQL)
    pub async fn list_databases(
        &self,
        include_system: bool,
    ) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let rows = self
            .session
            .query_unpaged("SELECT keyspace_name FROM system_schema.keyspaces", &[])
            .await?;

        const SYSTEM_KEYSPACES: &[&str] = &[
            "system",
            "system_schema",
            "system_traces",
            "system_auth",
            "system_distributed",
            "system_distributed_everywhere",
            "system_virtual_schema",
            "system_replicated_keys",
        ];

        let mut keyspaces = Vec::new();
        if let Some(rows) = rows.rows {
            for row in rows {
                if let Some(CqlValue::Text(s) | CqlValue::Ascii(s)) = row.columns.get(0).and_then(|v| v.as_ref()) {
                    if include_system || !SYSTEM_KEYSPACES.contains(&s.as_str()) {
                        keyspaces.push(s.clone());
                    }
                }
            }
        }
        keyspaces.sort();
        Ok(keyspaces)
    }

    /// List tables in a keyspace
    pub async fn list_tables(
        &self,
        keyspace: &str,
    ) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let rows = self
            .session
            .query_unpaged(
                "SELECT keyspace_name, table_name FROM system_schema.tables",
                &[],
            )
            .await?;

        let mut tables = Vec::new();
        if let Some(rows) = rows.rows {
            for row in rows {
                // Check if keyspace matches
                if let Some(CqlValue::Text(ks) | CqlValue::Ascii(ks)) = row.columns.get(0).and_then(|v| v.as_ref()) {
                    if ks == keyspace {
                        if let Some(CqlValue::Text(t) | CqlValue::Ascii(t)) = row.columns.get(1).and_then(|v| v.as_ref())
                        {
                            tables.push(t.clone());
                        }
                    }
                }
            }
        }
        tables.sort();
        Ok(tables)
    }

    pub async fn execute_query(
        &self,
        query: &str,
    ) -> Result<QueryResult, Box<dyn Error + Send + Sync>> {
        let query_upper = query.trim().to_uppercase();

        if query_upper.starts_with("SELECT") {
            let result = self.session.query_unpaged(query, &[]).await?;

            let columns: Vec<String> =
                result.col_specs().iter().map(|spec| spec.name.clone()).collect();

            let mut data_rows: Vec<Vec<String>> = Vec::new();
            if let Some(rows) = result.rows {
                for row in rows {
                    let row_data: Vec<String> =
                        row.columns.iter().map(|col| Self::format_column_value(col)).collect();
                    data_rows.push(row_data);
                }
            }

            Ok(QueryResult::Select {
                columns,
                rows: data_rows,
            })
        } else {
            // For non-SELECT queries, execute and report success
            self.session.query_unpaged(query, &[]).await?;
            Ok(QueryResult::Execute { rows_affected: 0 })
        }
    }

    fn format_column_value(value: &Option<CqlValue>) -> String {
        match value {
            None => "NULL".to_string(),
            Some(v) => match v {
                CqlValue::Ascii(s) => s.clone(),
                CqlValue::Text(s) => s.clone(),
                CqlValue::Int(i) => i.to_string(),
                CqlValue::BigInt(i) => i.to_string(),
                CqlValue::Float(f) => f.to_string(),
                CqlValue::Double(d) => d.to_string(),
                CqlValue::Boolean(b) => b.to_string(),
                CqlValue::Uuid(u) => u.to_string(),
                CqlValue::Timestamp(t) => format!("{:?}", t),
                CqlValue::Blob(b) => format!("<blob {} bytes>", b.len()),
                CqlValue::List(l) => format!("[{} items]", l.len()),
                CqlValue::Map(m) => format!("{{map {} entries}}", m.len()),
                CqlValue::Set(s) => format!("{{set {} items}}", s.len()),
                CqlValue::SmallInt(i) => i.to_string(),
                CqlValue::TinyInt(i) => i.to_string(),
                CqlValue::Inet(addr) => addr.to_string(),
                CqlValue::Date(d) => format!("{:?}", d),
                CqlValue::Time(t) => format!("{:?}", t),
                CqlValue::Varint(v) => format!("{:?}", v),
                CqlValue::Decimal(d) => format!("{:?}", d),
                _ => format!("{:?}", v),
            },
        }
    }

    /// Generate a SELECT query for previewing table contents
    pub fn select_table_query(table: &str, limit: usize) -> String {
        format!("SELECT * FROM {} LIMIT {};", table, limit)
    }

    /// Generate a query to describe table structure
    pub fn describe_table_query(table: &str, keyspace: Option<&str>) -> String {
        match keyspace {
            Some(ks) => format!(
                "SELECT column_name, type, kind \n\
                 FROM system_schema.columns \n\
                 WHERE keyspace_name = '{}' AND table_name = '{}';",
                ks, table
            ),
            None => format!(
                "SELECT column_name, type, kind \n\
                 FROM system_schema.columns \n\
                 WHERE table_name = '{}';",
                table
            ),
        }
    }

    /// Default keyspace to connect to when none is selected (empty = no keyspace)
    pub fn default_database() -> &'static str {
        ""
    }
}
