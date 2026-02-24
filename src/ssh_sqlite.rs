use crate::db::QueryResult;
use crate::error::{Result, SqliError};

use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// SQLite client that executes queries on a remote machine via SSH.
/// Uses the system `ssh` command, respecting ~/.ssh/config and SSH_AUTH_SOCK.
pub struct SshSqliteClient {
    /// SSH destination in format "user@host" or "user@host:port"
    ssh_dest: String,
    ssh_port: Option<u16>,
    db_path: String,
}

impl SshSqliteClient {
    /// Connect to a remote SQLite database via SSH.
    /// `host` should be in format "user@hostname" or "user@hostname:port"
    pub async fn connect(host: &str, db_path: &str) -> Result<Self> {
        let (ssh_dest, ssh_port) = Self::parse_ssh_host(host)?;

        let client = Self {
            ssh_dest,
            ssh_port,
            db_path: db_path.to_string(),
        };

        // Verify connectivity by checking if the database file exists
        client.verify_connection().await?;

        Ok(client)
    }

    /// Parse "user@host:port" into ("user@host", Some(port)) or ("user@host", None)
    fn parse_ssh_host(host: &str) -> Result<(String, Option<u16>)> {
        // Check for port suffix - but be careful with IPv6 addresses
        // Simple heuristic: if there's a colon after @, and the part after is numeric, it's a port
        if let Some(at_pos) = host.rfind('@') {
            let after_at = &host[at_pos + 1..];
            if let Some(colon_pos) = after_at.rfind(':') {
                let potential_port = &after_at[colon_pos + 1..];
                if let Ok(port) = potential_port.parse::<u16>() {
                    let dest = format!("{}{}", &host[..at_pos + 1], &after_at[..colon_pos]);
                    return Ok((dest, Some(port)));
                }
            }
        }
        Ok((host.to_string(), None))
    }

    async fn verify_connection(&self) -> Result<()> {
        let output = self
            .run_ssh_command(&format!(
                "test -f '{}' && echo ok || echo missing",
                self.db_path.replace("'", "'\\''")
            ))
            .await?;

        if output.trim() != "ok" {
            return Err(SqliError::Connection(format!(
                "SQLite database not found: '{}' on {}",
                self.db_path, self.ssh_dest
            )));
        }

        Ok(())
    }

    fn build_ssh_command(&self) -> Command {
        let mut cmd = Command::new("ssh");

        // BatchMode=yes prevents password prompts (relies on key auth)
        cmd.arg("-o").arg("BatchMode=yes");
        cmd.arg("-o").arg("ConnectTimeout=10");

        // Port if specified
        if let Some(port) = self.ssh_port {
            cmd.arg("-p").arg(port.to_string());
        }

        // Destination (user@host)
        cmd.arg(&self.ssh_dest);

        cmd
    }

    async fn run_ssh_command(&self, remote_cmd: &str) -> Result<String> {
        let mut cmd = self.build_ssh_command();
        cmd.arg(remote_cmd);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd
            .output()
            .await
            .map_err(|e| SqliError::Connection(format!("SSH failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SqliError::Connection(format!("SSH error: {}", stderr)));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn run_sqlite_query(&self, query: &str) -> Result<String> {
        let mut cmd = self.build_ssh_command();

        // Build sqlite3 command with JSON output mode
        let sqlite_cmd = format!("sqlite3 -json '{}'", self.db_path.replace("'", "'\\''"));
        cmd.arg(sqlite_cmd);

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| SqliError::Connection(format!("Failed to spawn SSH: {}", e)))?;

        // Write the query to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(query.as_bytes())
                .await
                .map_err(|e| SqliError::Query(format!("Failed to send query: {}", e)))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|e| SqliError::Query(format!("Failed to send query: {}", e)))?;
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| SqliError::Query(format!("Failed to get output: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Check for SQL errors
        if stdout.starts_with("Error:") || stdout.starts_with("Parse error") {
            return Err(SqliError::Query(stdout.trim().to_string()));
        }

        if !output.status.success() && !stderr.is_empty() {
            return Err(SqliError::Query(stderr));
        }

        Ok(stdout)
    }

    pub async fn list_databases(&self, _include_system: bool) -> Result<Vec<String>> {
        // SQLite is single-database — return filename as name
        let name = self
            .db_path
            .rsplit('/')
            .next()
            .unwrap_or(&self.db_path)
            .to_string();
        Ok(vec![name])
    }

    pub async fn list_tables(&self, _schema: &str) -> Result<Vec<String>> {
        let output = self
            .run_sqlite_query(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name;",
            )
            .await?;

        self.parse_single_column_json(&output)
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        let query_upper = query.trim().to_uppercase();

        if query_upper.starts_with("SELECT")
            || query_upper.starts_with("PRAGMA")
            || query_upper.starts_with("WITH")
            || query_upper.starts_with("EXPLAIN")
        {
            let output = self.run_sqlite_query(query).await?;
            self.parse_json_result(&output)
        } else {
            // For non-SELECT queries, use changes() to get affected rows
            let combined = format!(
                "{}; SELECT changes() as affected;",
                query.trim_end_matches(';')
            );
            let output = self.run_sqlite_query(&combined).await?;
            let rows_affected = self.parse_changes_result(&output)?;
            Ok(QueryResult::Execute { rows_affected })
        }
    }

    fn parse_json_result(&self, output: &str) -> Result<QueryResult> {
        let output = output.trim();

        if output.is_empty() || output == "[]" {
            return Ok(QueryResult::Select {
                columns: vec![],
                rows: vec![],
            });
        }

        let json_rows: Vec<serde_json::Map<String, serde_json::Value>> =
            serde_json::from_str(output).map_err(|e| {
                SqliError::Query(format!("Failed to parse JSON: {} (raw: {})", e, output))
            })?;

        if json_rows.is_empty() {
            return Ok(QueryResult::Select {
                columns: vec![],
                rows: vec![],
            });
        }

        // Extract column names from first row
        let columns: Vec<String> = json_rows[0].keys().cloned().collect();

        // Extract row values
        let rows: Vec<Vec<String>> = json_rows
            .iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|col| Self::format_json_value(row.get(col)))
                    .collect()
            })
            .collect();

        Ok(QueryResult::Select { columns, rows })
    }

    fn parse_single_column_json(&self, output: &str) -> Result<Vec<String>> {
        let output = output.trim();

        if output.is_empty() || output == "[]" {
            return Ok(vec![]);
        }

        let json_rows: Vec<serde_json::Map<String, serde_json::Value>> =
            serde_json::from_str(output)
                .map_err(|e| SqliError::Query(format!("Failed to parse JSON: {}", e)))?;

        Ok(json_rows
            .iter()
            .filter_map(|row| {
                row.values()
                    .next()
                    .map(|v| Self::format_json_value(Some(v)))
            })
            .collect())
    }

    fn parse_changes_result(&self, output: &str) -> Result<u64> {
        let output = output.trim();

        if output.is_empty() || output == "[]" {
            return Ok(0);
        }

        let json_rows: Vec<serde_json::Map<String, serde_json::Value>> =
            serde_json::from_str(output).unwrap_or_default();

        if let Some(row) = json_rows.first()
            && let Some(val) = row.get("affected")
        {
            if let Some(n) = val.as_u64() {
                return Ok(n);
            }
            if let Some(s) = val.as_str() {
                return s
                    .parse()
                    .map_err(|_| SqliError::Query("Invalid changes count".to_string()));
            }
        }

        Ok(0)
    }

    fn format_json_value(value: Option<&serde_json::Value>) -> String {
        match value {
            None => "NULL".to_string(),
            Some(v) => match v {
                serde_json::Value::Null => "NULL".to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(arr) => serde_json::to_string(arr).unwrap_or_default(),
                serde_json::Value::Object(obj) => serde_json::to_string(obj).unwrap_or_default(),
            },
        }
    }

    pub fn select_table_query(&self, table: &str, limit: usize, _schema: Option<&str>) -> String {
        format!("SELECT * FROM \"{}\" LIMIT {}", table, limit)
    }

    pub fn describe_table_query(&self, table: &str, _schema: Option<&str>) -> String {
        format!("PRAGMA table_info(\"{}\")", table)
    }

    pub async fn list_columns(&self, table: &str, _schema: Option<&str>) -> Result<Vec<String>> {
        let query = format!("PRAGMA table_info(\"{}\");", table);
        let output = self.run_sqlite_query(&query).await?;

        let output = output.trim();
        if output.is_empty() || output == "[]" {
            return Ok(vec![]);
        }

        let json_rows: Vec<serde_json::Map<String, serde_json::Value>> =
            serde_json::from_str(output)
                .map_err(|e| SqliError::Query(format!("Failed to parse PRAGMA: {}", e)))?;

        Ok(json_rows
            .iter()
            .filter_map(|row| row.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect())
    }
}
