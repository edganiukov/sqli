use crate::db::QueryResult;
use reqwest::Client;
use serde::Deserialize;
use std::error::Error;

pub struct ClickHouseClient {
    client: Client,
    base_url: String,
    user: String,
    password: String,
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
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
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

    async fn execute_raw(&self, query: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
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
            return Err(error_text.into());
        }

        Ok(response.text().await?)
    }

    pub async fn list_databases(
        &self,
        include_system: bool,
    ) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
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

    pub async fn list_tables(
        &self,
        database: &str,
    ) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
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

    pub async fn execute_query(
        &self,
        query: &str,
    ) -> Result<QueryResult, Box<dyn Error + Send + Sync>> {
        let query_upper = query.trim().to_uppercase();

        if query_upper.starts_with("SELECT")
            || query_upper.starts_with("SHOW")
            || query_upper.starts_with("DESCRIBE")
            || query_upper.starts_with("EXPLAIN")
        {
            // Add FORMAT JSON to get structured results
            let query_with_format = format!(
                "{} FORMAT JSON",
                query.trim().trim_end_matches(';')
            );

            let response = self.execute_raw(&query_with_format).await?;

            // Parse JSON response
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
                                .map(|v| Self::format_value(v))
                                .unwrap_or_else(|| "NULL".to_string())
                        })
                        .collect()
                })
                .collect();

            Ok(QueryResult::Select { columns, rows })
        } else {
            // For non-SELECT queries
            self.execute_raw(query).await?;
            Ok(QueryResult::Execute { rows_affected: 0 })
        }
    }

    fn format_value(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "NULL".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) => format!("[{} items]", arr.len()),
            serde_json::Value::Object(obj) => format!("{{{} fields}}", obj.len()),
        }
    }

    pub fn select_table_query(table: &str, limit: usize) -> String {
        format!("SELECT * FROM {} LIMIT {}", table, limit)
    }

    pub fn describe_table_query(table: &str, database: Option<&str>) -> String {
        match database {
            Some(db) => format!("DESCRIBE TABLE {}.{}", db, table),
            None => format!("DESCRIBE TABLE {}", table),
        }
    }

    pub fn default_database() -> &'static str {
        "default"
    }
}
