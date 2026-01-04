use crate::db::QueryResult;
use serde_json::Value as JsonValue;
use tokio_postgres::types::Type;
use tokio_postgres::{Client, Error, NoTls, Row};

pub struct PostgresClient {
    client: Client,
}

impl PostgresClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
        database: &str,
    ) -> Result<Self, Error> {
        let conn_string = format!(
            "host={} port={} user={} password={} dbname={}",
            host, port, user, password, database
        );

        let (client, connection) = tokio_postgres::connect(&conn_string, NoTls).await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });

        Ok(Self { client })
    }

    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>, Error> {
        const SYSTEM_DATABASES: &[&str] = &["postgres", "template0", "template1"];

        let rows = self
            .client
            .query("SELECT datname FROM pg_database ORDER BY datname", &[])
            .await?;

        let databases: Vec<String> = rows
            .iter()
            .map(|row| row.get::<_, String>(0))
            .filter(|db| include_system || !SYSTEM_DATABASES.contains(&db.as_str()))
            .collect();

        Ok(databases)
    }

    pub async fn list_tables(&self, schema: &str) -> Result<Vec<String>, Error> {
        let rows = self
            .client
            .query(
                "SELECT tablename FROM pg_tables WHERE schemaname = $1 ORDER BY tablename",
                &[&schema],
            )
            .await?;

        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult, Error> {
        let query_upper = query.trim().to_uppercase();

        if query_upper.starts_with("SELECT") || query_upper.starts_with("WITH") {
            let rows = self.client.query(query, &[]).await?;

            if rows.is_empty() {
                return Ok(QueryResult::Select {
                    columns: vec![],
                    rows: vec![],
                });
            }

            let columns: Vec<String> =
                rows[0].columns().iter().map(|c| c.name().to_string()).collect();

            let data_rows: Vec<Vec<String>> = rows
                .iter()
                .map(|row| (0..row.len()).map(|i| Self::get_column_value(row, i)).collect())
                .collect();

            Ok(QueryResult::Select {
                columns,
                rows: data_rows,
            })
        } else {
            let rows_affected = self.client.execute(query, &[]).await?;
            Ok(QueryResult::Execute { rows_affected })
        }
    }

    /// Generate a SELECT query for previewing table contents
    pub fn select_table_query(table: &str, limit: usize) -> String {
        format!("SELECT * FROM {} LIMIT {};", table, limit)
    }

    /// Default database to connect to when none is selected
    pub fn default_database() -> &'static str {
        "postgres"
    }

    /// Generate a query to describe table structure (like \d+ in psql)
    pub fn describe_table_query(table: &str) -> String {
        format!(
            "SELECT column_name, data_type, is_nullable, column_default \n\
             FROM information_schema.columns \n\
             WHERE table_name = '{}' \n\
             ORDER BY ordinal_position;",
            table
        )
    }

    fn get_column_value(row: &Row, idx: usize) -> String {
        let col_type = row.columns()[idx].type_();

        // Try to get value based on known types
        let result: Result<String, _> = match *col_type {
            Type::BOOL => row
                .try_get::<_, Option<bool>>(idx)
                .map(|v| v.map(|b| b.to_string()).unwrap_or_else(|| "NULL".to_string())),
            Type::INT2 => row
                .try_get::<_, Option<i16>>(idx)
                .map(|v| v.map(|n| n.to_string()).unwrap_or_else(|| "NULL".to_string())),
            Type::INT4 | Type::OID => row
                .try_get::<_, Option<i32>>(idx)
                .map(|v| v.map(|n| n.to_string()).unwrap_or_else(|| "NULL".to_string())),
            Type::INT8 => row
                .try_get::<_, Option<i64>>(idx)
                .map(|v| v.map(|n| n.to_string()).unwrap_or_else(|| "NULL".to_string())),
            Type::FLOAT4 => row
                .try_get::<_, Option<f32>>(idx)
                .map(|v| v.map(|n| n.to_string()).unwrap_or_else(|| "NULL".to_string())),
            Type::FLOAT8 | Type::NUMERIC => row
                .try_get::<_, Option<f64>>(idx)
                .map(|v| v.map(|n| n.to_string()).unwrap_or_else(|| "NULL".to_string())),
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => row
                .try_get::<_, Option<String>>(idx)
                .map(|v| v.unwrap_or_else(|| "NULL".to_string())),
            Type::JSON | Type::JSONB => row
                .try_get::<_, Option<JsonValue>>(idx)
                .map(|v| v.map(|j| j.to_string()).unwrap_or_else(|| "NULL".to_string())),
            _ => {
                // For unsupported types, try string first, then show type name
                row.try_get::<_, Option<String>>(idx)
                    .map(|v| v.unwrap_or_else(|| "NULL".to_string()))
                    .or_else(|_| Ok(format!("<{}>", col_type.name())))
            }
        };

        result.unwrap_or_else(|_| format!("<{}>", col_type.name()))
    }
}
