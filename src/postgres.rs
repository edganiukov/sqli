use crate::db::QueryResult;
use crate::error::Result;
use crate::format as fmt;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde_json::Value as JsonValue;
use tokio_postgres::types::{FromSql, Kind, Type};
use tokio_postgres::{Client, NoTls, Row};
use uuid::Uuid;

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

        // Helper macro for scalar types: get Option<T>, format with null_or
        macro_rules! get_value {
            ($t:ty) => {
                row.try_get::<_, Option<$t>>(idx).map(fmt::null_or)
            };
            ($t:ty, $f:expr) => {
                row.try_get::<_, Option<$t>>(idx)
                    .map(|v| fmt::null_or_else(v, $f))
            };
        }

        // Helper macro for array types: get Option<Vec<T>>, format as {a,b,c}
        macro_rules! get_array {
            ($t:ty) => {
                row.try_get::<_, Option<Vec<$t>>>(idx)
                    .map(|v| fmt::null_or_else(v, |arr| fmt::pg_array(&arr)))
            };
        }

        let result: std::result::Result<String, _> = match *col_type {
            // Scalar types
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
            Type::UUID => get_value!(Uuid),
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

            // Array types
            Type::BOOL_ARRAY => get_array!(bool),
            Type::INT2_ARRAY => get_array!(i16),
            Type::INT4_ARRAY => get_array!(i32),
            Type::INT8_ARRAY => get_array!(i64),
            Type::FLOAT4_ARRAY => get_array!(f32),
            Type::FLOAT8_ARRAY => get_array!(f64),
            Type::NUMERIC_ARRAY => get_array!(rust_decimal::Decimal),
            Type::TEXT_ARRAY | Type::VARCHAR_ARRAY | Type::BPCHAR_ARRAY | Type::NAME_ARRAY => {
                get_array!(String)
            }
            Type::UUID_ARRAY => get_array!(Uuid),
            Type::JSON_ARRAY | Type::JSONB_ARRAY => get_array!(JsonValue),
            Type::TIMESTAMP_ARRAY => get_array!(NaiveDateTime),
            Type::TIMESTAMPTZ_ARRAY => get_array!(DateTime<Utc>),
            Type::DATE_ARRAY => get_array!(NaiveDate),
            Type::BYTEA_ARRAY => {
                row.try_get::<_, Option<Vec<Vec<u8>>>>(idx)
                    .map(|v| fmt::null_or_else(v, |arr| fmt::collection("bytea[]", arr.len())))
            }

            // Custom and unknown types — use Kind metadata + RawText fallback
            _ => Self::get_custom_value(row, idx, col_type),
        };

        result.unwrap_or_else(|_| format!("<{}>", col_type.name()))
    }

    /// Handle custom types (enums, domains, arrays of custom types, etc.)
    /// by inspecting the type's Kind metadata and falling back to raw bytes.
    fn get_custom_value(
        row: &Row,
        idx: usize,
        col_type: &Type,
    ) -> std::result::Result<String, tokio_postgres::Error> {
        match col_type.kind() {
            // Arrays of custom types (e.g. enum[], domain[])
            Kind::Array(_) => row
                .try_get::<_, Option<Vec<RawText>>>(idx)
                .map(|v| fmt::null_or_else(v, |arr| fmt::pg_array(&arr))),

            // Domain types wrap an underlying type — try RawText which reads
            // the binary representation (same encoding as the base type)
            Kind::Domain(_) | Kind::Enum(_) | Kind::Composite(_) | Kind::Range(_) => row
                .try_get::<_, Option<RawText>>(idx)
                .map(fmt::null_or),

            // Any other unknown type
            _ => row
                .try_get::<_, Option<RawText>>(idx)
                .map(fmt::null_or),
        }
    }
}

/// A wrapper that reads any PostgreSQL value as raw bytes and interprets it as text.
///
/// PostgreSQL enums, domains over text types, and many extension types transmit
/// their values as UTF-8 strings even in the binary protocol. For types with true
/// binary representations (e.g. numeric domains, geometric types), this falls back
/// to hex display.
struct RawText(String);

impl std::fmt::Display for RawText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'a> FromSql<'a> for RawText {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        match std::str::from_utf8(raw) {
            Ok(s) => Ok(RawText(s.to_string())),
            Err(_) => Ok(RawText(fmt::bytes(raw, 32))),
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}
