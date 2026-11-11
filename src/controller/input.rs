use super::{Controller, Focus, Mode, PopupState, ViewState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tui_textarea::CursorMove;

impl Controller {
    pub fn handle_normal_mode(&mut self, key_event: KeyEvent) {
        // Popups are modal: while one is open it gets every key, whichever
        // pane has focus. Otherwise a popup could be left open behind a focus
        // or tab change, where it swallows input without being visible.
        match self.popup_state {
            PopupState::None => {}
            PopupState::Help { .. } => return self.handle_help_popup_keys(key_event.code),
            PopupState::RecordDetail { .. } => {
                return self.handle_record_detail_keys(key_event.code);
            }
            _ => return self.handle_popup_keys(key_event),
        }

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

    fn handle_help_popup_keys(&mut self, key_code: KeyCode) {
        let PopupState::Help { scroll } = self.popup_state else {
            return;
        };

        // Total help lines (must match the entries count in popups::help_lines)
        const HELP_TOTAL_LINES: usize = 57;

        match key_code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                self.popup_state = PopupState::None;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.popup_state = PopupState::Help {
                    scroll: scroll.saturating_add(1).min(HELP_TOTAL_LINES),
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.popup_state = PopupState::Help {
                    scroll: scroll.saturating_sub(1),
                };
            }
            KeyCode::PageDown => {
                self.popup_state = PopupState::Help {
                    scroll: scroll.saturating_add(10).min(HELP_TOTAL_LINES),
                };
            }
            KeyCode::PageUp => {
                self.popup_state = PopupState::Help {
                    scroll: scroll.saturating_sub(10),
                };
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.popup_state = PopupState::Help { scroll: 0 };
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.popup_state = PopupState::Help {
                    scroll: HELP_TOTAL_LINES,
                };
            }
            _ => {}
        }
    }

    fn open_help_popup(&mut self) {
        self.popup_state = PopupState::Help { scroll: 0 };
    }

    /// Cancel any pending async operation on the current tab. Returns true if something was cancelled.
    fn cancel_pending_operation(&mut self) -> bool {
        let tab = self.current_tab_mut();
        let Some(op) = tab.pending_operation.take() else {
            return false;
        };

        tab.loading = false;
        tab.pending_g = false;

        // Abort the spawned task so a long-running query stops instead of
        // running to completion in the background after the UI moves on.
        let mut cancel_on_server = None;
        if let super::PendingOperation::Query { handle, .. } = &op {
            handle.abort();
            tab.status_message = Some("Query cancelled".to_string());
            cancel_on_server = tab.session.clone();
        } else {
            tab.status_message = Some("Cancelled".to_string());
        }

        // Reset view state for connection operations
        if matches!(
            op,
            super::PendingOperation::ListDatabases { .. } | super::PendingOperation::Connect { .. }
        ) {
            if tab.session.is_some() {
                // Already connected - return to database view
                tab.view_state = ViewState::DatabaseView;
            } else {
                tab.view_state = ViewState::ConnectionList;
                tab.name = "new".to_string();
            }
            tab.databases.clear();
        }

        // Aborting the task only stops waiting for the answer. The server
        // keeps executing (and keeps the connection busy) until told to stop.
        if let Some(session) = cancel_on_server {
            self.runtime.spawn(async move {
                if let Err(e) = session.client().cancel_running().await {
                    crate::debug_log!("Failed to cancel query on the server: {}", e);
                }
            });
        }

        self.pending_escape = false;
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
            KeyCode::Char('h') | KeyCode::Left => {
                self.current_tab_mut().prev_group();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.current_tab_mut().next_group();
            }
            KeyCode::Char('t') => {
                self.new_tab();
            }
            KeyCode::Char('?') => {
                self.open_help_popup();
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
            KeyCode::Char('?') => {
                self.open_help_popup();
            }
            KeyCode::Esc => {
                let tab = self.current_tab_mut();
                if tab.session.is_some() {
                    // Already connected - go back to database view
                    tab.view_state = ViewState::DatabaseView;
                    tab.databases.clear();
                } else {
                    // Not connected - go back to connection list
                    tab.view_state = ViewState::ConnectionList;
                    tab.name = "new".to_string();
                    tab.databases.clear();
                }
            }
            KeyCode::Enter => {
                self.connect_to_selected_database_from_list();
            }
            _ => {}
        }
    }

    pub(super) fn handle_database_view_keys(&mut self, key_event: KeyEvent) {
        // Handle Ctrl-w + key for pane/tab navigation (vim-style)
        if self.pending_ctrl_w {
            self.pending_ctrl_w = false;
            match key_event.code {
                // Pane navigation
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
                // Tab navigation
                KeyCode::Char('n') => {
                    self.next_tab();
                    return;
                }
                KeyCode::Char('p') => {
                    self.previous_tab();
                    return;
                }
                KeyCode::Char(c @ '1'..='9') => {
                    let target_id = c.to_digit(10).unwrap() as usize;
                    self.switch_to_tab_by_id(target_id);
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

        // Ctrl+B toggles sidebar visibility
        if key_event.modifiers.contains(KeyModifiers::CONTROL)
            && key_event.code == KeyCode::Char('b')
        {
            self.toggle_sidebar();
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
            KeyCode::Char('?') => {
                self.open_help_popup();
            }
            KeyCode::F(5) => {
                self.execute_query();
            }
            _ => {}
        }
    }

    fn handle_query_keys(&mut self, key_event: KeyEvent) {
        // Handle Esc + : for command mode, Esc + ? for help
        if self.pending_escape {
            self.pending_escape = false;
            match key_event.code {
                KeyCode::Char(':') => {
                    self.mode = Mode::Command;
                    self.command_buffer.clear();
                    return;
                }
                KeyCode::Char('?') => {
                    self.open_help_popup();
                    return;
                }
                // Any other key after Esc - pass through
                _ => {}
            }
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
                KeyCode::Char('e') => {
                    self.execute_query();
                    return;
                }
                KeyCode::Char('r') => {
                    self.current_tab_mut().query_textarea.redo();
                    return;
                }
                KeyCode::Char('o') => {
                    self.open_template_popup();
                    return;
                }
                KeyCode::Char('s') => {
                    self.open_save_template_popup();
                    return;
                }
                KeyCode::Char('g') => {
                    self.edit_query_in_editor();
                    return;
                }
                KeyCode::Char(' ') => {
                    self.open_completion();
                    return;
                }
                KeyCode::Char('k') => {
                    self.current_tab_mut().query_textarea.delete_line_by_end();
                    return;
                }
                KeyCode::Char('z') => {
                    self.current_tab_mut().query_textarea.undo();
                    return;
                }
                KeyCode::Char(';') => {
                    self.mode = Mode::Command;
                    self.command_buffer.clear();
                    return;
                }

                KeyCode::Left => {
                    self.current_tab_mut()
                        .query_textarea
                        .move_cursor(CursorMove::WordBack);
                    return;
                }
                KeyCode::Right => {
                    self.current_tab_mut()
                        .query_textarea
                        .move_cursor(CursorMove::WordForward);
                    return;
                }
                _ => {
                    // Pass unhandled Ctrl combos (e.g. Ctrl+J from tmux paste)
                    // through to the textarea
                }
            }
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
        self.current_tab_mut().query_textarea.input(key_event);
    }

    fn handle_record_detail_keys(&mut self, key_code: KeyCode) {
        let PopupState::RecordDetail {
            row_index,
            selected_field,
            scroll,
        } = self.popup_state
        else {
            return;
        };

        // Get max field count from current query result
        let max_field = match &self.current_tab().query_result {
            Some(crate::db::QueryResult::Select { columns, .. }) => columns.len().saturating_sub(1),
            _ => 0,
        };

        let mut new_selected = selected_field;
        let mut new_scroll = scroll;

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
            KeyCode::Char('y') => {
                // Copy the selected field value to clipboard
                if let Some(crate::db::QueryResult::Select { rows, .. }) =
                    &self.current_tab().query_result
                    && let Some(row) = rows.get(row_index)
                {
                    let value = row.get(selected_field).cloned().unwrap_or_default();
                    self.copy_to_clipboard(&value, 1, "value");
                }
                return;
            }
            _ => {}
        }

        // Update scroll to keep selected field visible
        // Estimate ~2 lines per field (name + value + spacing)
        let term_height = crossterm::terminal::size().map(|(_, h)| h).unwrap_or(24);
        let popup_height = (term_height as f32 * 0.8) as usize;
        let visible_fields = popup_height.saturating_sub(4) / 2; // rough estimate
        if new_selected < new_scroll {
            new_scroll = new_selected;
        } else if new_selected >= new_scroll + visible_fields {
            new_scroll = new_selected.saturating_sub(visible_fields / 2);
        }

        self.popup_state = PopupState::RecordDetail {
            row_index,
            selected_field: new_selected,
            scroll: new_scroll,
        };
    }

    fn handle_output_keys(&mut self, key_code: KeyCode) {
        // Visible data rows from the last rendered output geometry.
        let visible_height = self.output_visible_rows();

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
            KeyCode::Char('?') => {
                self.open_help_popup();
            }
            KeyCode::Tab => {
                self.focus_sidebar();
            }
            KeyCode::BackTab => {
                self.current_tab_mut().focus = Focus::Query;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_cursor(1, visible_height);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_cursor(-1, visible_height);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.move_column(-1);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.move_column(1);
            }
            KeyCode::PageDown => {
                self.move_cursor(10, visible_height);
            }
            KeyCode::PageUp => {
                self.move_cursor(-10, visible_height);
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
            KeyCode::Char('v') => {
                self.toggle_visual_select_cell();
            }
            KeyCode::Char('V') => {
                self.toggle_visual_select_line();
            }
            KeyCode::Char('y') => {
                self.yank_selected_rows();
            }
            KeyCode::Esc => {
                let tab = self.current_tab_mut();
                if tab.visual_select.is_some() {
                    tab.visual_select = None;
                }
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

    pub fn handle_paste(&mut self, text: String) {
        let view_state = self.current_tab().view_state;
        if view_state == ViewState::DatabaseView && self.current_tab().focus == Focus::Query {
            self.current_tab_mut().query_textarea.insert_str(&text);
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
            "db" => self.open_database_select(),
            "system" => self.toggle_system_databases(),
            "help" | "h" => self.open_help_popup(),
            cmd => {
                self.current_tab_mut().status_message = Some(format!("Unknown command: {}", cmd));
            }
        }
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

    pub fn handle_mouse(&mut self, event: MouseEvent) {
        // While a popup is open it stays keyboard-driven, but the wheel still scrolls
        // it (mirroring the arrow keys) so scrolling works the same way everywhere.
        if !matches!(self.popup_state, PopupState::None) {
            match event.kind {
                MouseEventKind::ScrollUp => self.scroll_active_popup(true),
                MouseEventKind::ScrollDown => self.scroll_active_popup(false),
                _ => {}
            }
            return;
        }

        let x = event.column;
        let y = event.row;

        // Handle scroll events
        if matches!(
            event.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let scroll_up = matches!(event.kind, MouseEventKind::ScrollUp);
            self.handle_mouse_scroll(x, y, scroll_up);
            return;
        }

        // Horizontal wheel / trackpad pans the wide result table.
        if matches!(
            event.kind,
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight
        ) {
            let scroll_right = matches!(event.kind, MouseEventKind::ScrollRight);
            self.handle_mouse_hscroll(x, y, scroll_right);
            return;
        }

        // Continue or finish an in-progress column resize drag.
        if self.active_resize.is_some() {
            match event.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    self.update_column_resize(x);
                    return;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.active_resize = None;
                    return;
                }
                // A press or a plain move while "dragging" means the release
                // happened outside the window and was never delivered.
                MouseEventKind::Down(_) | MouseEventKind::Moved => self.active_resize = None,
                _ => {}
            }
        }

        // Only handle left button clicks for the rest
        if !matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }

        // Grabbing a result column's header border starts a resize instead of a click.
        if self.try_start_column_resize(x, y) {
            return;
        }

        // Detect double-click (within 400ms and same position)
        const DOUBLE_CLICK_MS: u128 = 400;
        let is_double_click = self
            .last_click
            .map(|(t, lx, ly)| t.elapsed().as_millis() < DOUBLE_CLICK_MS && lx == x && ly == y)
            .unwrap_or(false);
        self.last_click = Some((std::time::Instant::now(), x, y));

        // Tab bar
        if let Some(index) = self
            .tab_rects
            .iter()
            .position(|rect| rect_contains(*rect, x, y))
        {
            self.set_current_tab(index);
            return;
        }

        match self.current_tab().view_state {
            ViewState::ConnectionList | ViewState::DatabaseList => {
                self.handle_mouse_picker(x, y, is_double_click);
            }
            ViewState::DatabaseView => {
                self.handle_mouse_database_view(x, y, is_double_click);
            }
        }
    }

    /// If the cursor sits on a result column's header border, begin a resize drag.
    /// Returns true when a resize was started. Only the header row acts as a handle,
    /// so dragging never interferes with row/cell selection in the data area.
    fn try_start_column_resize(&mut self, x: u16, y: u16) -> bool {
        if self.current_tab().view_state != ViewState::DatabaseView {
            return false;
        }
        let Some(table) = self.view_layout.output_table else {
            return false;
        };
        if y != table.y {
            return false;
        }

        let grabbed = self.result_columns.iter().copied().find(|seg| {
            let right = seg.right_edge();
            x >= seg.x && (x == right || (right >= 1 && x == right - 1))
        });

        let Some(seg) = grabbed else {
            return false;
        };

        self.current_tab_mut().focus = Focus::Output;
        self.active_resize = Some(super::ColumnDrag {
            col_index: seg.index,
            start_x: x,
            start_width: seg.full_width,
        });
        true
    }

    /// Apply an in-progress resize: width tracks the cursor relative to where the
    /// drag began, clamped to a readable range.
    fn update_column_resize(&mut self, x: u16) {
        const MIN_WIDTH: i32 = 3;
        const MAX_WIDTH: i32 = 240;

        let Some(drag) = self.active_resize else {
            return;
        };
        let delta = x as i32 - drag.start_x as i32;
        let new_width = (drag.start_width as i32 + delta).clamp(MIN_WIDTH, MAX_WIDTH) as usize;
        self.current_tab_mut()
            .result_col_overrides
            .insert(drag.col_index, new_width);
    }

    /// Route a wheel notch to the open popup as the equivalent arrow key, so the
    /// wheel scrolls/moves selection identically to the keyboard in every popup.
    fn scroll_active_popup(&mut self, scroll_up: bool) {
        let code = if scroll_up {
            KeyCode::Up
        } else {
            KeyCode::Down
        };
        if matches!(self.popup_state, PopupState::Help { .. }) {
            self.handle_help_popup_keys(code);
        } else if matches!(self.popup_state, PopupState::RecordDetail { .. }) {
            self.handle_record_detail_keys(code);
        } else if matches!(
            self.popup_state,
            PopupState::TemplateList { .. } | PopupState::Completion { .. }
        ) {
            self.handle_popup_keys(KeyEvent::new(code, KeyModifiers::NONE));
        }
    }

    /// Horizontal wheel over the result table pans columns, reusing the keyboard
    /// column-move logic (which keeps the selected column and h-scroll in sync).
    fn handle_mouse_hscroll(&mut self, x: u16, y: u16, scroll_right: bool) {
        if self.current_tab().view_state != ViewState::DatabaseView {
            return;
        }
        let over_output = self
            .view_layout
            .output
            .is_some_and(|r| rect_contains(r, x, y));
        if !over_output {
            return;
        }
        if scroll_right {
            self.move_column(1);
        } else {
            self.move_column(-1);
        }
    }

    fn handle_mouse_scroll(&mut self, x: u16, y: u16, scroll_up: bool) {
        let view_state = self.current_tab().view_state;
        const SCROLL_AMOUNT: usize = 3;

        match view_state {
            ViewState::ConnectionList => {
                let count = self.current_tab().filtered_connections().len();
                if count == 0 {
                    return;
                }
                let tab = self.current_tab_mut();
                if scroll_up {
                    tab.selected_index = tab.selected_index.saturating_sub(SCROLL_AMOUNT);
                } else {
                    tab.selected_index = (tab.selected_index + SCROLL_AMOUNT).min(count - 1);
                }
            }
            ViewState::DatabaseList => {
                let tab = self.current_tab_mut();
                let count = tab.databases.len();
                if count == 0 {
                    return;
                }
                if scroll_up {
                    tab.database_selected = tab.database_selected.saturating_sub(SCROLL_AMOUNT);
                } else {
                    tab.database_selected = (tab.database_selected + SCROLL_AMOUNT).min(count - 1);
                }
            }
            ViewState::DatabaseView => {
                // Scroll the pane actually under the cursor, so hovering the query
                // editor no longer scrolls the results by mistake.
                let layout = self.view_layout;
                let over = |rect: Option<ratatui::layout::Rect>| {
                    rect.is_some_and(|r| rect_contains(r, x, y))
                };

                if over(layout.sidebar) {
                    let tab = self.current_tab_mut();
                    let table_count = tab.sidebar.tables.len();
                    if table_count == 0 {
                        return;
                    }
                    if scroll_up {
                        tab.sidebar.selected = tab.sidebar.selected.saturating_sub(SCROLL_AMOUNT);
                    } else {
                        tab.sidebar.selected =
                            (tab.sidebar.selected + SCROLL_AMOUNT).min(table_count - 1);
                    }
                } else if over(layout.query) {
                    use tui_textarea::Scrolling;
                    let rows = if scroll_up {
                        -(SCROLL_AMOUNT as i16)
                    } else {
                        SCROLL_AMOUNT as i16
                    };
                    self.current_tab_mut()
                        .query_textarea
                        .scroll(Scrolling::Delta { rows, cols: 0 });
                } else if over(layout.output) {
                    let visible_height = self.output_visible_rows();
                    if scroll_up {
                        self.move_cursor(-(SCROLL_AMOUNT as i32), visible_height);
                    } else {
                        self.move_cursor(SCROLL_AMOUNT as i32, visible_height);
                    }
                }
            }
        }
    }

    /// Click in the connection or database picker: select the row under the
    /// cursor, and open it on a double-click.
    fn handle_mouse_picker(&mut self, x: u16, y: u16, is_double_click: bool) {
        let layout = self.view_layout;
        let Some(list) = layout.picker else {
            return;
        };
        if !rect_contains(list, x, y) {
            return;
        }
        let index = (y - list.y) as usize + layout.picker_offset;

        match self.current_tab().view_state {
            ViewState::ConnectionList => {
                if index < self.current_tab().filtered_connections().len() {
                    self.current_tab_mut().selected_index = index;
                    if is_double_click {
                        self.initiate_connection();
                    }
                }
            }
            ViewState::DatabaseList => {
                if index < self.current_tab().databases.len() {
                    self.current_tab_mut().database_selected = index;
                    if is_double_click {
                        self.connect_to_selected_database_from_list();
                    }
                }
            }
            ViewState::DatabaseView => {}
        }
    }

    fn handle_mouse_database_view(&mut self, x: u16, y: u16, is_double_click: bool) {
        let layout = self.view_layout;

        // Sidebar: map the click to a table via the rendered list rect + scroll offset.
        if let Some(rect) = layout.sidebar
            && rect_contains(rect, x, y)
        {
            self.current_tab_mut().focus = Focus::Sidebar;

            if let Some(list) = layout.sidebar_list
                && rect_contains(list, x, y)
            {
                let tab = self.current_tab();
                let table_count = tab.sidebar.tables.len();
                // List row 0 is the "Tables" header; tables follow at row >= 1.
                let list_row = (y - list.y) as usize + tab.sidebar.offset;
                if list_row >= 1 {
                    let clicked_index = list_row - 1;
                    if clicked_index < table_count {
                        self.current_tab_mut().sidebar.selected = clicked_index;
                        if is_double_click {
                            self.select_table();
                        }
                    }
                }
            }
            return;
        }

        // Query pane: focus only.
        if let Some(rect) = layout.query
            && rect_contains(rect, x, y)
        {
            self.current_tab_mut().focus = Focus::Query;
            return;
        }

        // Output pane: focus, and map the click to a result row when it lands on one.
        if let Some(rect) = layout.output
            && rect_contains(rect, x, y)
        {
            self.current_tab_mut().focus = Focus::Output;

            let mut clicked_valid_row = false;
            if let Some(table) = layout.output_table
                && rect_contains(table, x, y)
            {
                // The column under the cursor, from the rendered placements.
                let clicked_col = self
                    .result_columns
                    .iter()
                    .find(|seg| x >= seg.x && x < seg.right_edge())
                    .map(|seg| seg.index);

                let tab = self.current_tab_mut();
                if let Some(crate::db::QueryResult::Select { rows, .. }) = &tab.query_result {
                    // Header occupies table.y; data rows start at table.y + 1.
                    let on_header = y == table.y;
                    let actual_row = tab.result_scroll + (y - table.y).saturating_sub(1) as usize;
                    if on_header || actual_row < rows.len() {
                        if let Some(col) = clicked_col {
                            tab.result_selected_col = col;
                        }
                        if !on_header {
                            tab.result_cursor = actual_row;
                            clicked_valid_row = true;
                        }
                    }
                }
            }

            if is_double_click && clicked_valid_row {
                self.open_record_detail();
            }
        }
    }
}

/// Whether the point `(x, y)` falls inside `rect`.
fn rect_contains(rect: ratatui::layout::Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::*;
    use crate::controller::{ColumnDrag, ColumnSegment, DatabaseConn, DatabaseType};
    use crate::db::QueryResult;

    fn connection(name: &str) -> DatabaseConn {
        DatabaseConn {
            name: name.to_string(),
            db_type: DatabaseType::Sqlite,
            host: String::new(),
            port: 0,
            user: String::new(),
            password: None,
            password_cmd: None,
            database: None,
            path: Some(format!("{name}.db")),
            tls: false,
            readonly: false,
            group: None,
            protocol: None,
        }
    }

    /// A controller whose tab shows a two-row result, with the geometry the
    /// renderer would have captured for a 100x24 terminal.
    fn database_view() -> Controller {
        let connections = vec![connection("a"), connection("b"), connection("c")];
        let mut controller = Controller::with_connections(connections, None);

        let tab = controller.current_tab_mut();
        tab.view_state = ViewState::DatabaseView;
        tab.focus = Focus::Query;
        tab.query_result = Some(QueryResult::Select {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![
                vec!["1".to_string(), "ann".to_string()],
                vec!["2".to_string(), "bob".to_string()],
            ],
        });

        controller.tab_rects = vec![Rect::new(0, 0, 8, 1)];
        controller.view_layout.sidebar = Some(Rect::new(0, 1, 40, 20));
        controller.view_layout.query = Some(Rect::new(40, 1, 60, 7));
        controller.view_layout.output = Some(Rect::new(40, 8, 60, 13));
        // Header on row 9, data rows from row 10.
        controller.view_layout.output_table = Some(Rect::new(41, 9, 58, 12));
        controller.result_columns = vec![
            ColumnSegment {
                index: 0,
                x: 41,
                width: 6,
                full_width: 6,
            },
            ColumnSegment {
                index: 1,
                x: 47,
                width: 10,
                full_width: 10,
            },
        ];
        controller
    }

    fn mouse(controller: &mut Controller, kind: MouseEventKind, x: u16, y: u16) {
        controller.handle_mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        });
    }

    fn click(controller: &mut Controller, x: u16, y: u16) {
        mouse(controller, MouseEventKind::Down(MouseButton::Left), x, y);
    }

    fn key(controller: &mut Controller, code: KeyCode, modifiers: KeyModifiers) {
        controller.handle_normal_mode(KeyEvent::new(code, modifiers));
    }

    fn record_popup() -> PopupState {
        PopupState::RecordDetail {
            row_index: 0,
            selected_field: 0,
            scroll: 0,
        }
    }

    #[test]
    fn click_focuses_the_pane_under_the_cursor() {
        let mut controller = database_view();

        click(&mut controller, 10, 15);
        assert_eq!(controller.current_tab().focus, Focus::Sidebar);
        click(&mut controller, 60, 3);
        assert_eq!(controller.current_tab().focus, Focus::Query);
        click(&mut controller, 90, 19);
        assert_eq!(controller.current_tab().focus, Focus::Output);
    }

    #[test]
    fn click_on_a_result_cell_selects_its_row_and_column() {
        let mut controller = database_view();

        click(&mut controller, 50, 11);

        let tab = controller.current_tab();
        assert_eq!(tab.focus, Focus::Output);
        assert_eq!(tab.result_cursor, 1);
        assert_eq!(tab.result_selected_col, 1);
    }

    #[test]
    fn click_below_the_last_row_keeps_the_selection() {
        let mut controller = database_view();

        click(&mut controller, 50, 14);

        let tab = controller.current_tab();
        assert_eq!(tab.focus, Focus::Output);
        assert_eq!((tab.result_cursor, tab.result_selected_col), (0, 0));
    }

    /// Regression: Ctrl-w used to be handled before the popup, so a popup
    /// could be left open behind a tab switch. In a tab where it had nothing
    /// to draw it was invisible, yet it kept swallowing every mouse click.
    #[test]
    fn popup_is_modal_until_closed() {
        let mut controller = database_view();
        controller.new_tab();
        controller.set_current_tab(0);
        controller.popup_state = record_popup();

        key(&mut controller, KeyCode::Char('w'), KeyModifiers::CONTROL);
        key(&mut controller, KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(controller.current_tab, 0);
        assert!(matches!(
            controller.popup_state,
            PopupState::RecordDetail { .. }
        ));

        // Clicks do not reach the panes while it is open...
        click(&mut controller, 10, 15);
        assert_eq!(controller.current_tab().focus, Focus::Query);

        // ...and do again once it is closed.
        key(&mut controller, KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(controller.popup_state, PopupState::None));
        click(&mut controller, 10, 15);
        assert_eq!(controller.current_tab().focus, Focus::Sidebar);
    }

    #[test]
    fn switching_tabs_drops_popups_and_drags() {
        let mut controller = database_view();
        controller.new_tab();
        controller.popup_state = record_popup();
        controller.pending_ctrl_w = true;
        controller.active_resize = Some(ColumnDrag {
            col_index: 0,
            start_x: 46,
            start_width: 6,
        });

        controller.next_tab();

        assert!(matches!(controller.popup_state, PopupState::None));
        assert!(!controller.pending_ctrl_w);
        assert!(controller.active_resize.is_none());
    }

    #[test]
    fn tab_bar_click_uses_the_rendered_labels() {
        let mut controller = database_view();
        controller.new_tab();
        controller.set_current_tab(0);
        controller.tab_rects = vec![Rect::new(0, 0, 8, 1), Rect::new(9, 0, 7, 1)];

        // The one-cell gap between labels belongs to neither tab.
        click(&mut controller, 8, 0);
        assert_eq!(controller.current_tab, 0);

        click(&mut controller, 9, 0);
        assert_eq!(controller.current_tab, 1);
    }

    #[test]
    fn picker_click_accounts_for_the_scroll_offset() {
        let mut controller = database_view();
        controller.current_tab_mut().view_state = ViewState::ConnectionList;
        // Two visible rows, scrolled so that the second connection is on top.
        controller.view_layout.picker = Some(Rect::new(20, 5, 40, 2));
        controller.view_layout.picker_offset = 1;

        click(&mut controller, 25, 6);
        assert_eq!(controller.current_tab().selected_index, 2);
        click(&mut controller, 25, 5);
        assert_eq!(controller.current_tab().selected_index, 1);

        // Outside the list rows nothing is selected.
        click(&mut controller, 25, 7);
        assert_eq!(controller.current_tab().selected_index, 1);
    }

    #[test]
    fn column_drag_resizes_and_ends_on_release() {
        let mut controller = database_view();

        // Grab the right edge of the first column in the header row.
        click(&mut controller, 46, 9);
        assert!(controller.active_resize.is_some());
        mouse(
            &mut controller,
            MouseEventKind::Drag(MouseButton::Left),
            50,
            9,
        );
        assert_eq!(
            controller.current_tab().result_col_overrides.get(&0),
            Some(&10)
        );

        mouse(
            &mut controller,
            MouseEventKind::Up(MouseButton::Left),
            50,
            9,
        );
        assert!(controller.active_resize.is_none());
    }

    /// The release can happen outside the terminal and never be delivered.
    #[test]
    fn drag_without_a_release_ends_on_the_next_plain_move() {
        let mut controller = database_view();
        click(&mut controller, 46, 9);
        assert!(controller.active_resize.is_some());

        mouse(&mut controller, MouseEventKind::Moved, 60, 12);

        assert!(controller.active_resize.is_none());
    }
}
