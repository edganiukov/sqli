/// SQL autocompletion support

#[derive(Debug, Clone, PartialEq)]
pub enum CompletionContext {
    /// After SELECT, WHERE, ORDER BY, etc. - suggest columns and keywords
    General,
    /// After FROM, JOIN, INTO, UPDATE - suggest tables
    Table,
    /// After a table alias and dot (e.g., "u.") - suggest columns for that table
    Column { table_or_alias: String },
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    pub text: String,
    pub kind: SuggestionKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SuggestionKind {
    Keyword,
    Table,
    Column,
}

const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "IN", "IS", "NULL",
    "LIKE", "ILIKE", "BETWEEN", "EXISTS", "CASE", "WHEN", "THEN", "ELSE", "END",
    "AS", "ON", "USING", "JOIN", "INNER", "LEFT", "RIGHT", "FULL", "OUTER", "CROSS",
    "ORDER", "BY", "ASC", "DESC", "NULLS", "FIRST", "LAST",
    "GROUP", "HAVING", "LIMIT", "OFFSET", "FETCH", "NEXT", "ROWS", "ONLY",
    "UNION", "INTERSECT", "EXCEPT", "ALL", "DISTINCT",
    "INSERT", "INTO", "VALUES", "DEFAULT", "RETURNING",
    "UPDATE", "SET",
    "DELETE", "TRUNCATE",
    "CREATE", "ALTER", "DROP", "TABLE", "INDEX", "VIEW", "SCHEMA", "DATABASE",
    "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "UNIQUE", "CHECK", "CONSTRAINT",
    "TRUE", "FALSE",
    "COUNT", "SUM", "AVG", "MIN", "MAX", "COALESCE", "NULLIF", "CAST",
    "WITH", "RECURSIVE",
];

/// Keywords that should be followed by table names
const TABLE_KEYWORDS: &[&str] = &[
    "FROM", "JOIN", "INNER JOIN", "LEFT JOIN", "RIGHT JOIN", "FULL JOIN",
    "CROSS JOIN", "INTO", "UPDATE", "TABLE", "TRUNCATE",
];

/// Get the word being typed at cursor position
pub fn get_current_word(text: &str, cursor_pos: usize) -> (String, usize) {
    let before_cursor = &text[..cursor_pos.min(text.len())];
    
    // Find start of current word (alphanumeric or underscore)
    let word_start = before_cursor
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);
    
    let word = &before_cursor[word_start..];
    (word.to_string(), word_start)
}

/// Detect completion context based on text before cursor
pub fn detect_context(text: &str, cursor_pos: usize) -> CompletionContext {
    let before_cursor = &text[..cursor_pos.min(text.len())];
    let before_lower = before_cursor.to_lowercase();
    
    // Check for table.column pattern (e.g., "users." or "u.")
    if let Some(dot_pos) = before_cursor.rfind('.') {
        let after_dot = &before_cursor[dot_pos + 1..];
        // Only if we're right after the dot or typing column name
        if after_dot.chars().all(|c| c.is_alphanumeric() || c == '_') {
            // Find the table/alias before the dot
            let before_dot = &before_cursor[..dot_pos];
            let table_start = before_dot
                .rfind(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|i| i + 1)
                .unwrap_or(0);
            let table_or_alias = before_dot[table_start..].to_string();
            if !table_or_alias.is_empty() {
                return CompletionContext::Column { table_or_alias };
            }
        }
    }
    
    // Check if we're after a table keyword
    for kw in TABLE_KEYWORDS {
        let kw_lower = kw.to_lowercase();
        // Look for keyword followed by optional whitespace at end
        if let Some(pos) = before_lower.rfind(&kw_lower) {
            let after_kw = &before_cursor[pos + kw.len()..];
            // If only whitespace and maybe partial word after keyword
            if after_kw.trim_start().chars().all(|c| c.is_alphanumeric() || c == '_') {
                return CompletionContext::Table;
            }
        }
    }
    
    CompletionContext::General
}

/// Generate completion suggestions
pub fn get_suggestions(
    context: &CompletionContext,
    prefix: &str,
    tables: &[String],
    columns: &[String],
) -> Vec<Suggestion> {
    let prefix_lower = prefix.to_lowercase();
    let mut suggestions = Vec::new();
    
    match context {
        CompletionContext::General => {
            // Suggest keywords that match prefix
            for &kw in SQL_KEYWORDS {
                if kw.to_lowercase().starts_with(&prefix_lower) {
                    suggestions.push(Suggestion {
                        text: kw.to_string(),
                        kind: SuggestionKind::Keyword,
                    });
                }
            }
            // Also suggest tables (might be typing table name in SELECT)
            for table in tables {
                if table.to_lowercase().starts_with(&prefix_lower) {
                    suggestions.push(Suggestion {
                        text: table.clone(),
                        kind: SuggestionKind::Table,
                    });
                }
            }
        }
        CompletionContext::Table => {
            // Suggest tables
            for table in tables {
                if table.to_lowercase().starts_with(&prefix_lower) {
                    suggestions.push(Suggestion {
                        text: table.clone(),
                        kind: SuggestionKind::Table,
                    });
                }
            }
        }
        CompletionContext::Column { .. } => {
            // Suggest columns for the table
            for col in columns {
                if col.to_lowercase().starts_with(&prefix_lower) {
                    suggestions.push(Suggestion {
                        text: col.clone(),
                        kind: SuggestionKind::Column,
                    });
                }
            }
        }
    }
    
    // Sort: exact prefix matches first, then alphabetically
    suggestions.sort_by(|a, b| {
        let a_exact = a.text.to_lowercase().starts_with(&prefix_lower);
        let b_exact = b.text.to_lowercase().starts_with(&prefix_lower);
        match (a_exact, b_exact) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.text.cmp(&b.text),
        }
    });
    
    // Limit results
    suggestions.truncate(15);
    suggestions
}
