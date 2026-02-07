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
            // Compute visible height to position scroll so last row is at bottom
            let term_size = crossterm::terminal::size().unwrap_or((80, 24));
            let main_area_height = term_size.1.saturating_sub(3);
            let output_height = main_area_height * 65 / 100;
            let visible_height = output_height.saturating_sub(3).max(1) as usize;
            tab.result_scroll = max_cursor.saturating_sub(visible_height.saturating_sub(1));
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

        // Calculate actual visible width from terminal size minus sidebar
        let term_width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80) as usize;
        const SIDEBAR_WIDTH: usize = 40;
        let visible_width = term_width.saturating_sub(SIDEBAR_WIDTH);
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

        // Calculate actual visible width from terminal size minus sidebar
        let term_width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80) as usize;
        const SIDEBAR_WIDTH: usize = 40;
        let visible_width = term_width.saturating_sub(SIDEBAR_WIDTH);

        if col_start < tab.result_h_scroll {
            // Column is to the left of viewport
            tab.result_h_scroll = col_start;
        } else if col_end > tab.result_h_scroll + visible_width {
            // Column is to the right of viewport
            tab.result_h_scroll = col_end.saturating_sub(visible_width);
        }
    }

    /// Toggle cell visual selection (v) — selects cells in the current column
    pub(super) fn toggle_visual_select_cell(&mut self) {
        let tab = self.current_tab_mut();
        if matches!(tab.visual_select, Some(super::VisualSelect::Cell { .. })) {
            tab.visual_select = None;
        } else {
            tab.visual_select = Some(super::VisualSelect::Cell {
                anchor: tab.result_cursor,
            });
        }
    }

    /// Toggle line visual selection (V) — selects entire rows
    pub(super) fn toggle_visual_select_line(&mut self) {
        let tab = self.current_tab_mut();
        if matches!(tab.visual_select, Some(super::VisualSelect::Line { .. })) {
            tab.visual_select = None;
        } else {
            tab.visual_select = Some(super::VisualSelect::Line {
                anchor: tab.result_cursor,
            });
        }
    }

    /// Yank (copy) selected cells/rows to the system clipboard
    pub(super) fn yank_selected_rows(&mut self) {
        let tab = self.current_tab();
        let visual = tab.visual_select;
        let cursor = tab.result_cursor;
        let selected_col = tab.result_selected_col;

        let Some(QueryResult::Select { columns, rows }) = &tab.query_result else {
            return;
        };

        match visual {
            Some(super::VisualSelect::Cell { .. }) => {
                // Cell mode: copy values from the selected column only
                let Some((sel_start, sel_end)) = tab.visual_selection_range() else {
                    return;
                };
                let col_name = columns.get(selected_col).cloned().unwrap_or_default();
                let mut lines = vec![col_name];
                for row in rows.iter().skip(sel_start).take(sel_end - sel_start + 1) {
                    let val = row.get(selected_col).cloned().unwrap_or_default();
                    lines.push(val);
                }
                let text = lines.join("\n");
                let count = sel_end - sel_start + 1;
                self.copy_to_clipboard(&text, count, "cell(s)");
                self.current_tab_mut().visual_select = None;
            }
            Some(super::VisualSelect::Line { .. }) => {
                // Line mode: copy entire rows as TSV
                let Some((sel_start, sel_end)) = tab.visual_selection_range() else {
                    return;
                };
                let header = columns.join("\t");
                let mut lines = vec![header];
                for row in rows.iter().skip(sel_start).take(sel_end - sel_start + 1) {
                    lines.push(row.join("\t"));
                }
                let text = lines.join("\n");
                let count = sel_end - sel_start + 1;
                self.copy_to_clipboard(&text, count, "row(s)");
                self.current_tab_mut().visual_select = None;
            }
            None => {
                // No visual mode — yank the single cell at cursor
                if let Some(row) = rows.get(cursor) {
                    let val = row.get(selected_col).cloned().unwrap_or_default();
                    self.copy_to_clipboard(&val, 1, "cell(s)");
                }
            }
        }
    }

    /// Copy text to the system clipboard and update status message
    pub(super) fn copy_to_clipboard(&mut self, text: &str, count: usize, unit: &str) {
        match &mut self.clipboard {
            Some(cb) => match cb.set_text(text) {
                Ok(()) => {
                    self.current_tab_mut().status_message =
                        Some(format!("Copied {} {}", count, unit));
                }
                Err(e) => {
                    self.current_tab_mut().status_message = Some(format!("Clipboard error: {}", e));
                }
            },
            None => {
                self.current_tab_mut().status_message = Some("Clipboard not available".to_string());
            }
        }
    }
}
