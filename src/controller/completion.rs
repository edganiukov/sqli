use super::{Controller, PopupState};
use crate::completion::{self, Suggestion};
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
        
        // Get suggestions
        let suggestions = completion::get_suggestions(&context, &current_word, &tables, &[]);
        
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
