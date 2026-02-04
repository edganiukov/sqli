use super::{Controller, Focus, Mode, PopupState, ViewState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
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

    /// Cancel any pending async operation on the current tab. Returns true if something was cancelled.
    fn cancel_pending_operation(&mut self) -> bool {
        let tab = self.current_tab_mut();
        let Some(op) = tab.pending_operation.take() else {
            return false;
        };

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
                let tab = self.current_tab_mut();
                if tab.db_client.is_some() {
                    // Already connected — go back to database view
                    tab.view_state = ViewState::DatabaseView;
                    tab.databases.clear();
                } else {
                    // Not connected — go back to connection list
                    tab.view_state = ViewState::ConnectionList;
                    tab.name = "New".to_string();
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
                KeyCode::Char(' ') => self.open_completion(),
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
            "help" | "h" => self.show_help(),
            cmd => {
                self.current_tab_mut().status_message = Some(format!("Unknown command: {}", cmd));
            }
        }
    }

    fn show_help(&mut self) {
        let help = ":q quit | :db switch database | F5/Ctrl+R exec | Ctrl+O templates | Ctrl+S save | Ctrl+G editor";
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

    pub fn handle_mouse(&mut self, event: MouseEvent) {
        // Block all mouse interaction when a popup is active
        if !matches!(self.popup_state, PopupState::None) {
            return;
        }

        let x = event.column;
        let y = event.row;

        // Get terminal size (approximate, since we don't have frame here)
        let term_size = crossterm::terminal::size().unwrap_or((80, 24));
        let term_height = term_size.1;

        // Layout constants
        const STATUS_LINE_HEIGHT: u16 = 1;
        const COMMAND_LINE_HEIGHT: u16 = 1;

        // Handle scroll events
        if matches!(
            event.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let scroll_up = matches!(event.kind, MouseEventKind::ScrollUp);
            self.handle_mouse_scroll(x, y, scroll_up, term_size);
            return;
        }

        // Only handle left button clicks for the rest
        if !matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }

        // Handle tab bar clicks (row 0)
        if y == 0 {
            self.handle_mouse_tab_bar(x);
            return;
        }

        // Skip if clicking on status or command line
        if y >= term_height - STATUS_LINE_HEIGHT - COMMAND_LINE_HEIGHT {
            return;
        }

        let view_state = self.current_tab().view_state;

        match view_state {
            ViewState::ConnectionList => {
                self.handle_mouse_connection_list(x, y, term_size);
            }
            ViewState::DatabaseList => {
                self.handle_mouse_database_list(x, y, term_size);
            }
            ViewState::DatabaseView => {
                self.handle_mouse_database_view(x, y, term_size);
            }
        }
    }

    fn handle_mouse_scroll(&mut self, x: u16, _y: u16, scroll_up: bool, term_size: (u16, u16)) {
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
                const SIDEBAR_WIDTH: u16 = 40;
                let main_area_height = term_size.1 - 3;
                let query_height = main_area_height * 35 / 100;

                if x < SIDEBAR_WIDTH {
                    // Scroll in sidebar
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
                } else {
                    // Scroll in output area
                    let visible_height = (term_size.1 - 3 - query_height) as usize;

                    if scroll_up {
                        self.move_cursor(-(SCROLL_AMOUNT as i32), visible_height);
                    } else {
                        self.move_cursor(SCROLL_AMOUNT as i32, visible_height);
                    }
                }
            }
        }
    }

    fn handle_mouse_tab_bar(&mut self, x: u16) {
        // Tabs are rendered as " TabName " with a space separator
        // Calculate which tab was clicked
        let mut current_x = 0u16;

        for (i, tab) in self.tabs.iter().enumerate() {
            let tab_width = (tab.name.len() + 2) as u16; // " TabName "

            if x >= current_x && x < current_x + tab_width {
                self.current_tab = i;
                return;
            }

            current_x += tab_width + 1; // +1 for separator space
        }
    }

    fn handle_mouse_connection_list(&mut self, _x: u16, y: u16, term_size: (u16, u16)) {
        let tab = self.current_tab();
        let filtered = tab.filtered_connections();
        let filtered_count = filtered.len();
        if filtered_count == 0 {
            return;
        }

        // Account for group tabs if present
        let has_groups = tab.connection_groups.len() > 1;
        let extra_offset = if has_groups { 2 } else { 0 }; // tabs + spacing

        // Connection list is centered - sized to total connections for consistency
        let total_count = tab.connections.len();
        let list_height = (total_count as u16 + 4 + extra_offset as u16).min(term_size.1 - 4);
        let list_y = 1 + (term_size.1 - 4 - list_height) / 2;

        // Check if click is within the list area (after tabs/hint if present)
        let list_content_y = list_y + 2 + extra_offset as u16;
        if y >= list_content_y && y < list_content_y + filtered_count as u16 {
            let clicked_index = (y - list_content_y) as usize;
            if clicked_index < filtered_count {
                self.current_tab_mut().selected_index = clicked_index;
                // Double-click effect: connect immediately
                self.initiate_connection();
            }
        }
    }

    fn handle_mouse_database_list(&mut self, _x: u16, y: u16, term_size: (u16, u16)) {
        let tab = self.current_tab();
        let db_count = tab.databases.len();
        if db_count == 0 {
            return;
        }

        // Database list is centered
        let list_height = (db_count as u16 + 4).min(term_size.1 - 4);
        let list_y = 1 + (term_size.1 - 4 - list_height) / 2;

        // Check if click is within the list area
        if y >= list_y + 2 && y < list_y + 2 + db_count as u16 {
            let clicked_index = (y - list_y - 2) as usize;
            if clicked_index < db_count {
                self.current_tab_mut().database_selected = clicked_index;
                // Connect to selected database
                self.connect_to_selected_database_from_list();
            }
        }
    }

    fn handle_mouse_database_view(&mut self, x: u16, y: u16, term_size: (u16, u16)) {
        const SIDEBAR_WIDTH: u16 = 40;
        let main_area_height = term_size.1 - 3; // minus tab bar, status, command
        let query_height = main_area_height * 35 / 100;

        if x < SIDEBAR_WIDTH {
            // Clicked on sidebar
            let was_focused = self.current_tab().focus == Focus::Sidebar;
            self.current_tab_mut().focus = Focus::Sidebar;

            // Calculate which table was clicked
            // Subtract: tab bar (1) + sidebar title (1) + "Tables" header (1) = 3
            let table_count = self.current_tab().sidebar.tables.len();
            let clicked_row = y.saturating_sub(3) as usize;

            if clicked_row < table_count {
                self.current_tab_mut().sidebar.selected = clicked_row;
                // Only select the table if sidebar was already focused
                if was_focused {
                    self.select_table();
                }
            }
        } else if y < 1 + query_height {
            // Clicked on query area
            self.current_tab_mut().focus = Focus::Query;
        } else {
            // Clicked on output area
            let was_focused = self.current_tab().focus == Focus::Output;
            self.current_tab_mut().focus = Focus::Output;

            // Calculate which row was clicked
            // Output area starts after: tab bar (1) + query area (query_height)
            // Then inside output: header row (1), then data rows
            let output_y_start = 1 + query_height;
            let clicked_row = (y - output_y_start).saturating_sub(2) as usize; // -1 for title, -1 for header

            let mut clicked_valid_row = false;
            let tab = self.current_tab_mut();
            if let Some(crate::db::QueryResult::Select { rows, .. }) = &tab.query_result {
                let actual_row = tab.result_scroll + clicked_row;
                if actual_row < rows.len() {
                    tab.result_cursor = actual_row;
                    clicked_valid_row = true;
                }
            }

            // Only open record detail popup if output was already focused
            // and the click landed on an actual data row
            if was_focused && clicked_valid_row {
                self.open_record_detail();
            }
        }
    }
}
