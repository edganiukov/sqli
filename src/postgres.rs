use crate::db::QueryResult;
use crate::error::Result;
use crate::format as fmt;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde_json::Value as JsonValue;
use tokio_postgres::types::Type;
use tokio_postgres::{Client, NoTls, Row};

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
    ) -> Result<Self> {
        crate::debug_log!(
            "Postgres connecting to {}:{} database='{}' user='{}'",
            host,
            port,
            database,
            user
        );
        let conn_string = format!(
            "host={} port={} user={} password={} dbname={}",
            host, port, user, password, database
        );

        let (client, connection) = tokio_postgres::connect(&conn_string, NoTls).await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                crate::debug_log!("Postgres connection error: {}", e);
            }
        });

        Ok(Self { client })
    }

    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>> {
        const SYSTEM_DATABASES: &[&str] = &["template0", "template1"];

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

    pub async fn list_tables(&self, schema: &str) -> Result<Vec<String>> {
        crate::debug_log!("Postgres listing tables for schema '{}'", schema);
        let rows = self
            .client
            .query(
                "SELECT tablename FROM pg_tables WHERE schemaname = $1 ORDER BY tablename",
                &[&schema],
            )
            .await?;

        let tables: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
        crate::debug_log!("Found {} tables: {:?}", tables.len(), tables);
        Ok(tables)
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        crate::debug_log!("Postgres executing: {}", query.trim().replace('\n', " "));
        let query_upper = query.trim().to_uppercase();

        if query_upper.starts_with("SELECT") || query_upper.starts_with("WITH") {
            let rows = self.client.query(query, &[]).await?;

            if rows.is_empty() {
                return Ok(QueryResult::Select {
                    columns: vec![],
                    rows: vec![],
                });
            }

            let columns: Vec<String> = rows[0]
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect();

            let data_rows: Vec<Vec<String>> = rows
                .iter()
                .map(|row| {
                    (0..row.len())
                        .map(|i| Self::get_column_value(row, i))
                        .collect()
                })
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

    pub fn select_table_query(&self, table: &str, limit: usize, _schema: Option<&str>) -> String {
        format!("SELECT * FROM \"{}\" LIMIT {}", table, limit)
    }

    pub fn describe_table_query(&self, table: &str, _schema: Option<&str>) -> String {
        format!(
            "SELECT column_name, data_type, is_nullable, column_default \n\
             FROM information_schema.columns \n\
             WHERE table_name = '{}' \n\
             ORDER BY ordinal_position",
            table
        )
    }

    pub async fn list_columns(&self, table: &str) -> Result<Vec<String>> {
        let query = format!(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = '{}' ORDER BY ordinal_position",
            table
        );
        let rows = self.client.query(&query, &[]).await?;
        Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
    }

    fn get_column_value(row: &Row, idx: usize) -> String {
        let col_type = row.columns()[idx].type_();

        // Helper macro to reduce boilerplate for simple types
        macro_rules! get_value {
            ($t:ty) => {
                row.try_get::<_, Option<$t>>(idx).map(fmt::null_or)
            };
            ($t:ty, $f:expr) => {
                row.try_get::<_, Option<$t>>(idx)
                    .map(|v| fmt::null_or_else(v, $f))
            };
        }

        let result: std::result::Result<String, _> = match *col_type {
            Type::BOOL => get_value!(bool),
            Type::INT2 => get_value!(i16),
            Type::INT4 | Type::OID => get_value!(i32),
            Type::INT8 => get_value!(i64),
            Type::FLOAT4 => get_value!(f32),
            Type::FLOAT8 => get_value!(f64),
            Type::NUMERIC => get_value!(rust_decimal::Decimal),
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => row
                .try_get::<_, Option<String>>(idx)
                .map(|v| v.unwrap_or_else(|| "NULL".to_string())),
            Type::JSON | Type::JSONB => get_value!(JsonValue),
            Type::TIMESTAMP => get_value!(NaiveDateTime, |t: NaiveDateTime| t
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string()),
            Type::TIMESTAMPTZ => get_value!(DateTime<Utc>, |t: DateTime<Utc>| t
                .format("%Y-%m-%d %H:%M:%S%.3f %Z")
                .to_string()),
            Type::DATE => get_value!(NaiveDate, |d: NaiveDate| d.format("%Y-%m-%d").to_string()),
            Type::TIME => get_value!(NaiveTime, |t: NaiveTime| t
                .format("%H:%M:%S%.3f")
                .to_string()),
            Type::BYTEA => get_value!(Vec<u8>, |b: Vec<u8>| fmt::bytes(&b, 32)),
            Type::TEXT_ARRAY => {
                get_value!(Vec<String>, |arr: Vec<String>| format!(
                    "{{{}}}",
                    arr.join(",")
                ))
            }
            Type::INT4_ARRAY => get_value!(Vec<i32>, |arr: Vec<i32>| {
                format!(
                    "{{{}}}",
                    arr.iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }),
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
