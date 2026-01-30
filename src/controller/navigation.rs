use super::{Controller, Focus, PopupState, QueryResult};

impl Controller {
    pub fn new_tab(&mut self) {
        // Clone connections from first tab for now
        let connections = self.tabs[0].connections.clone();
        self.tabs.push(super::Tab::new(connections));
        self.current_tab = self.tabs.len() - 1;
    }

    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.current_tab = (self.current_tab + 1) % self.tabs.len();
        }
    }

    pub fn previous_tab(&mut self) {
        if !self.tabs.is_empty() {
            if self.current_tab == 0 {
                self.current_tab = self.tabs.len() - 1;
            } else {
                self.current_tab -= 1;
            }
        }
    }

    pub fn close_current_tab(&mut self) {
        if self.tabs.len() == 1 {
            // Last tab, quit the app.
            self.quit = true;
        } else {
            self.tabs.remove(self.current_tab);
            if self.current_tab >= self.tabs.len() {
                self.current_tab = self.tabs.len() - 1;
            }
        }
    }

    pub(super) fn focus_left(&mut self) {
        // From Query or Output -> Sidebar
        let focus = self.current_tab().focus;
        if focus == Focus::Query || focus == Focus::Output {
            self.current_tab_mut().focus = Focus::Sidebar;
        }
    }

    pub(super) fn focus_right(&mut self) {
        // From Sidebar -> Query
        if self.current_tab().focus == Focus::Sidebar {
            self.current_tab_mut().focus = Focus::Query;
        }
    }

    pub(super) fn focus_up(&mut self) {
        // From Output -> Query
        if self.current_tab().focus == Focus::Output {
            self.current_tab_mut().focus = Focus::Query;
        }
    }

    pub(super) fn focus_down(&mut self) {
        // From Query -> Output
        if self.current_tab().focus == Focus::Query {
            self.current_tab_mut().focus = Focus::Output;
        }
    }

    pub(super) fn focus_sidebar(&mut self) {
        self.current_tab_mut().focus = Focus::Sidebar;
    }

    pub(super) fn scroll_to_end(&mut self) {
        let tab = self.current_tab_mut();
        if let Some(QueryResult::Select { rows, .. }) = &tab.query_result {
            let max_cursor = rows.len().saturating_sub(1);
            tab.result_cursor = max_cursor;
            // Scroll so cursor is visible at bottom
            tab.result_scroll = max_cursor;
        }
    }

    pub(super) fn scroll_to_start(&mut self) {
        let tab = self.current_tab_mut();
        tab.result_cursor = 0;
        tab.result_scroll = 0;
    }

    pub(super) fn move_cursor(&mut self, delta: i32, visible_height: usize) {
        let tab = self.current_tab_mut();
        if let Some(QueryResult::Select { rows, .. }) = &tab.query_result {
            let max_cursor = rows.len().saturating_sub(1);

            // Move cursor
            if delta > 0 {
                tab.result_cursor = (tab.result_cursor + delta as usize).min(max_cursor);
            } else {
                tab.result_cursor = tab.result_cursor.saturating_sub((-delta) as usize);
            }

            // Adjust scroll to keep cursor visible
            if tab.result_cursor < tab.result_scroll {
                tab.result_scroll = tab.result_cursor;
            } else if tab.result_cursor >= tab.result_scroll + visible_height {
                tab.result_scroll = tab.result_cursor - visible_height + 1;
            }
        }
    }

    pub(super) fn open_record_detail(&mut self) {
        let tab = self.current_tab();
        if let Some(QueryResult::Select { rows, .. }) = &tab.query_result
            && !rows.is_empty()
        {
            self.popup_state = PopupState::RecordDetail {
                row_index: tab.result_cursor,
                scroll: 0,
            };
        }
    }

    /// Calculate max horizontal scroll based on column widths
    fn calc_max_h_scroll(&self) -> usize {
        let tab = self.current_tab();
        if let Some(QueryResult::Select { columns, rows }) = &tab.query_result {
            const MIN_COL_WIDTH: usize = 12;
            const MAX_COL_WIDTH: usize = 50;

            // Calculate column widths (same logic as rendering)
            let mut col_widths: Vec<usize> = columns.iter().map(|h| h.len()).collect();
            for row in rows.iter() {
                for (i, cell) in row.iter().enumerate() {
                    if i < col_widths.len() {
                        col_widths[i] = col_widths[i].max(cell.len());
                    }
                }
            }
            for w in col_widths.iter_mut() {
                *w = (*w + 2).clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
            }

            let total_width: usize = col_widths.iter().sum();
            // Assume ~80 chars visible width as approximation (actual width set during render)
            // This is a reasonable default; the render will clamp if needed
            let available_width = 80;
            total_width.saturating_sub(available_width)
        } else {
            0
        }
    }

    pub(super) fn scroll_horizontal(&mut self, delta: i32) {
        let max_scroll = self.calc_max_h_scroll();
        let tab = self.current_tab_mut();

        if delta > 0 {
            tab.result_h_scroll = (tab.result_h_scroll + delta as usize).min(max_scroll);
        } else {
            tab.result_h_scroll = tab.result_h_scroll.saturating_sub((-delta) as usize);
        }
    }

    pub(super) fn scroll_horizontal_to_end(&mut self) {
        let max_scroll = self.calc_max_h_scroll();
        self.current_tab_mut().result_h_scroll = max_scroll;
    }
}
