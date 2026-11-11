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
    /// Verify the server certificate when `tls` is on. On by default; turn it
    /// off for servers with a self-signed certificate.
    #[serde(default = "default_true")]
    pub tls_verify: bool,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub group: Option<String>,
    /// Protocol for ClickHouse: "native" (default) or "http"
    #[serde(default)]
    pub protocol: Option<String>,
}

fn default_true() -> bool {
    true
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
            tls_verify: self.tls_verify,
            readonly: self.readonly,
            group: self.group.clone(),
            protocol: self.protocol.clone(),
        })
    }
}

/// Load the connections from the config file.
///
/// With no config file at all the built-in default connection is used. A
/// config file that exists but cannot be read or parsed is an error: silently
/// falling back to the default would hide the mistake behind a `localhost`
/// entry the user never wrote.
pub fn load_config(custom_path: Option<PathBuf>) -> Result<Vec<DatabaseConn>, String> {
    use crate::debug_log;

    let Some(path) = custom_path.or_else(get_config_path) else {
        debug_log!("No config file found, using defaults");
        return Ok(default_connections());
    };
    debug_log!("Config path: {}", path.display());

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read config file {}: {}", path.display(), e))?;
    let connections = parse_config(&content)
        .map_err(|e| format!("Invalid config file {}: {}", path.display(), e))?;
    debug_log!("Loaded {} connection(s)", connections.len());
    Ok(connections)
}

fn parse_config(content: &str) -> Result<Vec<DatabaseConn>, String> {
    let configs: IndexMap<String, ConnectionConfig> =
        toml::from_str(content).map_err(|e| e.to_string())?;

    let mut connections = Vec::with_capacity(configs.len());
    for (name, config) in &configs {
        let conn = config.to_database_conn(name).ok_or_else(|| {
            format!(
                "connection '{}' has unknown type '{}' \
                 (expected postgres, mysql, cassandra, clickhouse or sqlite)",
                name, config.db_type
            )
        })?;
        connections.push(conn);
    }

    if connections.is_empty() {
        return Ok(default_connections());
    }
    Ok(connections)
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
        tls_verify: true,
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
            tls_verify: true,
            readonly: false,
            group: None,
            protocol: None,
        });
    }

    // Options after `?`, e.g. `pgs://host/db?tls_verify=false`
    let (rest, tls_verify) = split_url_options(rest)?;

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
        tls_verify,
        readonly: false,
        group: None,
        protocol,
    })
}

/// Split `?key=value&...` options off the end of a connection URL and apply
/// them. Returns the URL without the options and the `tls_verify` setting.
fn split_url_options(rest: &str) -> Result<(&str, bool), String> {
    let Some((rest, options)) = rest.split_once('?') else {
        return Ok((rest, true));
    };

    let mut tls_verify = true;
    for option in options.split('&').filter(|o| !o.is_empty()) {
        match option.split_once('=') {
            Some(("tls_verify", "true")) => tls_verify = true,
            Some(("tls_verify", "false")) => tls_verify = false,
            Some(("tls_verify", value)) => {
                return Err(format!(
                    "Invalid value for tls_verify: '{}' (expected true or false)",
                    value
                ));
            }
            _ => {
                return Err(format!(
                    "Unknown connection option: '{}' (supported: tls_verify=false)",
                    option
                ));
            }
        }
    }
    Ok((rest, tls_verify))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_lists_connections_in_file_order() {
        let connections = parse_config(
            "[b]\ntype = \"postgres\"\nhost = \"db\"\n\n[a]\ntype = \"sqlite\"\npath = \"a.db\"\n",
        )
        .unwrap();
        let names: Vec<&str> = connections.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["b", "a"]);
    }

    #[test]
    fn config_syntax_error_is_reported() {
        let err = parse_config("[pg]\ntype = \"postgres\nhost = 1").unwrap_err();
        assert!(err.contains("line 2"), "unexpected message: {err}");
    }

    #[test]
    fn config_unknown_type_is_reported_with_the_connection_name() {
        let err = parse_config("[warehouse]\ntype = \"oracle\"\n").unwrap_err();
        assert!(err.contains("'warehouse'") && err.contains("'oracle'"));
    }

    #[test]
    fn empty_config_falls_back_to_the_default_connection() {
        let connections = parse_config("").unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].name, "localhost");
    }

    #[test]
    fn missing_explicit_config_file_is_an_error() {
        let err = load_config(Some(PathBuf::from("/nonexistent/sqli.toml"))).unwrap_err();
        assert!(err.contains("/nonexistent/sqli.toml"));
    }

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
    fn tls_is_verified_unless_turned_off() {
        let conn = parse_connection_string("pgs://user@host/db").unwrap();
        assert_eq!(conn.tls_mode(), crate::db::Tls::Verified);

        let conn = parse_connection_string("pgs://user@host/db?tls_verify=false").unwrap();
        assert_eq!(conn.tls_mode(), crate::db::Tls::Unverified);
        assert_eq!(conn.database.as_deref(), Some("db"));

        // Without a database the option still parses, and the host is intact.
        let conn = parse_connection_string("mys://root@host:3307?tls_verify=false").unwrap();
        assert_eq!(conn.tls_mode(), crate::db::Tls::Unverified);
        assert_eq!((conn.host.as_str(), conn.port), ("host", 3307));

        // The option alone does not switch TLS on.
        let conn = parse_connection_string("pg://user@host/db?tls_verify=false").unwrap();
        assert_eq!(conn.tls_mode(), crate::db::Tls::Off);
    }

    #[test]
    fn unknown_url_options_are_rejected() {
        let err = parse_connection_string("pgs://host/db?sslmode=require").unwrap_err();
        assert!(err.contains("sslmode=require"), "unexpected message: {err}");
        let err = parse_connection_string("pgs://host/db?tls_verify=no").unwrap_err();
        assert!(err.contains("tls_verify"), "unexpected message: {err}");
    }

    #[test]
    fn config_tls_verify_defaults_to_on() {
        let connections = parse_config(
            "[a]\ntype = \"postgres\"\ntls = true\n\n\
             [b]\ntype = \"postgres\"\ntls = true\ntls_verify = false\n\n\
             [c]\ntype = \"postgres\"\n",
        )
        .unwrap();
        let modes: Vec<_> = connections.iter().map(|c| c.tls_mode()).collect();
        use crate::db::Tls;
        assert_eq!(modes, [Tls::Verified, Tls::Unverified, Tls::Off]);
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
