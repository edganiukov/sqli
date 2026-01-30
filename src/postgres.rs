use crate::db::QueryResult;
use crate::error::Result;
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
            .query("select datname from pg_database order by datname", &[])
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
                "select tablename from pg_tables where schemaname = $1 order by tablename",
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
        format!("select * from \"{}\" limit {};", table, limit)
    }

    pub fn describe_table_query(&self, table: &str, _schema: Option<&str>) -> String {
        format!(
            "select column_name, data_type, is_nullable, column_default \n\
             from information_schema.columns \n\
             where table_name = '{}' \n\
             order by ordinal_position;",
            table
        )
    }

    fn get_column_value(row: &Row, idx: usize) -> String {
        let col_type = row.columns()[idx].type_();

        let result: std::result::Result<String, _> = match *col_type {
            Type::BOOL => row.try_get::<_, Option<bool>>(idx).map(|v| {
                v.map(|b| b.to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::INT2 => row.try_get::<_, Option<i16>>(idx).map(|v| {
                v.map(|n| n.to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::INT4 | Type::OID => row.try_get::<_, Option<i32>>(idx).map(|v| {
                v.map(|n| n.to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::INT8 => row.try_get::<_, Option<i64>>(idx).map(|v| {
                v.map(|n| n.to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::FLOAT4 => row.try_get::<_, Option<f32>>(idx).map(|v| {
                v.map(|n| n.to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::FLOAT8 => row.try_get::<_, Option<f64>>(idx).map(|v| {
                v.map(|n| n.to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::NUMERIC => row
                .try_get::<_, Option<rust_decimal::Decimal>>(idx)
                .map(|v| {
                    v.map(|n| n.to_string())
                        .unwrap_or_else(|| "NULL".to_string())
                }),
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => row
                .try_get::<_, Option<String>>(idx)
                .map(|v| v.unwrap_or_else(|| "NULL".to_string())),
            Type::JSON | Type::JSONB => row.try_get::<_, Option<JsonValue>>(idx).map(|v| {
                v.map(|j| j.to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::TIMESTAMP => row.try_get::<_, Option<NaiveDateTime>>(idx).map(|v| {
                v.map(|t| t.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::TIMESTAMPTZ => row.try_get::<_, Option<DateTime<Utc>>>(idx).map(|v| {
                v.map(|t| t.format("%Y-%m-%d %H:%M:%S%.3f %Z").to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::DATE => row.try_get::<_, Option<NaiveDate>>(idx).map(|v| {
                v.map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::TIME => row.try_get::<_, Option<NaiveTime>>(idx).map(|v| {
                v.map(|t| t.format("%H:%M:%S%.3f").to_string())
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::BYTEA => row.try_get::<_, Option<Vec<u8>>>(idx).map(|v| {
                v.map(|bytes| {
                    if bytes.len() <= 32 {
                        format!("\\x{}", hex::encode(&bytes))
                    } else {
                        format!("<bytea: {} bytes>", bytes.len())
                    }
                })
                .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::TEXT_ARRAY => row.try_get::<_, Option<Vec<String>>>(idx).map(|v| {
                v.map(|arr| format!("{{{}}}", arr.join(",")))
                    .unwrap_or_else(|| "NULL".to_string())
            }),
            Type::INT4_ARRAY => row.try_get::<_, Option<Vec<i32>>>(idx).map(|v| {
                v.map(|arr| {
                    format!(
                        "{{{}}}",
                        arr.iter()
                            .map(|n| n.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                })
                .unwrap_or_else(|| "NULL".to_string())
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
