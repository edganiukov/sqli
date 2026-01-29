use crate::controller::{DatabaseConn, DatabaseType};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct ConnectionConfig {
    #[serde(rename = "type")]
    pub db_type: String,
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub password_cmd: Option<String>,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub readonly: bool,
}

impl ConnectionConfig {
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
            password: self.password.clone(),
            password_cmd: self.password_cmd.clone(),
            tls: self.tls,
            readonly: self.readonly,
        })
    }
}

pub fn load_config(custom_path: Option<PathBuf>) -> Vec<DatabaseConn> {
    use crate::debug_log;

    let config_path = custom_path.clone().or_else(get_config_path);

    if let Some(ref path) = custom_path {
        debug_log!("Custom config path: {:?}", path);
    }
    debug_log!(
        "Resolved config path: {:?}",
        config_path.as_ref().map(|p| p.display().to_string())
    );

    match config_path {
        Some(path) => match fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<HashMap<String, ConnectionConfig>>(&content) {
                Ok(configs) => {
                    debug_log!("Parsed {} connection(s) from config", configs.len());
                    let mut connections: Vec<DatabaseConn> = configs
                        .iter()
                        .filter_map(|(name, config)| {
                            let conn = config.to_database_conn(name);
                            if conn.is_none() {
                                debug_log!(
                                    "Skipping connection '{}': unknown type '{}'",
                                    name,
                                    config.db_type
                                );
                            }
                            conn
                        })
                        .collect();
                    connections.sort_by(|a, b| a.name.cmp(&b.name));
                    if connections.is_empty() {
                        debug_log!("No valid connections found, using defaults");
                        default_connections()
                    } else {
                        debug_log!("Loaded {} connection(s)", connections.len());
                        connections
                    }
                }
                Err(e) => {
                    debug_log!("Failed to parse config file: {}", e);
                    default_connections()
                }
            },
            Err(e) => {
                debug_log!("Failed to read config file: {}", e);
                default_connections()
            }
        },
        None => {
            debug_log!("No config file found, using defaults");
            if let Some(config_dir) = get_config_dir() {
                let expected = config_dir.join("config.toml");
                debug_log!("Expected config at: {:?}", expected);
            }
            default_connections()
        }
    }
}

fn get_config_dir() -> Option<PathBuf> {
    // Use ~/.config on all platforms for consistency
    dirs::home_dir().map(|h| h.join(".config").join("sqli"))
}

fn get_config_path() -> Option<PathBuf> {
    // Priority 1: current directory
    let local_config = PathBuf::from("sqli.toml");
    if local_config.exists() {
        return Some(local_config);
    }

    // Priority 2: user config directory (~/.config/sqli/config.toml)
    if let Some(config_dir) = get_config_dir() {
        let user_config = config_dir.join("config.toml");
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
        password: None,
        password_cmd: None,
        tls: false,
        readonly: false,
    }]
}
