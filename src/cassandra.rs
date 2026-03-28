use crate::db::QueryResult;
use crate::error::Result;
use crate::format as fmt;

use chrono::{DateTime, NaiveDate, Utc};
use scylla::frame::response::result::CqlValue;
use scylla::frame::value::{CqlDate, CqlDecimal, CqlDuration, CqlTime, CqlTimestamp};
use scylla::{Session, SessionBuilder};
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
    ) -> Result<Self> {
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

    pub async fn list_databases(&self, include_system: bool) -> Result<Vec<String>> {
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
                    row.columns.first().and_then(|v| v.as_ref())
                    && (include_system || !SYSTEM_KEYSPACES.contains(&s.as_str()))
                {
                    keyspaces.push(s.clone());
                }
            }
        }
        keyspaces.sort();
        Ok(keyspaces)
    }

    pub async fn list_tables(&self, keyspace: &str) -> Result<Vec<String>> {
        // Use WHERE clause to filter server-side for efficiency
        let rows = self
            .session
            .query_unpaged(
                "SELECT table_name FROM system_schema.tables WHERE keyspace_name = ?",
                (keyspace,),
            )
            .await?;

        let mut tables = Vec::new();
        if let Some(rows) = rows.rows {
            for row in rows {
                if let Some(CqlValue::Text(t) | CqlValue::Ascii(t)) =
                    row.columns.first().and_then(|v| v.as_ref())
                {
                    tables.push(t.clone());
                }
            }
        }
        tables.sort();
        Ok(tables)
    }

    pub async fn execute_query(&self, query: &str) -> Result<QueryResult> {
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
                    let row_data: Vec<String> =
                        row.columns.iter().map(Self::format_column_value).collect();
                    data_rows.push(row_data);
                }
            }

            Ok(QueryResult::Select {
                columns,
                rows: data_rows,
            })
        } else {
            self.session.query_unpaged(query, &[]).await?;
            Ok(QueryResult::Execute { rows_affected: 0 })
        }
    }

    pub fn select_table_query(&self, table: &str, limit: usize, keyspace: Option<&str>) -> String {
        match keyspace {
            Some(ks) => format!("SELECT * FROM {}.{} LIMIT {}", ks, table, limit),
            None => format!("SELECT * FROM {} LIMIT {}", table, limit),
        }
    }

    pub fn describe_table_query(&self, table: &str, keyspace: Option<&str>) -> String {
        match keyspace {
            Some(ks) => format!(
                "SELECT column_name, type, kind \n\
                 FROM system_schema.columns \n\
                 WHERE keyspace_name = '{}' AND table_name = '{}'",
                ks, table
            ),
            None => format!(
                "SELECT column_name, type, kind \n\
                 FROM system_schema.columns \n\
                 WHERE table_name = '{}'",
                table
            ),
        }
    }

    pub async fn list_columns(&self, table: &str, keyspace: Option<&str>) -> Result<Vec<String>> {
        let query = match keyspace {
            Some(ks) => format!(
                "SELECT column_name FROM system_schema.columns \
                 WHERE keyspace_name = '{}' AND table_name = '{}'",
                ks, table
            ),
            None => format!(
                "SELECT column_name FROM system_schema.columns WHERE table_name = '{}'",
                table
            ),
        };
        let result = self.session.query_unpaged(query, &[]).await?;
        let mut columns = Vec::new();
        if let Some(rows) = result.rows {
            for row in rows {
                if let Some(Some(scylla::frame::response::result::CqlValue::Text(name))) =
                    row.columns.first()
                {
                    columns.push(name.clone());
                }
            }
        }
        Ok(columns)
    }

    fn format_column_value(value: &Option<CqlValue>) -> String {
        match value {
            None => "NULL".to_string(),
            Some(v) => Self::format_cql_value(v),
        }
    }

    fn format_cql_value(v: &CqlValue) -> String {
        match v {
            CqlValue::Ascii(s) | CqlValue::Text(s) => s.clone(),
            CqlValue::Int(i) => i.to_string(),
            CqlValue::BigInt(i) => i.to_string(),
            CqlValue::Float(f) => f.to_string(),
            CqlValue::Double(d) => d.to_string(),
            CqlValue::Boolean(b) => b.to_string(),
            CqlValue::Uuid(u) => u.to_string(),
            CqlValue::Timeuuid(u) => u.to_string(),
            CqlValue::SmallInt(i) => i.to_string(),
            CqlValue::TinyInt(i) => i.to_string(),
            CqlValue::Counter(c) => c.0.to_string(),
            CqlValue::Inet(addr) => addr.to_string(),
            CqlValue::Varint(v) => fmt::varint(v.as_signed_bytes_be_slice()),
            CqlValue::Duration(d) => Self::format_duration(d),
            CqlValue::Timestamp(t) => Self::format_timestamp(t),
            CqlValue::Date(d) => Self::format_date(d),
            CqlValue::Time(t) => Self::format_time(t),
            CqlValue::Decimal(d) => Self::format_decimal(d),
            CqlValue::Blob(b) => fmt::bytes(b, 32),
            CqlValue::Empty => "".to_string(),
            CqlValue::List(l) => {
                let items: Vec<String> = l.iter().map(Self::format_cql_value).collect();
                format!("[{}]", items.join(","))
            }
            CqlValue::Set(s) => {
                let items: Vec<String> = s.iter().map(Self::format_cql_value).collect();
                format!("{{{}}}", items.join(","))
            }
            CqlValue::Map(m) => {
                let entries: Vec<String> = m
                    .iter()
                    .map(|(k, v)| {
                        format!("{}:{}", Self::format_cql_value(k), Self::format_cql_value(v))
                    })
                    .collect();
                format!("{{{}}}", entries.join(","))
            }
            CqlValue::Tuple(t) => {
                let items: Vec<String> = t
                    .iter()
                    .map(|v| match v {
                        Some(val) => Self::format_cql_value(val),
                        None => "NULL".to_string(),
                    })
                    .collect();
                format!("({})", items.join(","))
            }
            _ => format!("{:?}", v),
        }
    }

    fn format_duration(d: &CqlDuration) -> String {
        let mut parts = Vec::new();
        if d.months != 0 {
            parts.push(format!("{}mo", d.months));
        }
        if d.days != 0 {
            parts.push(format!("{}d", d.days));
        }
        if d.nanoseconds != 0 {
            let total_us = d.nanoseconds.unsigned_abs();
            let hours = total_us / 3_600_000_000_000;
            let mins = (total_us % 3_600_000_000_000) / 60_000_000_000;
            let secs = (total_us % 60_000_000_000) / 1_000_000_000;
            let nanos = total_us % 1_000_000_000;
            let neg = if d.nanoseconds < 0 { "-" } else { "" };
            if nanos > 0 {
                parts.push(format!("{}{}h{}m{}s{}ns", neg, hours, mins, secs, nanos));
            } else {
                parts.push(format!("{}{}h{}m{}s", neg, hours, mins, secs));
            }
        }
        if parts.is_empty() {
            "0s".to_string()
        } else {
            parts.join("")
        }
    }

    fn format_timestamp(t: &CqlTimestamp) -> String {
        let millis = t.0;
        let secs = millis / 1000;
        let nsecs = ((millis % 1000) * 1_000_000) as u32;
        DateTime::<Utc>::from_timestamp(secs, nsecs)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
            .unwrap_or_else(|| millis.to_string())
    }

    fn format_date(d: &CqlDate) -> String {
        let days = d.0 as i64 - (1 << 31);
        NaiveDate::from_num_days_from_ce_opt(days as i32 + 719163)
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| d.0.to_string())
    }

    fn format_time(t: &CqlTime) -> String {
        let nanos = t.0;
        let total_secs = nanos / 1_000_000_000;
        let hours = (total_secs / 3600) as u32;
        let mins = ((total_secs % 3600) / 60) as u32;
        let secs = (total_secs % 60) as u32;
        let millis = ((nanos % 1_000_000_000) / 1_000_000) as u32;
        fmt::time_millis(hours, mins, secs, millis)
    }

    fn format_decimal(d: &CqlDecimal) -> String {
        let (bytes, scale) = d.as_signed_be_bytes_slice_and_exponent();
        fmt::decimal_from_bytes(bytes, scale)
    }
}
