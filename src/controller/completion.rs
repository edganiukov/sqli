use super::{Controller, PopupState};
use crate::completion::{self, CompletionContext, Suggestion};
use crossterm::event::{KeyCode, KeyEvent};
use tui_textarea::CursorMove;

impl Controller {
    pub(super) fn open_completion(&mut self) {
        let query: String = self.query_textarea.lines().join("\n");
        
        // Get cursor position in the full text
        let (row, col) = self.query_textarea.cursor();
        let cursor_pos: usize = self
            .query_textarea
            .lines()
            .iter()
            .take(row)
            .map(|l| l.len() + 1) // +1 for newline
            .sum::<usize>()
            + col;
        
        // Get current word and context
        let (current_word, word_start) = completion::get_current_word(&query, cursor_pos);
        let context = completion::detect_context(&query, cursor_pos);
        
        // Get tables from sidebar
        let tables: Vec<String> = self.current_tab().sidebar.tables.clone();
        
        // Get columns if needed for column context
        let columns: Vec<String> = if let CompletionContext::Column { ref table_or_alias } = context {
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
        let table_name = self.resolve_table_alias(table_or_alias, query)
            .unwrap_or_else(|| table_or_alias.to_string());
        
        // Check if we have columns cached
        if let Some(columns) = self.current_tab().column_cache.get(&table_name) {
            return columns.clone();
        }
        
        // Try to fetch columns from database
        let tab = self.current_tab();
        let db_client = match &tab.db_client {
            Some(client) => client.clone(),
            None => return Vec::new(),
        };
        let current_db = tab.current_database.clone();
        
        let result = self.runtime.block_on(async {
            db_client.list_columns(&table_name, current_db.as_deref()).await
        });
        
        match result {
            Ok(columns) => {
                // Cache the columns
                self.current_tab_mut().column_cache.insert(table_name, columns.clone());
                columns
            }
            Err(_) => Vec::new(),
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
            KeyCode::Down | KeyCode::Char('j') if !key_event.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                self.popup_state = PopupState::Completion {
                    suggestions,
                    selected: (selected + 1).min(max),
                    word_start,
                };
            }
            KeyCode::Up | KeyCode::Char('k') if !key_event.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                self.popup_state = PopupState::Completion {
                    suggestions,
                    selected: selected.saturating_sub(1),
                    word_start,
                };
            }
            KeyCode::Char(' ') if key_event.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                // Ctrl+Space again closes completion
                self.popup_state = PopupState::None;
            }
            KeyCode::Char(_) => {
                // Continue typing - close completion and pass key to textarea
                self.popup_state = PopupState::None;
                self.query_textarea.input(key_event);
            }
            KeyCode::Backspace => {
                // Continue typing - close completion and pass key to textarea
                self.popup_state = PopupState::None;
                self.query_textarea.input(key_event);
            }
            _ => {}
        }
    }
    
    fn apply_completion(&mut self, suggestion: &Suggestion, word_start: usize) {
        // Get current cursor position
        let (row, col) = self.query_textarea.cursor();
        let lines: Vec<&str> = self.query_textarea.lines().iter().map(|s| s.as_str()).collect();
        
        // Calculate position in current line where word starts
        let mut pos = 0usize;
        for (i, line) in lines.iter().enumerate() {
            if i < row {
                pos += line.len() + 1; // +1 for newline
            } else {
                break;
            }
        }
        
        // word_start is global position, convert to position in current line
        let line_start = pos;
        let word_start_in_line = if word_start >= line_start {
            word_start - line_start
        } else {
            // Word spans multiple lines? Shouldn't happen, but handle it
            0
        };
        
        // Select from word start to current position and replace
        // Move cursor to word start
        let chars_to_move_back = col.saturating_sub(word_start_in_line);
        for _ in 0..chars_to_move_back {
            self.query_textarea.move_cursor(CursorMove::Back);
        }
        
        // Select to end of word (current position)
        for _ in 0..chars_to_move_back {
            self.query_textarea.move_cursor(CursorMove::Forward);
        }
        
        // Delete the current word and insert completion
        // First, delete characters from word_start to cursor
        for _ in 0..chars_to_move_back {
            self.query_textarea.move_cursor(CursorMove::Back);
        }
        for _ in 0..chars_to_move_back {
            self.query_textarea.delete_next_char();
        }
        
        // Insert the completion text
        self.query_textarea.insert_str(&suggestion.text);
    }
}
