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
                selected_field: 0,
                scroll: 0,
            };
        }
    }

    pub(super) fn move_column_to_end(&mut self) {
        let tab = self.current_tab();

        let (total_width, max_col) = match &tab.query_result {
            Some(QueryResult::Select { columns, rows }) => {
                const MIN_COL_WIDTH: usize = 12;
                const MAX_COL_WIDTH: usize = 50;

                let mut widths: Vec<usize> = columns.iter().map(|h| h.len()).collect();
                for row in rows.iter() {
                    for (i, cell) in row.iter().enumerate() {
                        if i < widths.len() {
                            widths[i] = widths[i].max(cell.len());
                        }
                    }
                }
                for w in widths.iter_mut() {
                    *w = (*w + 2).clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
                }
                let total: usize = widths.iter().sum();
                (total, columns.len().saturating_sub(1))
            }
            _ => return,
        };

        let tab = self.current_tab_mut();
        tab.result_selected_col = max_col;

        // Scroll to show the last column
        let visible_width = 80usize;
        tab.result_h_scroll = total_width.saturating_sub(visible_width);
    }

    pub(super) fn move_column(&mut self, delta: i32) {
        let tab = self.current_tab();

        // Get column info from query result
        let (col_widths, max_col) = match &tab.query_result {
            Some(QueryResult::Select { columns, rows }) => {
                const MIN_COL_WIDTH: usize = 12;
                const MAX_COL_WIDTH: usize = 50;

                let mut widths: Vec<usize> = columns.iter().map(|h| h.len()).collect();
                for row in rows.iter() {
                    for (i, cell) in row.iter().enumerate() {
                        if i < widths.len() {
                            widths[i] = widths[i].max(cell.len());
                        }
                    }
                }
                for w in widths.iter_mut() {
                    *w = (*w + 2).clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
                }
                (widths, columns.len().saturating_sub(1))
            }
            _ => return,
        };

        let tab = self.current_tab_mut();

        // Update selected column
        if delta > 0 {
            tab.result_selected_col = (tab.result_selected_col + delta as usize).min(max_col);
        } else {
            tab.result_selected_col = tab.result_selected_col.saturating_sub((-delta) as usize);
        }

        // Calculate position of selected column
        let col_start: usize = col_widths.iter().take(tab.result_selected_col).sum();
        let col_end = col_start
            + col_widths
                .get(tab.result_selected_col)
                .copied()
                .unwrap_or(0);

        // Auto-scroll to keep selected column visible (assume ~80 char visible width)
        // The actual visible width will be set during render, but this is a reasonable default
        let visible_width = 80usize;

        if col_start < tab.result_h_scroll {
            // Column is to the left of viewport
            tab.result_h_scroll = col_start;
        } else if col_end > tab.result_h_scroll + visible_width {
            // Column is to the right of viewport
            tab.result_h_scroll = col_end.saturating_sub(visible_width);
        }
    }
}
