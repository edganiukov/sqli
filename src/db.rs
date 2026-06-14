use crate::error::Result;

/// Result of executing a query
#[derive(Debug, Clone)]
pub enum QueryResult {
    Select {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Execute {
        rows_affected: u64,
    },
}

/// Database client abstraction using enum dispatch
pub enum DatabaseClient {
    Postgres(crate::postgres::PostgresClient),
    MySql(crate::mysql::MySqlClient),
    Cassandra(crate::cassandra::CassandraClient),
    ClickHouse(crate::clickhouse::ClickHouseClient),
    Sqlite(crate::sqlite::SqliteClient),
    SshSqlite(crate::ssh_sqlite::SshSqliteClient),
}

/// Forward a method call to the inner client of every `DatabaseClient` variant.
///
/// The variants share method names and (for these calls) argument lists, so the
/// per-backend match arms are pure boilerplate. `list_columns` is the exception
/// (its arguments differ per backend) and stays hand-written below.
macro_rules! dispatch {
    ($self:ident, $method:ident ( $($arg:expr),* ) $(.$await:ident)?) => {
        match $self {
            DatabaseClient::Postgres(client) => client.$method($($arg),*) $(.$await)?,
            DatabaseClient::MySql(client) => client.$method($($arg),*) $(.$await)?,
            DatabaseClient::Cassandra(client) => client.$method($($arg),*) $(.$await)?,
            DatabaseClient::ClickHouse(client) => client.$method($($arg),*) $(.$await)?,
            DatabaseClient::Sqlite(client) => client.$method($($arg),*) $(.$await)?,
            DatabaseClient::SshSqlite(client) => client.$method($($arg),*) $(.$await)?,
        }
    };
}

impl DatabaseClient {
    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>> {
        dispatch!(self, list_databases(include_system).await)
    }

    pub async fn list_tables(&self, schema: &str) -> Result<Vec<String>> {
        dispatch!(self, list_tables(schema).await)
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        dispatch!(self, execute_query(query).await)
    }

    /// Generate a SELECT query for previewing table contents
    pub fn select_table_query(&self, table: &str, limit: usize, schema: Option<&str>) -> String {
        dispatch!(self, select_table_query(table, limit, schema))
    }

    /// Generate a query to describe table structure
    pub fn describe_table_query(&self, table: &str, schema: Option<&str>) -> String {
        dispatch!(self, describe_table_query(table, schema))
    }

    /// Get column names for a table
    pub async fn list_columns(&self, table: &str, schema: Option<&str>) -> Result<Vec<String>> {
        match self {
            DatabaseClient::Postgres(client) => client.list_columns(table).await,
            DatabaseClient::MySql(client) => client.list_columns(table).await,
            DatabaseClient::Cassandra(client) => client.list_columns(table, schema).await,
            DatabaseClient::ClickHouse(client) => client.list_columns(table, schema).await,
            DatabaseClient::Sqlite(client) => client.list_columns(table, schema).await,
            DatabaseClient::SshSqlite(client) => client.list_columns(table, schema).await,
        }
    }
}
