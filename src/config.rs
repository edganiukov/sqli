use crate::controller::{DatabaseConn, DatabaseType};
use indexmap::IndexMap;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct ConnectionConfig {
    #[serde(rename = "type")]
    pub db_type: String,
    #[serde(default)]
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
    pub database: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub group: Option<String>,
    /// Protocol for ClickHouse: "native" (default) or "http"
    #[serde(default)]
    pub protocol: Option<String>,
}

impl ConnectionConfig {
    pub fn to_database_conn(&self, name: &str) -> Option<DatabaseConn> {
        let db_type = match self.db_type.to_lowercase().as_str() {
            "postgres" | "postgresql" => DatabaseType::Postgres,
            "mysql" | "mariadb" => DatabaseType::MySql,
            "cassandra" | "scylla" => DatabaseType::Cassandra,
            "clickhouse" | "ch" => DatabaseType::ClickHouse,
            "sqlite" | "sqlite3" => DatabaseType::Sqlite,
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
            database: self.database.clone(),
            path: self.path.clone(),
            tls: self.tls,
            readonly: self.readonly,
            group: self.group.clone(),
            protocol: self.protocol.clone(),
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
            Ok(content) => match toml::from_str::<IndexMap<String, ConnectionConfig>>(&content) {
                Ok(configs) => {
                    debug_log!("Parsed {} connection(s) from config", configs.len());
                    let connections: Vec<DatabaseConn> = configs
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
        database: None,
        path: None,
        tls: false,
        readonly: false,
        group: None,
        protocol: None,
    }]
}

/// Parse a connection string URL into a DatabaseConn.
/// Format: <type>://[user[:pass]@]host[:port][/database]
/// Types: pg, my, cs, ch, chh, sq (add 's' suffix for TLS: pgs, mys, css, chs, chhs)
///
/// ClickHouse protocols:
///   ch  - Native TCP protocol (default, port 9000)
///   chh - HTTP API (port 8123)
///
/// SQLite:
///   sq:///path/to/database.db               (local, absolute path)
///   sq://./local.db                         (local, relative path)
///   sq://user@host/path/to/database.db     (remote via SSH)
///   sq://user@host:22/path/to/database.db  (remote via SSH with port)
///
/// Examples:
///   pg://postgres:secret@localhost:5432/mydb
///   pgs://postgres@secure.example.com/mydb  (with TLS)
///   my://root@localhost:3306
///   cs://user:pass@cassandra.example.com/keyspace
///   ch://default@localhost:9000/default     (Native protocol, default)
///   chh://default@localhost:8123/default    (HTTP API)
///   sq:///path/to/database.db
///   sq://./local.db
///   sq://deploy@server.example.com/home/user/app.db
pub fn parse_connection_string(url: &str) -> Result<DatabaseConn, String> {
    // Parse scheme (type)
    let (scheme, rest) = url
        .split_once("://")
        .ok_or("Invalid URL: missing '://' separator")?;

    // Parse scheme: type, TLS, and protocol (for ClickHouse: native vs http)
    // ClickHouse: ch/chs = native (default), chh/chhs = HTTP
    let (db_type, tls, protocol) = match scheme.to_lowercase().as_str() {
        "pg" | "postgres" | "postgresql" => (DatabaseType::Postgres, false, None),
        "pgs" | "postgress" | "postgresqls" => (DatabaseType::Postgres, true, None),
        "my" | "mysql" | "mariadb" => (DatabaseType::MySql, false, None),
        "mys" | "mysqls" | "mariadbs" => (DatabaseType::MySql, true, None),
        "cs" | "cassandra" | "scylla" => (DatabaseType::Cassandra, false, None),
        "css" | "cassandras" | "scyllas" => (DatabaseType::Cassandra, true, None),
        "ch" | "clickhouse" => (DatabaseType::ClickHouse, false, None),
        "chs" | "clickhouses" => (DatabaseType::ClickHouse, true, None),
        "chh" | "clickhouse-http" => (DatabaseType::ClickHouse, false, Some("http".to_string())),
        "chhs" | "clickhouse-https" => (DatabaseType::ClickHouse, true, Some("http".to_string())),
        "sq" | "sqlite" | "sqlite3" => (DatabaseType::Sqlite, false, None),
        _ => {
            return Err(format!(
                "Unknown database type: '{}'. Use pg, my, cs, ch, chh, or sq (add 's' for TLS)",
                scheme
            ));
        }
    };

    // SQLite has special handling
    // Formats:
    //   sq:///absolute/path.db         -> local, path="/absolute/path.db"
    //   sq://./relative/path.db        -> local, path="./relative/path.db"
    //   sq://user@host/path.db         -> SSH, host="user@host", path="/path.db"
    //   sq://user@host:22/path.db      -> SSH, host="user@host:22", path="/path.db"
    if matches!(db_type, DatabaseType::Sqlite) {
        if rest.is_empty() {
            return Err("SQLite requires a file path".to_string());
        }

        // Check if it's a local path (starts with / or .)
        let is_local = rest.starts_with('/') || rest.starts_with('.');

        let (host, path) = if is_local {
            // Local file: sq:///path or sq://./path
            (String::new(), rest.to_string())
        } else {
            // Remote via SSH: sq://user@host[:port]/path
            // Find the first / after the host part
            if let Some(slash_idx) = rest.find('/') {
                let host_part = &rest[..slash_idx];
                let path_part = &rest[slash_idx..]; // includes leading /
                (host_part.to_string(), path_part.to_string())
            } else {
                return Err(
                    "SSH SQLite requires a path after host (e.g., sq://user@host/path.db)"
                        .to_string(),
                );
            }
        };

        let name = if host.is_empty() {
            path.rsplit('/').next().unwrap_or(&path).to_string()
        } else {
            format!("{}:{}", host, path.rsplit('/').next().unwrap_or(&path))
        };

        return Ok(DatabaseConn {
            name,
            db_type,
            host,
            port: 0,
            user: String::new(),
            password: None,
            password_cmd: None,
            database: None,
            path: Some(path),
            tls: false,
            readonly: false,
            group: None,
            protocol: None,
        });
    }

    // Parse user:pass@host:port/database
    let (auth_host, database) = if let Some(idx) = rest.rfind('/') {
        let db = &rest[idx + 1..];
        let db = if db.is_empty() {
            None
        } else {
            Some(db.to_string())
        };
        (&rest[..idx], db)
    } else {
        (rest, None)
    };

    // Split auth from host
    let (user, password, host_port) = if let Some(idx) = auth_host.rfind('@') {
        let auth = &auth_host[..idx];
        let host_port = &auth_host[idx + 1..];

        let (user, password) = if let Some(colon_idx) = auth.find(':') {
            let user = &auth[..colon_idx];
            let pass = &auth[colon_idx + 1..];
            (user.to_string(), Some(pass.to_string()))
        } else {
            (auth.to_string(), None)
        };

        (user, password, host_port)
    } else {
        // No auth, use defaults
        let default_user = match db_type {
            DatabaseType::Postgres => "postgres",
            DatabaseType::MySql => "root",
            DatabaseType::ClickHouse => "default",
            _ => "",
        };
        (default_user.to_string(), None, auth_host)
    };

    // Split host and port
    let (host, port) = if let Some(colon_idx) = host_port.rfind(':') {
        let h = &host_port[..colon_idx];
        let p = &host_port[colon_idx + 1..];
        let port: u16 = p.parse().map_err(|_| format!("Invalid port: '{}'", p))?;
        (h.to_string(), port)
    } else {
        // Use default port for the database type
        let default_port = match (&db_type, &protocol) {
            (DatabaseType::Postgres, _) => 5432,
            (DatabaseType::MySql, _) => 3306,
            (DatabaseType::Cassandra, _) => 9042,
            (DatabaseType::ClickHouse, Some(p)) if p == "http" => 8123,
            (DatabaseType::ClickHouse, _) => 9000, // native is default
            (DatabaseType::Sqlite, _) => 0,
        };
        (host_port.to_string(), default_port)
    };

    // Use localhost if host is empty
    let host = if host.is_empty() {
        "localhost".to_string()
    } else {
        host
    };

    // Generate a connection name
    let name = if let Some(ref db) = database {
        format!("{}@{}/{}", user, host, db)
    } else {
        format!("{}@{}", user, host)
    };

    Ok(DatabaseConn {
        name,
        db_type,
        host,
        port,
        user,
        password,
        password_cmd: None,
        database,
        path: None,
        tls,
        readonly: false,
        group: None,
        protocol,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_postgres_full() {
        let conn = parse_connection_string("pg://myuser:secret@dbhost:5433/mydb").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Postgres));
        assert_eq!(conn.user, "myuser");
        assert_eq!(conn.password, Some("secret".to_string()));
        assert_eq!(conn.host, "dbhost");
        assert_eq!(conn.port, 5433);
        assert_eq!(conn.database, Some("mydb".to_string()));
    }

    #[test]
    fn test_parse_postgres_no_db() {
        let conn = parse_connection_string("pg://postgres@localhost").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Postgres));
        assert_eq!(conn.user, "postgres");
        assert_eq!(conn.password, None);
        assert_eq!(conn.host, "localhost");
        assert_eq!(conn.port, 5432); // default port
        assert_eq!(conn.database, None);
    }

    #[test]
    fn test_parse_postgres_no_auth() {
        let conn = parse_connection_string("pg://localhost:5432/testdb").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Postgres));
        assert_eq!(conn.user, "postgres"); // default user
        assert_eq!(conn.host, "localhost");
        assert_eq!(conn.port, 5432);
        assert_eq!(conn.database, Some("testdb".to_string()));
    }

    #[test]
    fn test_parse_mysql() {
        let conn = parse_connection_string("my://root:pass@mysql.local:3307/app").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::MySql));
        assert_eq!(conn.user, "root");
        assert_eq!(conn.password, Some("pass".to_string()));
        assert_eq!(conn.host, "mysql.local");
        assert_eq!(conn.port, 3307);
        assert_eq!(conn.database, Some("app".to_string()));
    }

    #[test]
    fn test_parse_clickhouse() {
        let conn = parse_connection_string("ch://default@localhost/default").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::ClickHouse));
        assert_eq!(conn.user, "default");
        assert_eq!(conn.host, "localhost");
        assert_eq!(conn.port, 9000); // native port (default)
        assert_eq!(conn.database, Some("default".to_string()));
    }

    #[test]
    fn test_parse_cassandra() {
        let conn = parse_connection_string("cs://cassandra:pass@node1:9043/keyspace").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Cassandra));
        assert_eq!(conn.user, "cassandra");
        assert_eq!(conn.password, Some("pass".to_string()));
        assert_eq!(conn.host, "node1");
        assert_eq!(conn.port, 9043);
        assert_eq!(conn.database, Some("keyspace".to_string()));
    }

    #[test]
    fn test_parse_sqlite() {
        let conn = parse_connection_string("sq:///path/to/database.db").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Sqlite));
        assert_eq!(conn.path, Some("/path/to/database.db".to_string()));
        assert_eq!(conn.name, "database.db");
    }

    #[test]
    fn test_parse_sqlite_relative() {
        let conn = parse_connection_string("sq://./local.db").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Sqlite));
        assert_eq!(conn.path, Some("./local.db".to_string()));
        assert_eq!(conn.name, "local.db");
    }

    #[test]
    fn test_parse_postgres_tls() {
        let conn = parse_connection_string("pgs://postgres@secure.example.com/mydb").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Postgres));
        assert!(conn.tls);
        assert_eq!(conn.host, "secure.example.com");
        assert_eq!(conn.database, Some("mydb".to_string()));
    }

    #[test]
    fn test_parse_mysql_tls() {
        let conn = parse_connection_string("mys://root@localhost:3306/app").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::MySql));
        assert!(conn.tls);
    }

    #[test]
    fn test_parse_clickhouse_tls() {
        let conn = parse_connection_string("chs://default@ch.example.com:9440/default").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::ClickHouse));
        assert!(conn.tls);
        assert_eq!(conn.port, 9440); // native TLS port
    }

    #[test]
    fn test_parse_cassandra_tls() {
        let conn = parse_connection_string("css://user@node1/keyspace").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Cassandra));
        assert!(conn.tls);
    }

    #[test]
    fn test_parse_no_tls_by_default() {
        let conn = parse_connection_string("pg://postgres@localhost").unwrap();
        assert!(!conn.tls);
    }

    #[test]
    fn test_parse_invalid_scheme() {
        let result = parse_connection_string("invalid://localhost");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown database type"));
    }

    #[test]
    fn test_parse_missing_separator() {
        let result = parse_connection_string("pg:localhost");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing '://'"));
    }

    #[test]
    fn test_parse_clickhouse_native_default() {
        let conn = parse_connection_string("ch://default@localhost/default").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::ClickHouse));
        assert_eq!(conn.protocol, None); // Native is default (None)
        assert_eq!(conn.port, 9000); // native port default
        assert!(!conn.tls);
    }

    #[test]
    fn test_parse_clickhouse_native_tls() {
        let conn = parse_connection_string("chs://default@ch.example.com/default").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::ClickHouse));
        assert_eq!(conn.protocol, None); // Native is default
        assert!(conn.tls);
        assert_eq!(conn.port, 9000); // native port default
    }

    #[test]
    fn test_parse_clickhouse_http() {
        let conn = parse_connection_string("chh://default@localhost/default").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::ClickHouse));
        assert_eq!(conn.protocol, Some("http".to_string()));
        assert_eq!(conn.port, 8123); // HTTP port default
        assert!(!conn.tls);
    }

    #[test]
    fn test_parse_clickhouse_https() {
        let conn = parse_connection_string("chhs://default@ch.example.com:8443/default").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::ClickHouse));
        assert_eq!(conn.protocol, Some("http".to_string()));
        assert!(conn.tls);
        assert_eq!(conn.port, 8443);
    }

    #[test]
    fn test_parse_sqlite_ssh() {
        let conn =
            parse_connection_string("sq://deploy@server.example.com/home/user/app.db").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Sqlite));
        assert_eq!(conn.host, "deploy@server.example.com");
        assert_eq!(conn.path, Some("/home/user/app.db".to_string()));
        assert_eq!(conn.name, "deploy@server.example.com:app.db");
    }

    #[test]
    fn test_parse_sqlite_ssh_with_port() {
        let conn =
            parse_connection_string("sq://deploy@server.example.com:2222/data/app.db").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Sqlite));
        assert_eq!(conn.host, "deploy@server.example.com:2222");
        assert_eq!(conn.path, Some("/data/app.db".to_string()));
    }

    #[test]
    fn test_parse_sqlite_local_absolute() {
        let conn = parse_connection_string("sq:///var/lib/data.db").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Sqlite));
        assert_eq!(conn.host, ""); // empty = local
        assert_eq!(conn.path, Some("/var/lib/data.db".to_string()));
    }

    #[test]
    fn test_parse_sqlite_local_relative() {
        let conn = parse_connection_string("sq://./data/local.db").unwrap();
        assert!(matches!(conn.db_type, DatabaseType::Sqlite));
        assert_eq!(conn.host, ""); // empty = local
        assert_eq!(conn.path, Some("./data/local.db".to_string()));
    }
}
