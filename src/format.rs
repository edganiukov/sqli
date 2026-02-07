//! Common value formatting utilities for database clients.
//!
//! Provides consistent formatting of SQL types across all database backends.

#![allow(dead_code)] // Utility functions may not all be used yet

/// Format an optional value, returning "NULL" for None.
#[inline]
pub fn null_or<T: ToString>(value: Option<T>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "NULL".to_string())
}

/// Format an optional value with a custom formatter.
#[inline]
pub fn null_or_else<T, F>(value: Option<T>, f: F) -> String
where
    F: FnOnce(T) -> String,
{
    value.map(f).unwrap_or_else(|| "NULL".to_string())
}

/// Format a date as YYYY-MM-DD.
#[inline]
pub fn date(year: i32, month: u32, day: u32) -> String {
    format!("{:04}-{:02}-{:02}", year, month, day)
}

/// Format a time as HH:MM:SS.
#[inline]
pub fn time(hour: u32, min: u32, sec: u32) -> String {
    format!("{:02}:{:02}:{:02}", hour, min, sec)
}

/// Format a time with milliseconds as HH:MM:SS.mmm.
#[inline]
pub fn time_millis(hour: u32, min: u32, sec: u32, millis: u32) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", hour, min, sec, millis)
}

/// Format a datetime as YYYY-MM-DD HH:MM:SS.
#[inline]
pub fn datetime(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hour, min, sec
    )
}

/// Format a datetime with milliseconds as YYYY-MM-DD HH:MM:SS.mmm.
#[inline]
pub fn datetime_millis(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
    millis: u32,
) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        year, month, day, hour, min, sec, millis
    )
}

/// Format a decimal value given its internal representation and scale.
///
/// # Arguments
/// * `value` - The unscaled integer value
/// * `scale` - Number of decimal places
///
/// # Examples
/// ```
/// assert_eq!(decimal(12345, 2), "123.45");
/// assert_eq!(decimal(-12345, 2), "-123.45");
/// assert_eq!(decimal(5, 3), "0.005");
/// ```
pub fn decimal(value: i64, scale: u32) -> String {
    if scale == 0 {
        return value.to_string();
    }

    let negative = value < 0;
    let abs_value = value.unsigned_abs();
    let divisor = 10u64.pow(scale);
    let whole = abs_value / divisor;
    let frac = abs_value % divisor;

    if negative {
        format!("-{}.{:0>width$}", whole, frac, width = scale as usize)
    } else {
        format!("{}.{:0>width$}", whole, frac, width = scale as usize)
    }
}

/// Format a decimal from BigInt bytes and scale (for Cassandra).
pub fn decimal_from_bytes(bytes: &[u8], scale: i32) -> String {
    use num_bigint::BigInt;

    let bigint = BigInt::from_signed_bytes_be(bytes);
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

/// Format bytes as hex with \x prefix, or as size if too large.
pub fn bytes(data: &[u8], max_display: usize) -> String {
    if data.len() <= max_display {
        format!("\\x{}", hex::encode(data))
    } else {
        format!("<{} bytes>", data.len())
    }
}

/// Format a collection with item count.
pub fn collection(kind: &str, len: usize) -> String {
    format!("<{}: {} items>", kind, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_or() {
        assert_eq!(null_or(Some(42)), "42");
        assert_eq!(null_or::<i32>(None), "NULL");
        assert_eq!(null_or(Some("hello")), "hello");
    }

    #[test]
    fn test_date() {
        assert_eq!(date(2024, 1, 15), "2024-01-15");
        assert_eq!(date(2024, 12, 31), "2024-12-31");
    }

    #[test]
    fn test_datetime() {
        assert_eq!(datetime(2024, 1, 15, 14, 30, 45), "2024-01-15 14:30:45");
    }

    #[test]
    fn test_decimal() {
        assert_eq!(decimal(12345, 2), "123.45");
        assert_eq!(decimal(-12345, 2), "-123.45");
        assert_eq!(decimal(5, 3), "0.005");
        assert_eq!(decimal(1000, 0), "1000");
        assert_eq!(decimal(-5, 3), "-0.005");
    }

    #[test]
    fn test_bytes() {
        assert_eq!(bytes(&[0xDE, 0xAD], 32), "\\xdead");
        assert_eq!(bytes(&[0; 100], 32), "<100 bytes>");
    }
}
