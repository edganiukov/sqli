use thiserror::Error;

pub type Result<T> = std::result::Result<T, SqliError>;

#[derive(Debug, Error)]
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

impl From<tokio_postgres::Error> for SqliError {
    fn from(e: tokio_postgres::Error) -> Self {
        SqliError::Connection(e.to_string())
    }
}

impl From<mysql_async::Error> for SqliError {
    fn from(e: mysql_async::Error) -> Self {
        SqliError::Connection(e.to_string())
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
