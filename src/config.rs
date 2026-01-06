use crate::controller::{DatabaseConn, DatabaseType};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Deserialize)]
pub struct ConnectionConfig {
    #[serde(rename = "type")]
    pub db_type: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub password_cmd: Option<String>,
    #[serde(default)]
    pub tls: bool,
}

fn default_port() -> u16 {
    5432
}

impl ConnectionConfig {
    pub fn resolve_password(&self) -> String {
        if let Some(ref cmd) = self.password_cmd {
            match run_password_command(cmd) {
                Ok(pwd) => return pwd,
                Err(e) => eprintln!("Failed to run password_cmd: {}", e),
            }
        }
        self.password.clone().unwrap_or_default()
    }

    pub fn to_database_conn(&self, name: &str) -> Option<DatabaseConn> {
        let db_type = match self.db_type.to_lowercase().as_str() {
            "postgres" | "postgresql" => DatabaseType::Postgres,
            "mysql" | "mariadb" => DatabaseType::MySql,
            "cassandra" | "scylla" => DatabaseType::Cassandra,
            "clickhouse" | "ch" => DatabaseType::ClickHouse,
            _ => return None,
        };

        Some(DatabaseConn {
            name: name.to_string(),
            db_type,
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            password: self.resolve_password(),
            tls: self.tls,
        })
    }
}

fn run_password_command(cmd: &str) -> io::Result<String> {
    let output = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", cmd]).output()?
    } else {
        Command::new("sh").args(["-c", cmd]).output()?
    };

    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Command failed with status: {}", output.status),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn load_config(custom_path: Option<PathBuf>) -> Vec<DatabaseConn> {
    let config_path = custom_path.or_else(get_config_path);

    match config_path {
        Some(path) => match fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<HashMap<String, ConnectionConfig>>(&content) {
                Ok(configs) => {
                    let mut connections: Vec<DatabaseConn> = configs
                        .iter()
                        .filter_map(|(name, config)| config.to_database_conn(name))
                        .collect();
                    connections.sort_by(|a, b| a.name.cmp(&b.name));
                    if connections.is_empty() {
                        default_connections()
                    } else {
                        connections
                    }
                }
                Err(e) => {
                    eprintln!("Failed to parse config file: {}", e);
                    default_connections()
                }
            },
            Err(e) => {
                eprintln!("Failed to read config file: {}", e);
                default_connections()
            }
        },
        None => default_connections(),
    }
}

fn get_config_path() -> Option<PathBuf> {
    // Priority 1: current directory
    let local_config = PathBuf::from("sqli.toml");
    if local_config.exists() {
        return Some(local_config);
    }

    // Priority 2: user config directory (~/.config/sqli/config.toml)
    if let Some(config_dir) = dirs::config_dir() {
        let user_config = config_dir.join("sqli").join("config.toml");
        if user_config.exists() {
            return Some(user_config);
        }
    }

    None
}

fn default_connections() -> Vec<DatabaseConn> {
    vec![DatabaseConn {
        name: "localhost".to_string(),
        db_type: DatabaseType::Postgres,
        host: "localhost".to_string(),
        port: 5432,
        user: "postgres".to_string(),
        password: String::new(),
        tls: false,
    }]
}
