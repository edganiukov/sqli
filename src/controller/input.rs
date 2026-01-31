use super::{Controller, Focus, Mode, PopupState, ViewState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_textarea::CursorMove;

impl Controller {
    pub fn handle_normal_mode(&mut self, key_event: KeyEvent) {
        // Esc cancels pending operation (if any)
        if key_event.code == KeyCode::Esc && self.cancel_pending_operation() {
            return;
        }

        let view_state = self.current_tab().view_state;
        match view_state {
            ViewState::ConnectionList => self.handle_connection_list_keys(key_event.code),
            ViewState::DatabaseList => self.handle_database_list_keys(key_event.code),
            ViewState::DatabaseView => self.handle_database_view_keys(key_event),
        }
    }

    /// Cancel any pending async operation. Returns true if something was cancelled.
    fn cancel_pending_operation(&mut self) -> bool {
        let Some(op) = self.pending_operation.take() else {
            return false;
        };

        self.pending_escape = false;
        let tab = self.current_tab_mut();
        tab.loading = false;
        tab.pending_g = false;
        tab.status_message = Some("Cancelled".to_string());

        // Reset view state for connection operations
        if matches!(
            op,
            super::PendingOperation::ListDatabases { .. } | super::PendingOperation::Connect { .. }
        ) {
            tab.view_state = ViewState::ConnectionList;
            tab.name = "New".to_string();
            tab.databases.clear();
        }
        true
    }

    fn handle_connection_list_keys(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.current_tab_mut().select_next();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.current_tab_mut().select_previous();
            }
            KeyCode::Char('t') => {
                self.new_tab();
            }
            KeyCode::Enter => {
                self.initiate_connection();
            }
            KeyCode::Esc => {
                // Clear any residual state
                let tab = self.current_tab_mut();
                tab.loading = false;
                tab.status_message = None;
            }
            _ => {}
        }
    }

    fn handle_database_list_keys(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.current_tab_mut().database_next();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.current_tab_mut().database_prev();
            }
            KeyCode::Esc => {
                // Go back to connection list
                let tab = self.current_tab_mut();
                tab.view_state = ViewState::ConnectionList;
                tab.name = "New".to_string();
                tab.databases.clear();
            }
            KeyCode::Enter => {
                self.connect_to_selected_database_from_list();
            }
            _ => {}
        }
    }

    pub(super) fn handle_database_view_keys(&mut self, key_event: KeyEvent) {
        // Handle Ctrl-w + h/j/k/l for pane navigation (vim-style)
        if self.pending_ctrl_w {
            self.pending_ctrl_w = false;
            match key_event.code {
                KeyCode::Char('h') | KeyCode::Left => {
                    self.focus_left();
                    return;
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.focus_right();
                    return;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.focus_up();
                    return;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.focus_down();
                    return;
                }
                KeyCode::Char('w') => {
                    // Ctrl-w w cycles to next pane
                    self.focus_right();
                    return;
                }
                _ => {
                    // Unknown key after Ctrl-w, ignore
                    return;
                }
            }
        }

        // Ctrl+w starts pane navigation sequence
        if key_event.modifiers.contains(KeyModifiers::CONTROL)
            && key_event.code == KeyCode::Char('w')
        {
            self.pending_ctrl_w = true;
            return;
        }

        let focus = self.current_tab().focus;
        match focus {
            Focus::Sidebar => self.handle_sidebar_keys(key_event.code),
            Focus::Query => self.handle_query_keys(key_event),
            Focus::Output => self.handle_output_keys(key_event.code),
        }
    }

    fn handle_sidebar_keys(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.current_tab_mut().sidebar_next();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.current_tab_mut().sidebar_prev();
            }
            KeyCode::Tab => {
                self.current_tab_mut().focus = Focus::Query;
            }
            KeyCode::BackTab => {
                self.current_tab_mut().focus = Focus::Output;
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.current_tab_mut().focus = Focus::Query;
            }
            KeyCode::Enter => {
                self.select_table();
            }
            KeyCode::Char('d') => {
                self.describe_table();
            }
            KeyCode::Char('r') => {
                self.refresh_tables();
            }
            KeyCode::F(5) => {
                self.execute_query();
            }
            _ => {}
        }
    }

    fn handle_query_keys(&mut self, key_event: KeyEvent) {
        // Handle popup state first
        if !matches!(self.popup_state, PopupState::None) {
            self.handle_popup_keys(key_event);
            return;
        }

        // Handle Esc + : for command mode
        if self.pending_escape {
            self.pending_escape = false;
            if key_event.code == KeyCode::Char(':') {
                self.mode = Mode::Command;
                self.command_buffer.clear();
                return;
            }
            // Any other key after Esc - pass through
        }

        // Esc sets pending_escape flag
        if key_event.code == KeyCode::Esc {
            self.pending_escape = true;
            return;
        }

        // Check for execute shortcut
        if key_event.code == KeyCode::F(5) {
            self.execute_query();
            return;
        }

        // Ctrl shortcuts
        if key_event.modifiers.contains(KeyModifiers::CONTROL) {
            match key_event.code {
                KeyCode::Char('r') => self.execute_query(),
                KeyCode::Char('o') => self.open_template_popup(),
                KeyCode::Char('s') => self.open_save_template_popup(),
                KeyCode::Char('g') => self.edit_query_in_editor(),
                KeyCode::Char('k') => {
                    self.query_textarea.delete_line_by_end();
                }
                KeyCode::Left => {
                    self.query_textarea.move_cursor(CursorMove::WordBack);
                }
                KeyCode::Right => {
                    self.query_textarea.move_cursor(CursorMove::WordForward);
                }
                _ => {}
            }
            return;
        }

        // Focus navigation
        match key_event.code {
            KeyCode::Tab => {
                self.current_tab_mut().focus = Focus::Output;
                return;
            }
            KeyCode::BackTab => {
                self.focus_sidebar();
                return;
            }
            _ => {}
        }

        // All other keys go to textarea
        self.query_textarea.input(key_event);
    }

    fn handle_output_keys(&mut self, key_code: KeyCode) {
        // Handle record detail popup first
        if let PopupState::RecordDetail {
            row_index,
            selected_field,
            scroll,
        } = &self.popup_state
        {
            // Get max field count from current query result
            let max_field = self
                .current_tab()
                .query_result
                .as_ref()
                .map(|r| match r {
                    crate::db::QueryResult::Select { columns, .. } => {
                        columns.len().saturating_sub(1)
                    }
                    _ => 0,
                })
                .unwrap_or(0);

            let row_index = *row_index;
            let mut new_selected = *selected_field;
            let scroll = *scroll;

            match key_code {
                KeyCode::Esc => {
                    self.popup_state = PopupState::None;
                    return;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    new_selected = (new_selected + 1).min(max_field);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    new_selected = new_selected.saturating_sub(1);
                }
                KeyCode::Char('g') => {
                    new_selected = 0;
                }
                KeyCode::Char('G') => {
                    new_selected = max_field;
                }
                KeyCode::PageDown => {
                    new_selected = (new_selected + 10).min(max_field);
                }
                KeyCode::PageUp => {
                    new_selected = new_selected.saturating_sub(10);
                }
                _ => {}
            }

            self.popup_state = PopupState::RecordDetail {
                row_index,
                selected_field: new_selected,
                scroll,
            };
            return;
        }

        // Use a reasonable default visible height (will be adjusted by scroll logic)
        const VISIBLE_HEIGHT: usize = 20;

        let tab = self.current_tab_mut();

        // Handle gg (scroll to top)
        if tab.pending_g {
            tab.pending_g = false;
            if key_code == KeyCode::Char('g') {
                self.scroll_to_start();
                return;
            }
        }

        match key_code {
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            KeyCode::Tab => {
                self.focus_sidebar();
            }
            KeyCode::BackTab => {
                self.current_tab_mut().focus = Focus::Query;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_cursor(1, VISIBLE_HEIGHT);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_cursor(-1, VISIBLE_HEIGHT);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.move_column(-1);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.move_column(1);
            }
            KeyCode::PageDown => {
                self.move_cursor(10, VISIBLE_HEIGHT);
            }
            KeyCode::PageUp => {
                self.move_cursor(-10, VISIBLE_HEIGHT);
            }
            KeyCode::Char('^') => {
                self.current_tab_mut().result_selected_col = 0;
                self.current_tab_mut().result_h_scroll = 0;
            }
            KeyCode::Char('$') => {
                self.move_column_to_end();
            }
            KeyCode::Char('g') => {
                self.current_tab_mut().pending_g = true;
            }
            KeyCode::Char('G') => {
                self.scroll_to_end();
            }
            KeyCode::Enter => {
                self.open_record_detail();
            }
            KeyCode::F(5) => {
                self.execute_query();
            }
            _ => {}
        }
    }

    pub fn handle_command_mode(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.command_buffer.clear();
            }
            KeyCode::Enter => {
                self.exec_command();
                self.mode = Mode::Normal;
                self.command_buffer.clear();
            }
            KeyCode::Backspace => {
                self.command_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.command_buffer.push(c);
            }
            _ => {}
        }
    }

    fn exec_command(&mut self) {
        match self.command_buffer.as_str() {
            "q" | "quit" => self.close_current_tab(),
            "qa" | "q!" | "quitall" => self.quit = true,
            "next" => self.next_tab(),
            "prev" => self.previous_tab(),
            "new" => self.new_tab(),
            "sysdb" => self.toggle_system_databases(),
            "help" | "h" => self.show_help(),
            cmd => {
                self.current_tab_mut().status_message = Some(format!("Unknown command: {}", cmd));
            }
        }
    }

    fn show_help(&mut self) {
        let help = ":q quit | F5/Ctrl+R exec | Ctrl+O templates | Ctrl+S save | Ctrl+G editor | Ctrl+w hjkl nav";
        self.current_tab_mut().status_message = Some(help.to_string());
    }

    fn toggle_system_databases(&mut self) {
        let tab = self.current_tab_mut();
        if tab.view_state != ViewState::DatabaseList {
            return;
        }

        tab.show_system_databases = !tab.show_system_databases;
        let show = tab.show_system_databases;

        self.refresh_database_list();
        let msg = if show {
            "Showing system databases"
        } else {
            "Hiding system databases"
        };
        self.current_tab_mut().status_message = Some(msg.to_string());
    }
}
