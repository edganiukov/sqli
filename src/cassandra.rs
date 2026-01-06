use crate::db::QueryResult;
use chrono::{DateTime, NaiveDate, Utc};
use scylla::frame::response::result::CqlValue;
use scylla::frame::value::{CqlDate, CqlDecimal, CqlTime, CqlTimestamp};
use scylla::{Session, SessionBuilder};
use std::error::Error;
use std::sync::Arc;

pub struct CassandraClient {
    session: Arc<Session>,
}

impl CassandraClient {
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
        keyspace: &str,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let addr = format!("{}:{}", host, port);

        let mut builder = SessionBuilder::new().known_node(&addr);

        if !user.is_empty() {
            builder = builder.user(user, password);
        }

        let session = builder.build().await?;

        // Use keyspace if provided
        if !keyspace.is_empty() {
            session
                .query_unpaged(format!("USE {}", keyspace), &[])
                .await?;
        }

        Ok(Self {
            session: Arc::new(session),
        })
    }

    /// List all keyspaces (equivalent to databases in PostgreSQL)
    pub async fn list_databases(
        &self,
        include_system: bool,
    ) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let rows = self
            .session
            .query_unpaged("SELECT keyspace_name FROM system_schema.keyspaces", &[])
            .await?;

        const SYSTEM_KEYSPACES: &[&str] = &[
            "system",
            "system_schema",
            "system_traces",
            "system_auth",
            "system_distributed",
            "system_distributed_everywhere",
            "system_virtual_schema",
            "system_replicated_keys",
        ];

        let mut keyspaces = Vec::new();
        if let Some(rows) = rows.rows {
            for row in rows {
                if let Some(CqlValue::Text(s) | CqlValue::Ascii(s)) =
                    row.columns.get(0).and_then(|v| v.as_ref())
                {
                    if include_system || !SYSTEM_KEYSPACES.contains(&s.as_str()) {
                        keyspaces.push(s.clone());
                    }
                }
            }
        }
        keyspaces.sort();
        Ok(keyspaces)
    }

    /// List tables in a keyspace
    pub async fn list_tables(
        &self,
        keyspace: &str,
    ) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let rows = self
            .session
            .query_unpaged(
                "SELECT keyspace_name, table_name FROM system_schema.tables",
                &[],
            )
            .await?;

        let mut tables = Vec::new();
        if let Some(rows) = rows.rows {
            for row in rows {
                // Check if keyspace matches
                if let Some(CqlValue::Text(ks) | CqlValue::Ascii(ks)) =
                    row.columns.get(0).and_then(|v| v.as_ref())
                {
                    if ks == keyspace {
                        if let Some(CqlValue::Text(t) | CqlValue::Ascii(t)) =
                            row.columns.get(1).and_then(|v| v.as_ref())
                        {
                            tables.push(t.clone());
                        }
                    }
                }
            }
        }
        tables.sort();
        Ok(tables)
    }

    pub async fn execute_query(
        &self,
        query: &str,
    ) -> Result<QueryResult, Box<dyn Error + Send + Sync>> {
        let query_upper = query.trim().to_uppercase();

        if query_upper.starts_with("SELECT") {
            let result = self.session.query_unpaged(query, &[]).await?;

            let columns: Vec<String> = result
                .col_specs()
                .iter()
                .map(|spec| spec.name.clone())
                .collect();

            let mut data_rows: Vec<Vec<String>> = Vec::new();
            if let Some(rows) = result.rows {
                for row in rows {
                    let row_data: Vec<String> = row
                        .columns
                        .iter()
                        .map(|col| Self::format_column_value(col))
                        .collect();
                    data_rows.push(row_data);
                }
            }

            Ok(QueryResult::Select {
                columns,
                rows: data_rows,
            })
        } else {
            // For non-SELECT queries, execute and report success
            self.session.query_unpaged(query, &[]).await?;
            Ok(QueryResult::Execute { rows_affected: 0 })
        }
    }

    fn format_column_value(value: &Option<CqlValue>) -> String {
        match value {
            None => "NULL".to_string(),
            Some(v) => match v {
                CqlValue::Ascii(s) => s.clone(),
                CqlValue::Text(s) => s.clone(),
                CqlValue::Int(i) => i.to_string(),
                CqlValue::BigInt(i) => i.to_string(),
                CqlValue::Float(f) => f.to_string(),
                CqlValue::Double(d) => d.to_string(),
                CqlValue::Boolean(b) => b.to_string(),
                CqlValue::Uuid(u) => u.to_string(),
                CqlValue::Timestamp(t) => Self::format_timestamp(t),
                CqlValue::Blob(b) => format!("<blob {} bytes>", b.len()),
                CqlValue::List(l) => format!("[{} items]", l.len()),
                CqlValue::Map(m) => format!("{{map {} entries}}", m.len()),
                CqlValue::Set(s) => format!("{{set {} items}}", s.len()),
                CqlValue::SmallInt(i) => i.to_string(),
                CqlValue::TinyInt(i) => i.to_string(),
                CqlValue::Inet(addr) => addr.to_string(),
                CqlValue::Date(d) => Self::format_date(d),
                CqlValue::Time(t) => Self::format_time(t),
                CqlValue::Varint(v) => format!("{:?}", v),
                CqlValue::Decimal(d) => Self::format_decimal(d),
                _ => format!("{:?}", v),
            },
        }
    }

    fn format_timestamp(t: &CqlTimestamp) -> String {
        // CqlTimestamp contains milliseconds since Unix epoch
        let millis = t.0;
        let secs = millis / 1000;
        let nsecs = ((millis % 1000) * 1_000_000) as u32;
        match DateTime::<Utc>::from_timestamp(secs, nsecs) {
            Some(dt) => dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            None => format!("{}", millis),
        }
    }

    fn format_date(d: &CqlDate) -> String {
        // CqlDate is days since Unix epoch (1970-01-01)
        let days = d.0 as i64 - (1 << 31); // CqlDate uses unsigned with offset
        match NaiveDate::from_num_days_from_ce_opt(days as i32 + 719163) {
            Some(date) => date.format("%Y-%m-%d").to_string(),
            None => format!("{}", d.0),
        }
    }

    fn format_time(t: &CqlTime) -> String {
        // CqlTime is nanoseconds since midnight
        let nanos = t.0;
        let secs = nanos / 1_000_000_000;
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let secs = secs % 60;
        let millis = (nanos % 1_000_000_000) / 1_000_000;
        format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
    }

    fn format_decimal(d: &CqlDecimal) -> String {
        use num_bigint::BigInt;
        let (int_val, scale) = d.as_signed_be_bytes_slice_and_exponent();
        let bigint = BigInt::from_signed_bytes_be(int_val);
        let scale = scale as usize;

        let s = bigint.to_string();
        let negative = s.starts_with('-');
        let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();

        if scale == 0 {
            if negative {
                format!("-{}", digits)
            } else {
                digits
            }
        } else if scale >= digits.len() {
            let zeros = "0".repeat(scale - digits.len());
            if negative {
                format!("-0.{}{}", zeros, digits)
            } else {
                format!("0.{}{}", zeros, digits)
            }
        } else {
            let (int_part, frac_part) = digits.split_at(digits.len() - scale);
            if negative {
                format!("-{}.{}", int_part, frac_part)
            } else {
                format!("{}.{}", int_part, frac_part)
            }
        }
    }

    /// Generate a SELECT query for previewing table contents
    pub fn select_table_query(table: &str, limit: usize) -> String {
        format!("SELECT * FROM {} LIMIT {};", table, limit)
    }

    /// Generate a query to describe table structure
    pub fn describe_table_query(table: &str, keyspace: Option<&str>) -> String {
        match keyspace {
            Some(ks) => format!(
                "SELECT column_name, type, kind \n\
                 FROM system_schema.columns \n\
                 WHERE keyspace_name = '{}' AND table_name = '{}';",
                ks, table
            ),
            None => format!(
                "SELECT column_name, type, kind \n\
                 FROM system_schema.columns \n\
                 WHERE table_name = '{}';",
                table
            ),
        }
    }

    /// Default keyspace to connect to when none is selected (empty = no keyspace)
    pub fn default_database() -> &'static str {
        ""
    }
}
