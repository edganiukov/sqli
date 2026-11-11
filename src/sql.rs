//! Quoting helpers for names and values spliced into generated SQL.
//!
//! Table, schema and database names come from the server's catalog and can be
//! anything: reserved words, mixed case, spaces, quotes. Generated statements
//! must quote them rather than paste them in as-is.

/// Quote an identifier with `quote` (`"` in standard SQL, `` ` `` in MySQL),
/// doubling any quote character inside it.
pub fn ident(name: &str, quote: char) -> String {
    let mut quoted = String::with_capacity(name.len() + 2);
    quoted.push(quote);
    for ch in name.chars() {
        if ch == quote {
            quoted.push(quote);
        }
        quoted.push(ch);
    }
    quoted.push(quote);
    quoted
}

/// Like [`ident`], for dialects where a backslash escapes the next character
/// inside quoted identifiers (ClickHouse).
pub fn ident_backslash(name: &str, quote: char) -> String {
    let mut quoted = String::with_capacity(name.len() + 2);
    quoted.push(quote);
    for ch in name.chars() {
        if ch == quote || ch == '\\' {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted.push(quote);
    quoted
}

/// A single-quoted string literal with embedded quotes doubled (standard SQL).
pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Like [`literal`], for dialects where a backslash escapes the next character
/// inside strings (MySQL, ClickHouse).
pub fn literal_backslash(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_quoted_and_embedded_quotes_doubled() {
        assert_eq!(ident("order", '"'), "\"order\"");
        assert_eq!(ident("Mixed Case", '"'), "\"Mixed Case\"");
        assert_eq!(ident("we\"ird", '"'), "\"we\"\"ird\"");
        assert_eq!(ident("we`ird", '`'), "`we``ird`");
        // The other dialect's quote character is ordinary text.
        assert_eq!(ident("we\"ird", '`'), "`we\"ird`");
    }

    #[test]
    fn backslash_dialect_escapes_quotes_and_backslashes() {
        assert_eq!(ident_backslash("a`b\\c", '`'), "`a\\`b\\\\c`");
        assert_eq!(literal_backslash("it's a\\b"), "'it\\'s a\\\\b'");
    }

    #[test]
    fn literals_cannot_be_closed_from_inside() {
        assert_eq!(literal("users"), "'users'");
        assert_eq!(literal("x' OR '1'='1"), "'x'' OR ''1''=''1'");
    }
}
