use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent};

use super::{Controller, PopupState};
use crate::completion::{self, CompletionContext, Suggestion};

/// Upper bound on the synchronous column fetch behind Ctrl+Space.
const COLUMN_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);

impl Controller {
    pub(super) fn open_completion(&mut self) {
        let query: String = self.current_tab().query_textarea.lines().join("\n");

        // Get cursor position in the full text
        let (row, col) = self.current_tab().query_textarea.cursor();
        let cursor_pos =
            completion::cursor_offset(self.current_tab().query_textarea.lines(), row, col);

        // Get current word and context
        let (current_word, word_start) = completion::get_current_word(&query, cursor_pos);
        let context = completion::detect_context(&query, cursor_pos);

        // Get tables from sidebar
        let tables: Vec<String> = self.current_tab().sidebar.tables.clone();

        // Get columns if needed for column context
        let columns: Vec<String> = if let CompletionContext::Column { ref table_or_alias } = context
        {
            self.get_columns_for_completion(table_or_alias, &query)
        } else {
            Vec::new()
        };

        // Get suggestions
        let suggestions = completion::get_suggestions(&context, &current_word, &tables, &columns);

        if suggestions.is_empty() {
            self.current_tab_mut().status_message = Some("No completions available".to_string());
            return;
        }

        self.popup_state = PopupState::Completion {
            suggestions,
            selected: 0,
            word_start,
        };
    }

    /// Get columns for a table or alias, using cache or fetching from DB
    fn get_columns_for_completion(&mut self, table_or_alias: &str, query: &str) -> Vec<String> {
        // First, try to resolve alias to actual table name
        let table_name = self
            .resolve_table_alias(table_or_alias, query)
            .unwrap_or_else(|| table_or_alias.to_string());

        // Check if we have columns cached
        if let Some(columns) = self.current_tab().column_cache.get(&table_name) {
            return columns.clone();
        }

        // Try to fetch columns from database
        let tab = self.current_tab();
        let Some(session) = tab.session.clone() else {
            return Vec::new();
        };
        let current_db = tab.current_database.clone();

        // This blocks the UI thread, so it must stay short: a busy or dead
        // connection just means no column suggestions this time.
        let lookup = session.run(|client| {
            let table_name = table_name.clone();
            async move {
                client
                    .list_columns(&table_name, current_db.as_deref())
                    .await
            }
        });
        let result = self
            .runtime
            .block_on(async { tokio::time::timeout(COLUMN_LOOKUP_TIMEOUT, lookup).await });

        match result {
            Ok(Ok(columns)) => {
                // Cache the columns
                self.current_tab_mut()
                    .column_cache
                    .insert(table_name, columns.clone());
                columns
            }
            _ => Vec::new(),
        }
    }

    /// Try to resolve a table alias to the actual table name by parsing the query
    fn resolve_table_alias(&self, alias: &str, query: &str) -> Option<String> {
        let query_lower = query.to_lowercase();
        let alias_lower = alias.to_lowercase();

        // Look for patterns like "FROM table_name alias" or "FROM table_name AS alias"
        // or "JOIN table_name alias" or "JOIN table_name AS alias"
        let tables = &self.current_tab().sidebar.tables;

        for table in tables {
            let table_lower = table.to_lowercase();

            // Pattern: "table alias" or "table AS alias"
            let pattern1 = format!("{} {}", table_lower, alias_lower);
            let pattern2 = format!("{} as {}", table_lower, alias_lower);

            if query_lower.contains(&pattern1) || query_lower.contains(&pattern2) {
                return Some(table.clone());
            }
        }

        // If alias matches a table name directly, use it
        if tables.iter().any(|t| t.to_lowercase() == alias_lower) {
            return Some(alias.to_string());
        }

        None
    }

    pub(super) fn handle_completion_keys(
        &mut self,
        key_event: KeyEvent,
        suggestions: Vec<Suggestion>,
        selected: usize,
        word_start: usize,
    ) {
        let max = suggestions.len().saturating_sub(1);

        match key_event.code {
            KeyCode::Esc => {
                self.popup_state = PopupState::None;
            }
            KeyCode::Enter | KeyCode::Tab => {
                // Apply the selected completion
                if let Some(suggestion) = suggestions.get(selected) {
                    self.apply_completion(suggestion, word_start);
                }
                self.popup_state = PopupState::None;
            }
            KeyCode::Down | KeyCode::Char('j')
                if !key_event
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.popup_state = PopupState::Completion {
                    suggestions,
                    selected: (selected + 1).min(max),
                    word_start,
                };
            }
            KeyCode::Up | KeyCode::Char('k')
                if !key_event
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.popup_state = PopupState::Completion {
                    suggestions,
                    selected: selected.saturating_sub(1),
                    word_start,
                };
            }
            KeyCode::Char(' ')
                if key_event
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                // Ctrl+Space again closes completion
                self.popup_state = PopupState::None;
            }
            KeyCode::Char(_) => {
                // Continue typing - close completion and pass key to textarea
                self.popup_state = PopupState::None;
                self.current_tab_mut().query_textarea.input(key_event);
            }
            KeyCode::Backspace => {
                // Continue typing - close completion and pass key to textarea
                self.popup_state = PopupState::None;
                self.current_tab_mut().query_textarea.input(key_event);
            }
            _ => {}
        }
    }

    /// Replace the word being typed (from byte offset `word_start` of the
    /// query up to the cursor) with the chosen suggestion.
    fn apply_completion(&mut self, suggestion: &Suggestion, word_start: usize) {
        let tab = self.current_tab_mut();
        let (row, col) = tab.query_textarea.cursor();
        let lines = tab.query_textarea.lines();

        // The word never spans lines, so work within the cursor's line. The
        // editor deletes by character, so count characters, not bytes.
        let line = lines.get(row).map(String::as_str).unwrap_or("");
        let line_start = completion::cursor_offset(lines, row, 0);
        let cursor_in_line = completion::cursor_offset(lines, row, col) - line_start;
        let word_in_line = word_start.saturating_sub(line_start).min(cursor_in_line);
        let typed_chars = line
            .get(word_in_line..cursor_in_line)
            .map_or(0, |typed| typed.chars().count());

        for _ in 0..typed_chars {
            tab.query_textarea.delete_char();
        }
        tab.query_textarea.insert_str(&suggestion.text);
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::controller::{Focus, ViewState};

    fn editing(query: &str, tables: &[&str]) -> Controller {
        let mut controller = Controller::with_connections(Vec::new(), None);
        let tab = controller.current_tab_mut();
        tab.view_state = ViewState::DatabaseView;
        tab.focus = Focus::Query;
        tab.sidebar.tables = tables.iter().map(|t| t.to_string()).collect();
        tab.query_textarea.insert_str(query);
        controller
    }

    fn query(controller: &Controller) -> String {
        controller.current_tab().query_textarea.lines().join("\n")
    }

    #[test]
    fn completes_the_word_after_multibyte_text() {
        let mut controller = editing("sélect 'ü' from us", &["users", "orders"]);

        controller.open_completion();
        let PopupState::Completion { suggestions, .. } = &controller.popup_state else {
            panic!("expected the completion popup");
        };
        assert_eq!(suggestions[0].text, "users");

        controller.handle_normal_mode(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(query(&controller), "sélect 'ü' from users");
    }

    #[test]
    fn completes_on_a_later_line() {
        let mut controller = editing("select *\nfrom tablé t\njoin use", &["users"]);

        controller.open_completion();
        controller.handle_normal_mode(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(query(&controller), "select *\nfrom tablé t\njoin users");
    }

    /// Used to panic: the cursor column (characters) was used as a byte offset.
    #[test]
    fn completion_right_after_a_multibyte_character_does_not_panic() {
        let mut controller = editing("é", &["users"]);
        controller.open_completion();
    }
}
