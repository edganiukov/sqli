use std::error::Error;
use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, SqliError>;

#[derive(Debug, ThisError)]
pub enum SqliError {
    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("Query failed: {0}")]
    Query(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl SqliError {
    /// Whether this error means the connection itself is unusable (as opposed
    /// to the server rejecting a statement over a working connection).
    pub fn is_connection(&self) -> bool {
        matches!(self, SqliError::Connection(_) | SqliError::Io(_))
    }

    /// The error text without the category prefix that `Display` adds.
    pub fn message(&self) -> String {
        match self {
            SqliError::Connection(msg) | SqliError::Query(msg) | SqliError::Other(msg) => {
                msg.clone()
            }
            SqliError::Io(e) => e.to_string(),
        }
    }
}

/// SQLSTATE codes the server sends while tearing a session down: class 08
/// (connection exception) and the 57P0x shutdown family.
fn is_pg_connection_state(code: &str) -> bool {
    code.starts_with("08") || matches!(code, "57P01" | "57P02" | "57P03")
}

impl From<tokio_postgres::Error> for SqliError {
    fn from(e: tokio_postgres::Error) -> Self {
        // Log full error for debugging
        crate::debug_log!("Postgres error: {:?}", e);

        // Extract detailed error info from postgres
        if let Some(db_err) = e.as_db_error() {
            crate::debug_log!(
                "DB error details - severity: {}, code: {}, message: {}, detail: {:?}, hint: {:?}",
                db_err.severity(),
                db_err.code().code(),
                db_err.message(),
                db_err.detail(),
                db_err.hint()
            );
            let mut msg = db_err.message().to_string();
            if let Some(detail) = db_err.detail() {
                msg.push_str(" - ");
                msg.push_str(detail);
            }
            if let Some(hint) = db_err.hint() {
                msg.push_str(" (hint: ");
                msg.push_str(hint);
                msg.push(')');
            }
            if is_pg_connection_state(db_err.code().code()) {
                SqliError::Connection(msg)
            } else {
                SqliError::Query(msg)
            }
        } else if let Some(source) = e.source() {
            // IO or other connection errors
            SqliError::Connection(source.to_string())
        } else {
            SqliError::Connection(e.to_string())
        }
    }
}

impl From<mysql_async::Error> for SqliError {
    fn from(e: mysql_async::Error) -> Self {
        match e {
            // The server answered, so the link works: this is a statement error.
            mysql_async::Error::Server(err) => SqliError::Query(err.to_string()),
            other => SqliError::Connection(other.to_string()),
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for SqliError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        SqliError::Other(e.to_string())
    }
}

impl From<reqwest::Error> for SqliError {
    fn from(e: reqwest::Error) -> Self {
        SqliError::Connection(e.to_string())
    }
}

impl From<serde_json::Error> for SqliError {
    fn from(e: serde_json::Error) -> Self {
        SqliError::Query(e.to_string())
    }
}

impl From<scylla::transport::errors::NewSessionError> for SqliError {
    fn from(e: scylla::transport::errors::NewSessionError) -> Self {
        SqliError::Connection(e.to_string())
    }
}

impl From<scylla::transport::errors::QueryError> for SqliError {
    fn from(e: scylla::transport::errors::QueryError) -> Self {
        SqliError::Query(e.to_string())
    }
}

impl From<clickhouse_rs::errors::Error> for SqliError {
    fn from(e: clickhouse_rs::errors::Error) -> Self {
        use clickhouse_rs::errors::Error;
        match &e {
            Error::Driver(_) | Error::Io(_) | Error::Connection(_) => {
                SqliError::Connection(e.to_string())
            }
            Error::Server(server_err) => SqliError::Query(server_err.message.clone()),
            _ => SqliError::Query(e.to_string()),
        }
    }
}
