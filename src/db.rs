use std::error::Error;

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

/// Database client abstraction
pub enum DatabaseClient {
    Postgres(crate::postgres::PostgresClient),
    MySql(crate::mysql::MySqlClient),
    Cassandra(crate::cassandra::CassandraClient),
    ClickHouse(crate::clickhouse::ClickHouseClient),
}

impl DatabaseClient {
    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        match self {
            DatabaseClient::Postgres(client) => client.list_databases(include_system).await.map_err(|e| e.into()),
            DatabaseClient::MySql(client) => client.list_databases(include_system).await,
            DatabaseClient::Cassandra(client) => client.list_databases(include_system).await.map_err(|e| e.into()),
            DatabaseClient::ClickHouse(client) => client.list_databases(include_system).await,
        }
    }

    pub async fn list_tables(&self, schema: &str) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        match self {
            DatabaseClient::Postgres(client) => client.list_tables(schema).await.map_err(|e| e.into()),
            DatabaseClient::MySql(client) => client.list_tables(schema).await,
            DatabaseClient::Cassandra(client) => client.list_tables(schema).await.map_err(|e| e.into()),
            DatabaseClient::ClickHouse(client) => client.list_tables(schema).await,
        }
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult, Box<dyn Error + Send + Sync>> {
        match self {
            DatabaseClient::Postgres(client) => client.execute_query(query).await.map_err(|e| e.into()),
            DatabaseClient::MySql(client) => client.execute_query(query).await,
            DatabaseClient::Cassandra(client) => client.execute_query(query).await.map_err(|e| e.into()),
            DatabaseClient::ClickHouse(client) => client.execute_query(query).await,
        }
    }

    /// Generate a SELECT query for previewing table contents
    pub fn select_table_query(&self, table: &str, limit: usize) -> String {
        match self {
            DatabaseClient::Postgres(_) => crate::postgres::PostgresClient::select_table_query(table, limit),
            DatabaseClient::MySql(_) => crate::mysql::MySqlClient::select_table_query(table, limit),
            DatabaseClient::Cassandra(_) => crate::cassandra::CassandraClient::select_table_query(table, limit),
            DatabaseClient::ClickHouse(_) => crate::clickhouse::ClickHouseClient::select_table_query(table, limit),
        }
    }

    /// Generate a query to describe table structure
    pub fn describe_table_query(&self, table: &str, schema: Option<&str>) -> String {
        match self {
            DatabaseClient::Postgres(_) => crate::postgres::PostgresClient::describe_table_query(table),
            DatabaseClient::MySql(_) => crate::mysql::MySqlClient::describe_table_query(table, schema),
            DatabaseClient::Cassandra(_) => crate::cassandra::CassandraClient::describe_table_query(table, schema),
            DatabaseClient::ClickHouse(_) => crate::clickhouse::ClickHouseClient::describe_table_query(table, schema),
        }
    }
}
