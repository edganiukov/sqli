use crate::db::QueryResult;
use crate::error::{Result, SqliError};
use crate::format as fmt;
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Pool, Value};

pub struct MySqlClient {
    pool: Pool,
}

impl MySqlClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
        database: &str,
    ) -> Result<Self> {
        let opts = OptsBuilder::default()
            .ip_or_hostname(host)
            .tcp_port(port)
            .user(Some(user))
            .pass(Some(password))
            .db_name(if database.is_empty() {
                None
            } else {
                Some(database)
            });

        let pool = Pool::new(Opts::from(opts));

        // Test connection
        let mut conn = pool.get_conn().await?;
        conn.query_drop("SELECT 1").await?;

        Ok(Self { pool })
    }

    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>> {
        const SYSTEM_DATABASES: &[&str] =
            &["information_schema", "mysql", "performance_schema", "sys"];

        let mut conn = self.pool.get_conn().await?;
        let databases: Vec<String> = conn.query("SHOW DATABASES").await?;

        let filtered: Vec<String> = databases
            .into_iter()
            .filter(|db| include_system || !SYSTEM_DATABASES.contains(&db.as_str()))
            .collect();

        Ok(filtered)
    }

    pub async fn list_tables(&self, database: &str) -> Result<Vec<String>> {
        let mut conn = self.pool.get_conn().await?;
        let query = format!(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = '{}' ORDER BY table_name",
            database.replace('\'', "''")
        );
        let tables: Vec<String> = conn.query(query).await?;
        Ok(tables)
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        let mut conn = self.pool.get_conn().await?;
        let query_upper = query.trim().to_uppercase();

        if query_upper.starts_with("SELECT")
            || query_upper.starts_with("SHOW")
            || query_upper.starts_with("DESCRIBE")
            || query_upper.starts_with("EXPLAIN")
            || query_upper.starts_with("WITH")
        {
            let mut result = conn.query_iter(query).await?;

            let columns: Vec<String> = result
                .columns_ref()
                .iter()
                .map(|c| c.name_str().to_string())
                .collect();

            let rows: Vec<mysql_async::Row> = result.collect().await?;

            let data_rows: Vec<Vec<String>> = rows
                .iter()
                .map(|row| {
                    (0..columns.len())
                        .map(|i| Self::format_value(row.get::<Value, _>(i)))
                        .collect()
                })
                .collect();

            Ok(QueryResult::Select {
                columns,
                rows: data_rows,
            })
        } else {
            let result = conn.exec_drop(query, ()).await;
            match result {
                Ok(_) => Ok(QueryResult::Execute {
                    rows_affected: conn.affected_rows(),
                }),
                Err(e) => Err(SqliError::Query(e.to_string())),
            }
        }
    }

    pub fn select_table_query(&self, table: &str, limit: usize, _database: Option<&str>) -> String {
        format!("SELECT * FROM {} LIMIT {}", table, limit)
    }

    pub fn describe_table_query(&self, table: &str, _database: Option<&str>) -> String {
        format!("DESCRIBE {}", table)
    }

    pub async fn list_columns(&self, table: &str) -> Result<Vec<String>> {
        let query = format!("SHOW COLUMNS FROM {}", table);
        let rows: Vec<mysql_async::Row> = self.pool.get_conn().await?.query(query).await?;
        Ok(rows.iter().filter_map(|r| r.get::<String, _>(0)).collect())
    }

    fn format_value(value: Option<Value>) -> String {
        match value {
            None | Some(Value::NULL) => "NULL".to_string(),
            Some(Value::Bytes(b)) => String::from_utf8_lossy(&b).to_string(),
            Some(Value::Int(i)) => i.to_string(),
            Some(Value::UInt(u)) => u.to_string(),
            Some(Value::Float(f)) => f.to_string(),
            Some(Value::Double(d)) => d.to_string(),
            Some(Value::Date(y, m, d, h, min, s, _us)) => {
                if h == 0 && min == 0 && s == 0 {
                    fmt::date(y as i32, m as u32, d as u32)
                } else {
                    fmt::datetime(y as i32, m as u32, d as u32, h as u32, min as u32, s as u32)
                }
            }
            Some(Value::Time(neg, d, h, m, s, us)) => {
                let sign = if neg { "-" } else { "" };
                let total_hours = d * 24 + h as u32;
                if us == 0 {
                    format!("{}{}:{:02}:{:02}", sign, total_hours, m, s)
                } else {
                    format!("{}{}:{:02}:{:02}.{:06}", sign, total_hours, m, s, us)
                }
            }
        }
    }
}
