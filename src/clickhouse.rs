use crate::db::QueryResult;
use crate::error::{Result, SqliError};
use crate::format as fmt;
use chrono::{DateTime, NaiveDate};
use chrono_tz::Tz;
use clickhouse_rs::types::{Block, Complex, Decimal};
use clickhouse_rs::{ClientHandle, Pool};
use reqwest::Client;
use serde::Deserialize;

/// ClickHouse client supporting both HTTP and native protocols.
pub enum ClickHouseClient {
    Http(HttpClient),
    Native(NativeClient),
}

/// HTTP API client using reqwest
pub struct HttpClient {
    client: Client,
    base_url: String,
    user: String,
    password: String,
    database: String,
}

/// Native protocol client using clickhouse-rs
pub struct NativeClient {
    pool: Pool,
    database: String,
}

#[derive(Debug, Deserialize)]
struct JsonResponse {
    meta: Vec<ColumnMeta>,
    data: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ColumnMeta {
    name: String,
}

impl ClickHouseClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
        database: &str,
        tls: bool,
        use_http: bool,
    ) -> Result<Self> {
        if use_http {
            let client = HttpClient::connect(host, port, user, password, database, tls).await?;
            Ok(ClickHouseClient::Http(client))
        } else {
            let client = NativeClient::connect(host, port, user, password, database, tls).await?;
            Ok(ClickHouseClient::Native(client))
        }
    }

    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>> {
        match self {
            ClickHouseClient::Http(c) => c.list_databases(include_system).await,
            ClickHouseClient::Native(c) => c.list_databases(include_system).await,
        }
    }

    pub async fn list_tables(&self, database: &str) -> Result<Vec<String>> {
        match self {
            ClickHouseClient::Http(c) => c.list_tables(database).await,
            ClickHouseClient::Native(c) => c.list_tables(database).await,
        }
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        match self {
            ClickHouseClient::Http(c) => c.execute_query(query).await,
            ClickHouseClient::Native(c) => c.execute_query(query).await,
        }
    }

    pub fn select_table_query(&self, table: &str, limit: usize, _database: Option<&str>) -> String {
        format!("SELECT * FROM {} LIMIT {}", table, limit)
    }

    pub fn describe_table_query(&self, table: &str, database: Option<&str>) -> String {
        match database {
            Some(db) => format!("DESCRIBE TABLE {}.{}", db, table),
            None => format!("DESCRIBE TABLE {}", table),
        }
    }

    pub async fn list_columns(&self, table: &str, database: Option<&str>) -> Result<Vec<String>> {
        match self {
            ClickHouseClient::Http(c) => c.list_columns(table, database).await,
            ClickHouseClient::Native(c) => c.list_columns(table, database).await,
        }
    }
}

// ============================================================================
// HTTP Client Implementation
// ============================================================================

impl HttpClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
        database: &str,
        tls: bool,
    ) -> Result<Self> {
        let client = Client::new();
        let scheme = if tls { "https" } else { "http" };
        let base_url = format!("{}://{}:{}", scheme, host, port);

        let ch_client = Self {
            client,
            base_url,
            user: user.to_string(),
            password: password.to_string(),
            database: database.to_string(),
        };

        // Test connection
        ch_client.execute_raw("SELECT 1").await?;

        Ok(ch_client)
    }

    async fn execute_raw(&self, query: &str) -> Result<String> {
        let mut request = self
            .client
            .post(&self.base_url)
            .query(&[("database", &self.database)])
            .body(query.to_string());

        if !self.user.is_empty() {
            request = request.header("X-ClickHouse-User", &self.user);
        }
        if !self.password.is_empty() {
            request = request.header("X-ClickHouse-Key", &self.password);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(SqliError::Query(error_text));
        }

        Ok(response.text().await?)
    }

    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>> {
        const SYSTEM_DATABASES: &[&str] = &["system", "INFORMATION_SCHEMA", "information_schema"];

        let query = "SELECT name FROM system.databases ORDER BY name FORMAT JSONEachRow";
        let response = self.execute_raw(query).await?;

        let databases: Vec<String> = response
            .lines()
            .filter(|line| !line.is_empty())
            .filter_map(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .and_then(|v| v.get("name")?.as_str().map(String::from))
            })
            .filter(|db| include_system || !SYSTEM_DATABASES.contains(&db.as_str()))
            .collect();

        Ok(databases)
    }

    pub async fn list_tables(&self, database: &str) -> Result<Vec<String>> {
        let query = format!(
            "SELECT name FROM system.tables WHERE database = '{}' ORDER BY name FORMAT JSONEachRow",
            database.replace('\'', "''")
        );
        let response = self.execute_raw(&query).await?;

        let tables: Vec<String> = response
            .lines()
            .filter(|line| !line.is_empty())
            .filter_map(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .and_then(|v| v.get("name")?.as_str().map(String::from))
            })
            .collect();

        Ok(tables)
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        let query_upper = query.trim().to_uppercase();

        if query_upper.starts_with("SELECT")
            || query_upper.starts_with("SHOW")
            || query_upper.starts_with("DESCRIBE")
            || query_upper.starts_with("EXPLAIN")
            || query_upper.starts_with("WITH")
        {
            let query_with_format = format!("{} format JSON", query.trim().trim_end_matches(';'));

            let response = self.execute_raw(&query_with_format).await?;

            let json_response: JsonResponse = serde_json::from_str(&response)?;

            let columns: Vec<String> = json_response.meta.into_iter().map(|m| m.name).collect();

            let rows: Vec<Vec<String>> = json_response
                .data
                .into_iter()
                .map(|row| {
                    columns
                        .iter()
                        .map(|col| {
                            row.get(col)
                                .map(Self::format_value)
                                .unwrap_or_else(|| "NULL".to_string())
                        })
                        .collect()
                })
                .collect();

            Ok(QueryResult::Select { columns, rows })
        } else {
            self.execute_raw(query).await?;
            Ok(QueryResult::Execute { rows_affected: 0 })
        }
    }

    pub async fn list_columns(&self, table: &str, database: Option<&str>) -> Result<Vec<String>> {
        let query = match database {
            Some(db) => format!(
                "SELECT name FROM system.columns WHERE database = '{}' AND table = '{}'",
                db, table
            ),
            None => format!("SELECT name FROM system.columns WHERE table = '{}'", table),
        };
        let result = self.execute_query(&query).await?;
        match result {
            QueryResult::Select { rows, .. } => Ok(rows
                .into_iter()
                .filter_map(|r| r.into_iter().next())
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    fn format_value(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "NULL".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => value.to_string(),
        }
    }
}

// ============================================================================
// Native Protocol Client Implementation
// ============================================================================

impl NativeClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
        database: &str,
        _tls: bool, // TLS not yet supported in clickhouse-rs
    ) -> Result<Self> {
        // Build connection URL for native protocol
        // Format: tcp://user:password@host:port/database
        let url = if password.is_empty() {
            format!("tcp://{}@{}:{}/{}", user, host, port, database)
        } else {
            format!("tcp://{}:{}@{}:{}/{}", user, password, host, port, database)
        };

        let pool = Pool::new(url);

        // Test connection
        let mut client = pool.get_handle().await?;
        client.query("SELECT 1").fetch_all().await?;

        Ok(Self {
            pool,
            database: database.to_string(),
        })
    }

    async fn get_client(&self) -> Result<ClientHandle> {
        Ok(self.pool.get_handle().await?)
    }

    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>> {
        const SYSTEM_DATABASES: &[&str] = &["system", "INFORMATION_SCHEMA", "information_schema"];

        let mut client = self.get_client().await?;
        let block = client
            .query("SELECT name FROM system.databases ORDER BY name")
            .fetch_all()
            .await?;

        let databases = Self::extract_string_column(&block, "name")?
            .into_iter()
            .filter(|db| include_system || !SYSTEM_DATABASES.contains(&db.as_str()))
            .collect();

        Ok(databases)
    }

    pub async fn list_tables(&self, database: &str) -> Result<Vec<String>> {
        let mut client = self.get_client().await?;
        let query = format!(
            "SELECT name FROM system.tables WHERE database = '{}' ORDER BY name",
            database.replace('\'', "''")
        );
        let block = client.query(&query).fetch_all().await?;

        Self::extract_string_column(&block, "name")
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        let mut client = self.get_client().await?;
        let query_upper = query.trim().to_uppercase();

        if query_upper.starts_with("SELECT")
            || query_upper.starts_with("SHOW")
            || query_upper.starts_with("DESCRIBE")
            || query_upper.starts_with("EXPLAIN")
            || query_upper.starts_with("WITH")
        {
            let block = client
                .query(query.trim().trim_end_matches(';'))
                .fetch_all()
                .await?;

            let (columns, rows) = Self::block_to_result(&block)?;
            Ok(QueryResult::Select { columns, rows })
        } else {
            client.execute(query).await?;
            Ok(QueryResult::Execute { rows_affected: 0 })
        }
    }

    pub async fn list_columns(&self, table: &str, database: Option<&str>) -> Result<Vec<String>> {
        let mut client = self.get_client().await?;
        let query = match database {
            Some(db) => format!(
                "SELECT name FROM system.columns WHERE database = '{}' AND table = '{}'",
                db, table
            ),
            None => format!(
                "SELECT name FROM system.columns WHERE database = '{}' AND table = '{}'",
                self.database, table
            ),
        };
        let block = client.query(&query).fetch_all().await?;

        Self::extract_string_column(&block, "name")
    }

    /// Extract a single string column from a block
    fn extract_string_column(block: &Block<Complex>, column_name: &str) -> Result<Vec<String>> {
        let mut values = Vec::new();
        let row_count = block.row_count();

        for i in 0..row_count {
            let value: String = block.get(i, column_name)?;
            values.push(value);
        }

        Ok(values)
    }

    /// Convert a Block to columns and rows for QueryResult
    fn block_to_result(block: &Block<Complex>) -> Result<(Vec<String>, Vec<Vec<String>>)> {
        let columns: Vec<String> = block
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect();

        let row_count = block.row_count();
        let mut rows = Vec::with_capacity(row_count);

        for row_idx in 0..row_count {
            let mut row = Vec::with_capacity(columns.len());
            for col in block.columns() {
                let value = Self::get_column_value(block, row_idx, col.name(), col.sql_type());
                row.push(value);
            }
            rows.push(row);
        }

        Ok((columns, rows))
    }

    /// Get a value from a column, handling different ClickHouse types
    fn get_column_value(
        block: &Block<Complex>,
        row: usize,
        column: &str,
        sql_type: clickhouse_rs::types::SqlType,
    ) -> String {
        use clickhouse_rs::types::SqlType;

        // Helper macro to try getting a value and converting to string
        macro_rules! try_get {
            ($t:ty) => {
                if let Ok(v) = block.get::<$t, _>(row, column) {
                    return v.to_string();
                }
            };
            ($t:ty, $fmt:expr) => {
                if let Ok(v) = block.get::<$t, _>(row, column) {
                    return $fmt(v);
                }
            };
        }

        macro_rules! try_get_opt {
            ($t:ty) => {
                if let Ok(v) = block.get::<Option<$t>, _>(row, column) {
                    return fmt::null_or(v);
                }
            };
            ($t:ty, $fmt:expr) => {
                if let Ok(v) = block.get::<Option<$t>, _>(row, column) {
                    return fmt::null_or_else(v, $fmt);
                }
            };
        }

        // Formatters using shared format module
        let fmt_decimal = |d: Decimal| fmt::decimal(d.internal(), d.scale() as u32);
        let fmt_datetime = |dt: DateTime<Tz>| dt.format("%Y-%m-%d %H:%M:%S").to_string();
        let fmt_date = |d: NaiveDate| d.format("%Y-%m-%d").to_string();

        // Match on SQL type and try to extract appropriate type
        match sql_type {
            SqlType::UInt8 => try_get!(u8),
            SqlType::UInt16 => try_get!(u16),
            SqlType::UInt32 => try_get!(u32),
            SqlType::UInt64 => try_get!(u64),
            SqlType::Int8 => try_get!(i8),
            SqlType::Int16 => try_get!(i16),
            SqlType::Int32 => try_get!(i32),
            SqlType::Int64 => try_get!(i64),
            SqlType::Float32 => try_get!(f32),
            SqlType::Float64 => try_get!(f64),
            SqlType::String | SqlType::FixedString(_) => try_get!(String),
            SqlType::Date => try_get!(NaiveDate, fmt_date),
            SqlType::DateTime(_) => try_get!(DateTime<Tz>, fmt_datetime),
            SqlType::Decimal(_, _) => try_get!(Decimal, fmt_decimal),
            SqlType::Uuid | SqlType::Ipv4 | SqlType::Ipv6 => try_get!(String),
            SqlType::Nullable(inner) => match *inner {
                SqlType::UInt8 => try_get_opt!(u8),
                SqlType::UInt16 => try_get_opt!(u16),
                SqlType::UInt32 => try_get_opt!(u32),
                SqlType::UInt64 => try_get_opt!(u64),
                SqlType::Int8 => try_get_opt!(i8),
                SqlType::Int16 => try_get_opt!(i16),
                SqlType::Int32 => try_get_opt!(i32),
                SqlType::Int64 => try_get_opt!(i64),
                SqlType::Float32 => try_get_opt!(f32),
                SqlType::Float64 => try_get_opt!(f64),
                SqlType::String | SqlType::FixedString(_) => try_get_opt!(String),
                SqlType::Date => try_get_opt!(NaiveDate, fmt_date),
                SqlType::DateTime(_) => try_get_opt!(DateTime<Tz>, fmt_datetime),
                SqlType::Decimal(_, _) => try_get_opt!(Decimal, fmt_decimal),
                SqlType::Uuid | SqlType::Ipv4 | SqlType::Ipv6 => try_get_opt!(String),
                _ => {}
            },
            _ => {}
        }

        // Fallback: try common types in order of likelihood
        try_get!(String);
        try_get!(DateTime<Tz>, fmt_datetime);
        try_get!(NaiveDate, fmt_date);
        try_get!(Decimal, fmt_decimal);
        try_get!(i64);
        try_get!(u64);
        try_get!(f64);

        // Ultimate fallback
        "<unsupported>".to_string()
    }
}
