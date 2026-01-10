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
}

impl DatabaseClient {
    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>> {
        match self {
            DatabaseClient::Postgres(client) => client.list_databases(include_system).await,
            DatabaseClient::MySql(client) => client.list_databases(include_system).await,
            DatabaseClient::Cassandra(client) => client.list_databases(include_system).await,
            DatabaseClient::ClickHouse(client) => client.list_databases(include_system).await,
        }
    }

    pub async fn list_tables(&self, schema: &str) -> Result<Vec<String>> {
        match self {
            DatabaseClient::Postgres(client) => client.list_tables(schema).await,
            DatabaseClient::MySql(client) => client.list_tables(schema).await,
            DatabaseClient::Cassandra(client) => client.list_tables(schema).await,
            DatabaseClient::ClickHouse(client) => client.list_tables(schema).await,
        }
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
        match self {
            DatabaseClient::Postgres(client) => client.execute_query(query).await,
            DatabaseClient::MySql(client) => client.execute_query(query).await,
            DatabaseClient::Cassandra(client) => client.execute_query(query).await,
            DatabaseClient::ClickHouse(client) => client.execute_query(query).await,
        }
    }

    /// Generate a SELECT query for previewing table contents
    pub fn select_table_query(&self, table: &str, limit: usize) -> String {
        match self {
            DatabaseClient::Postgres(client) => client.select_table_query(table, limit),
            DatabaseClient::MySql(client) => client.select_table_query(table, limit),
            DatabaseClient::Cassandra(client) => client.select_table_query(table, limit),
            DatabaseClient::ClickHouse(client) => client.select_table_query(table, limit),
        }
    }

    /// Generate a query to describe table structure
    pub fn describe_table_query(&self, table: &str, schema: Option<&str>) -> String {
        match self {
            DatabaseClient::Postgres(client) => client.describe_table_query(table, schema),
            DatabaseClient::MySql(client) => client.describe_table_query(table, schema),
            DatabaseClient::Cassandra(client) => client.describe_table_query(table, schema),
            DatabaseClient::ClickHouse(client) => client.describe_table_query(table, schema),
        }
    }
}
