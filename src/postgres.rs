use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use postgres_native_tls::MakeTlsConnector;
use serde_json::Value as JsonValue;
use tokio_postgres::config::SslMode;
use tokio_postgres::types::{FromSql, Kind, Type};
use tokio_postgres::{Client, Config, NoTls, Row};
use uuid::Uuid;

use crate::db::{QueryResult, Tls};
use crate::error::{Result, SqliError};
use crate::format as fmt;
use crate::sql;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Schema assumed for a table name that carries no schema of its own.
const DEFAULT_SCHEMA: &str = "public";

pub struct PostgresClient {
    client: Client,
    /// The TLS setup of this connection, needed again to cancel a query
    /// (a cancel request travels over a connection of its own).
    tls: Option<MakeTlsConnector>,
}

/// Run a connection's I/O in the background until it ends.
fn drive<F>(connection: F)
where
    F: Future<Output = std::result::Result<(), tokio_postgres::Error>> + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            crate::debug_log!("Postgres connection error: {}", e);
        }
    });
}

/// Name a table is listed under: bare for the default schema, `schema.table`
/// otherwise. A default-schema table with a dot in its own name is qualified
/// too, so the first dot always separates schema from table.
fn display_name(schema: &str, table: &str) -> String {
    if schema == DEFAULT_SCHEMA && !table.contains('.') {
        table.to_string()
    } else {
        format!("{}.{}", schema, table)
    }
}

/// Inverse of [`display_name`]: `(schema, table)`.
fn split_name(name: &str) -> (&str, &str) {
    name.split_once('.').unwrap_or((DEFAULT_SCHEMA, name))
}

impl PostgresClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
        database: &str,
        readonly: bool,
        tls: Tls,
    ) -> Result<Self> {
        crate::debug_log!(
            "Postgres connecting to {}:{} database='{}' user='{}'",
            host,
            port,
            database,
            user
        );
        // Built field by field rather than as a `key=value` string: that format
        // breaks on empty values and on spaces/quotes in passwords.
        let mut config = Config::new();
        config
            .host(host)
            .port(port)
            .user(user)
            .connect_timeout(CONNECT_TIMEOUT)
            // TCP keepalive lets the OS notice a dead peer (sleep/wake, NAT
            // expiry) so the connection closes instead of hanging silently.
            .keepalives(true)
            .keepalives_idle(Duration::from_secs(30))
            .keepalives_interval(Duration::from_secs(10))
            .keepalives_retries(3);
        if !password.is_empty() {
            config.password(password);
        }
        if !database.is_empty() {
            config.dbname(database);
        }

        let tls = match tls {
            Tls::Off => None,
            mode => {
                let unverified = mode == Tls::Unverified;
                let connector = native_tls::TlsConnector::builder()
                    .danger_accept_invalid_certs(unverified)
                    .danger_accept_invalid_hostnames(unverified)
                    .build()
                    .map_err(|e| SqliError::Connection(format!("TLS setup failed: {}", e)))?;
                // Never fall back to plain text when TLS was asked for.
                config.ssl_mode(SslMode::Require);
                Some(MakeTlsConnector::new(connector))
            }
        };

        let client = match &tls {
            Some(tls) => {
                let (client, connection) = config.connect(tls.clone()).await?;
                drive(connection);
                client
            }
            None => {
                let (client, connection) = config.connect(NoTls).await?;
                drive(connection);
                client
            }
        };

        // Enforce read-only at the session level so writes are rejected by the
        // server, not just by the client-side statement heuristic.
        if readonly {
            client
                .batch_execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY")
                .await?;
        }

        Ok(Self { client, tls })
    }

    /// Round-trip a protocol-level `Sync`; fails if the connection is gone.
    pub async fn ping(&self) -> Result<()> {
        self.client.check_connection().await?;
        Ok(())
    }

    /// True once the connection task has ended (server closed it, I/O error).
    pub fn is_closed(&self) -> bool {
        self.client.is_closed()
    }

    /// Ask the server to abort the statement currently running on this
    /// connection. Dropping the client-side future alone leaves it running.
    pub async fn cancel_running(&self) -> Result<()> {
        let token = self.client.cancel_token();
        match &self.tls {
            Some(tls) => token.cancel_query(tls.clone()).await?,
            None => token.cancel_query(NoTls).await?,
        }
        Ok(())
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

    /// Tables of every user schema, the default schema first. Names outside
    /// the default schema are listed as `schema.table`.
    pub async fn list_tables(&self, _schema: &str) -> Result<Vec<String>> {
        let rows = self
            .client
            .query(
                "SELECT schemaname, tablename FROM pg_tables \
                 WHERE schemaname NOT IN ('pg_catalog', 'information_schema') \
                   AND schemaname NOT LIKE 'pg\\_toast%' \
                   AND schemaname NOT LIKE 'pg\\_temp%' \
                 ORDER BY schemaname <> $1, schemaname, tablename",
                &[&DEFAULT_SCHEMA],
            )
            .await?;

        let tables: Vec<String> = rows
            .iter()
            .map(|row| display_name(row.get(0), row.get(1)))
            .collect();
        crate::debug_log!("Found {} tables: {:?}", tables.len(), tables);
        Ok(tables)
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        crate::debug_log!("Postgres executing: {}", query.trim().replace('\n', " "));

        // Preparing the statement tells us what it returns. Guessing from the
        // first keyword misses SHOW, EXPLAIN, VALUES and INSERT ... RETURNING,
        // and reading column names off the first row loses the header of an
        // empty result.
        let statement = self.client.prepare(query).await?;
        if statement.columns().is_empty() {
            let rows_affected = self.client.execute(&statement, &[]).await?;
            return Ok(QueryResult::Execute { rows_affected });
        }

        let columns: Vec<String> = statement
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect();

        let rows = self.client.query(&statement, &[]).await?;
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
    }

    pub fn select_table_query(&self, table: &str, limit: usize, _schema: Option<&str>) -> String {
        let (schema, table) = split_name(table);
        format!(
            "SELECT * FROM {}.{} LIMIT {}",
            sql::ident(schema, '"'),
            sql::ident(table, '"'),
            limit
        )
    }

    pub fn describe_table_query(&self, table: &str, _schema: Option<&str>) -> String {
        let (schema, table) = split_name(table);
        format!(
            "SELECT column_name, data_type, is_nullable, column_default \n\
             FROM information_schema.columns \n\
             WHERE table_schema = {} AND table_name = {} \n\
             ORDER BY ordinal_position",
            sql::literal(schema),
            sql::literal(table)
        )
    }

    pub async fn list_columns(&self, table: &str) -> Result<Vec<String>> {
        let (schema, table) = split_name(table);
        let rows = self
            .client
            .query(
                "SELECT column_name::text FROM information_schema.columns \
                 WHERE table_schema = $1::text AND table_name = $2::text \
                 ORDER BY ordinal_position",
                &[&schema, &table],
            )
            .await?;
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
            Type::BYTEA_ARRAY => row
                .try_get::<_, Option<Vec<Vec<u8>>>>(idx)
                .map(|v| fmt::null_or_else(v, |arr| fmt::collection("bytea[]", arr.len()))),

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
            Kind::Domain(_) | Kind::Enum(_) | Kind::Composite(_) | Kind::Range(_) => {
                row.try_get::<_, Option<RawText>>(idx).map(fmt::null_or)
            }

            // Any other unknown type
            _ => row.try_get::<_, Option<RawText>>(idx).map(fmt::null_or),
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
    fn from_sql(
        _ty: &Type,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        match std::str::from_utf8(raw) {
            Ok(s) => Ok(RawText(s.to_string())),
            Err(_) => Ok(RawText(fmt::bytes(raw, 32))),
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_names_round_trip_through_their_display_form() {
        for (schema, table) in [
            ("public", "users"),
            ("analytics", "events"),
            ("public", "odd.name"),
            ("analytics", "odd.name"),
        ] {
            assert_eq!(split_name(&display_name(schema, table)), (schema, table));
        }
        assert_eq!(display_name("public", "users"), "users");
        assert_eq!(display_name("analytics", "events"), "analytics.events");
    }
}
