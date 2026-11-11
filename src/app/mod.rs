mod popups;
pub mod theme;
pub mod widgets;

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, Wrap,
};
use tui_textarea::CursorMove;

use crate::controller::{
    ColumnSegment, Controller, Focus, Mode, PendingOperation, PopupState, ViewLayout, ViewState,
};
use crate::db::QueryResult;
use crate::result_table::result_table_widths_with_overrides;
use crate::session::ConnStatus;

use theme::*;
use widgets::*;

pub struct App {
    controller: Controller,
}

impl App {
    pub fn new(controller: Controller) -> Self {
        Self { controller }
    }

    pub fn quit(&self) -> bool {
        self.controller.quit
    }

    pub fn needs_redraw(&mut self) -> bool {
        if self.controller.needs_redraw {
            self.controller.needs_redraw = false;
            true
        } else {
            false
        }
    }

    pub fn handle_key(&mut self, key_event: KeyEvent) {
        crate::debug_log!(
            "handle_key: mode={:?}, key={:?}",
            self.controller.mode,
            key_event.code
        );
        match self.controller.mode {
            Mode::Normal => self.controller.handle_normal_mode(key_event),
            Mode::Command => self.controller.handle_command_mode(key_event.code),
        }
    }

    pub fn handle_mouse(&mut self, mouse_event: crossterm::event::MouseEvent) {
        self.controller.handle_mouse(mouse_event);
    }

    pub fn handle_paste(&mut self, text: String) {
        self.controller.handle_paste(text);
    }

    pub fn handle_resize(&mut self) {
        let tab = self.controller.current_tab_mut();
        tab.query_textarea.move_cursor(CursorMove::Top);
        tab.query_textarea.move_cursor(CursorMove::Head);
    }

    pub fn tick_spinner(&mut self) {
        self.controller.tick_spinner();
    }

    pub fn poll_pending(&mut self) {
        self.controller.poll_pending();
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Geometry is captured again below by whatever is drawn this frame.
        self.controller.view_layout = ViewLayout::default();

        // Fill background
        frame.render_widget(Block::default().style(Style::default().bg(SURFACE)), area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // tab bar
                Constraint::Min(1),    // main window
                Constraint::Length(1), // status line
                Constraint::Length(1), // command line
            ])
            .split(area);

        self.draw_tabs(frame, chunks[0]);

        let view_state = self.controller.current_tab().view_state;
        match view_state {
            ViewState::ConnectionList => self.draw_connection_list(frame, chunks[1]),
            ViewState::DatabaseList => self.draw_database_list(frame, chunks[1]),
            ViewState::DatabaseView => self.draw_database_view(frame, chunks[1]),
        };

        self.draw_status_line(frame, chunks[2], view_state);
        self.draw_command_line(frame, chunks[3]);

        // Draw popup overlay if active
        self.draw_popup(frame);
    }

    fn draw_tabs(&mut self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![];
        let mut rects = Vec::with_capacity(self.controller.tabs.len());
        let mut x = area.x;

        for (i, tab) in self.controller.tabs.iter().enumerate() {
            let is_current = i == self.controller.current_tab;
            let base = if is_current {
                Style::default()
                    .fg(SURFACE)
                    .bg(BLUE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_MUTED).bg(SURFACE_DIM)
            };

            // Connection health marker. The status colors are unreadable on the
            // current tab's bright background, where the glyph alone tells.
            let marker = match tab.conn_status() {
                Some(ConnStatus::Connected) => Some(("●", SUCCESS)),
                Some(ConnStatus::Reconnecting { .. }) => Some(("◌", WARNING)),
                None => None,
            };

            let mut label = vec![Span::styled(" ", base)];
            if let Some((glyph, color)) = marker {
                let style = if is_current { base } else { base.fg(color) };
                label.push(Span::styled(format!("{} ", glyph), style));
            }
            label.push(Span::styled(format!("{}:{} ", tab.id, tab.name), base));

            // Remember where the label landed so clicks hit exactly what is drawn.
            let width: u16 = label.iter().map(|span| span.width() as u16).sum();
            rects.push(Rect {
                x,
                y: area.y,
                width: width.min(area.right().saturating_sub(x)),
                height: area.height.min(1),
            });
            x = x.saturating_add(width).saturating_add(1);

            spans.extend(label);
            spans.push(Span::styled(" ", Style::default().bg(SURFACE)));
        }
        self.controller.tab_rects = rects;

        let tabs = Paragraph::new(Line::from(spans)).style(Style::default().bg(SURFACE));
        frame.render_widget(tabs, area);
    }

    fn draw_status_line(&self, frame: &mut Frame, area: Rect, view_state: ViewState) {
        let tab = self.controller.current_tab();
        let conn_status = tab.conn_status();
        let mut left: Vec<Span> = Vec::new();

        // Connection health: always visible while the tab has a connection.
        match &conn_status {
            Some(ConnStatus::Connected) => {
                left.push(Span::styled(" ● ", Style::default().fg(SUCCESS)));
            }
            Some(ConnStatus::Reconnecting { .. }) => {
                let spinner = format!(" {} ", self.controller.spinner_char());
                left.push(Span::styled(spinner, bold(WARNING)));
            }
            None => left.push(Span::raw(" ")),
        }

        if view_state == ViewState::DatabaseView {
            let db_name = tab.current_database.as_deref().unwrap_or("(none)");
            left.push(Span::styled(db_name.to_string(), bold(TEXT)));

            let is_readonly = tab
                .connections
                .get(tab.connected_index)
                .is_some_and(|c| c.readonly);
            if is_readonly {
                left.push(Span::raw(" "));
                left.push(Span::styled(" RO ", bold(SURFACE).bg(WARNING)));
            }
            if let Some(table) = tab.sidebar.tables.get(tab.sidebar.selected) {
                left.push(Span::styled(format!(" › {}", table), dim()));
            }
            left.push(Span::styled("  │  ", Style::default().fg(BORDER)));
        }

        // Message: progress while working, reconnect progress while the link
        // is down, otherwise the last status message.
        if tab.loading {
            let mut message = format!(
                "{} {}",
                self.controller.spinner_char(),
                tab.status_message.as_deref().unwrap_or("")
            );
            if let Some(PendingOperation::Query { start, .. }) = &tab.pending_operation {
                message.push_str(&format!("  {:.1}s", start.elapsed().as_secs_f32()));
            }
            left.push(Span::styled(message, text()));
        } else if let Some(ConnStatus::Reconnecting {
            attempt,
            error,
            retry_in,
        }) = &conn_status
        {
            let mut progress = "Reconnecting".to_string();
            if *attempt > 0 {
                progress.push_str(&format!(" · attempt {}", attempt));
            }
            if let Some(wait) = retry_in {
                progress.push_str(&format!(" · retry in {}s", wait.as_secs_f32().ceil()));
            }
            left.push(Span::styled(progress, bold(WARNING)));
            left.push(Span::styled(
                format!("  {}", error.replace('\n', " ")),
                Style::default().fg(ERROR),
            ));
        } else {
            let message = tab.status_message.as_deref().unwrap_or("");
            left.push(Span::styled(
                message.replace('\n', " "),
                Style::default().fg(TEXT_MUTED),
            ));
        }

        // Right side: pending key chord, then the position in the result set.
        let mut right: Vec<Span> = Vec::new();
        let pending_indicator = if self.controller.pending_ctrl_w {
            " ^W- "
        } else if self.controller.pending_escape {
            " Esc- "
        } else if tab.pending_g {
            " g- "
        } else {
            ""
        };
        if !pending_indicator.is_empty() {
            right.push(Span::styled(pending_indicator, bold(ACCENT)));
        }
        if view_state == ViewState::DatabaseView
            && let Some(QueryResult::Select { columns, rows }) = &tab.query_result
            && !rows.is_empty()
        {
            right.push(Span::styled(
                format!(
                    " row {}/{} · col {}/{} ",
                    tab.result_cursor + 1,
                    rows.len(),
                    tab.result_selected_col + 1,
                    columns.len()
                ),
                dim(),
            ));
        }

        let right = Line::from(right);
        let bar = Style::default().bg(SURFACE_DIM);
        let [left_area, right_area] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length((right.width() as u16).min(area.width)),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(Line::from(left)).style(bar), left_area);
        frame.render_widget(Paragraph::new(right).style(bar), right_area);
    }

    /// The bottom row is the `:` command prompt and nothing else. Shortcuts
    /// are documented in the help popup (`?`), not here.
    fn draw_command_line(&self, frame: &mut Frame, area: Rect) {
        if self.controller.mode != Mode::Command {
            return;
        }
        let command_text = format!(":{}", self.controller.command_buffer);
        let cursor_x = area.x + command_text.chars().count() as u16;
        frame.set_cursor_position((cursor_x, area.y));
        frame.render_widget(
            Paragraph::new(command_text).style(Style::default().fg(TEXT).bg(SURFACE)),
            area,
        );
    }

    fn draw_connection_list(&mut self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let filtered = tab.filtered_connections();

        let items: Vec<ListItem> = filtered
            .iter()
            .map(|conn| {
                let location = if let Some(ref path) = conn.path {
                    format!(" {}", path)
                } else {
                    format!(" {}:{}", conn.host, conn.port)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>2}", conn.db_type.short_label()),
                        Style::default().fg(TEXT_DIM),
                    ),
                    Span::styled(" | ", dim()),
                    Span::styled(&conn.name, bold(TEXT)),
                    Span::styled(location, dim()),
                ]))
            })
            .collect();

        let padded_area = Rect {
            x: area.x + PADDING,
            y: area.y + PADDING,
            width: area.width.saturating_sub(PADDING * 2),
            height: area.height.saturating_sub(PADDING),
        };

        let list_width = 60.min(padded_area.width);
        let has_groups = tab.connection_groups.len() > 1;
        let extra_height = if has_groups { 2 } else { 0 };
        let list_height = (tab.connections.len() as u16 + 4 + extra_height).min(padded_area.height);

        let list_x = padded_area.x + (padded_area.width.saturating_sub(list_width)) / 2;
        let list_y = padded_area.y + (padded_area.height.saturating_sub(list_height)) / 2;

        let list_area = Rect {
            x: list_x,
            y: list_y,
            width: list_width,
            height: list_height,
        };

        let conn_count = filtered.len();
        let title = format!(" Connections ({}) ", conn_count);
        let block = Block::default()
            .title(title)
            .title_style(accent_bold())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(SURFACE));

        let inner_area = block.inner(list_area);
        frame.render_widget(block, list_area);

        let content_area = Rect {
            x: inner_area.x + 1,
            y: inner_area.y,
            width: inner_area.width.saturating_sub(2),
            height: inner_area.height,
        };

        let items_area = if has_groups {
            let tabs_area = Rect {
                height: content_area.height.min(1),
                ..content_area
            };

            let mut spans = vec![];
            for (i, group) in tab.connection_groups.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" | ", dim()));
                }
                let style = if i == tab.selected_group {
                    Style::default()
                        .fg(ACCENT)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    dim()
                };
                spans.push(Span::styled(group, style));
            }

            frame.render_widget(Paragraph::new(Line::from(spans)), tabs_area);

            Rect {
                y: content_area.y + 2,
                height: content_area.height.saturating_sub(2),
                ..content_area
            }
        } else {
            content_area
        };

        let list = List::new(items)
            .highlight_style(Style::default().bg(HIGHLIGHT).fg(TEXT))
            .highlight_symbol("> ");

        let mut list_state = ListState::default();
        list_state.select(Some(tab.selected_index.min(conn_count.saturating_sub(1))));

        frame.render_stateful_widget(list, items_area, &mut list_state);

        // Clicks map to items through the drawn rows and the list's scroll offset.
        self.controller.view_layout.picker = Some(items_area);
        self.controller.view_layout.picker_offset = list_state.offset();
    }

    fn draw_database_list(&mut self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let items: Vec<ListItem> = tab
            .databases
            .iter()
            .map(|db| ListItem::new(Line::from(Span::styled(db, text()))))
            .collect();

        let padded_area = Rect {
            x: area.x + PADDING,
            y: area.y + PADDING,
            width: area.width.saturating_sub(PADDING * 2),
            height: area.height.saturating_sub(PADDING),
        };

        let list_width = 50.min(padded_area.width);
        let list_height = (tab.databases.len() as u16 + 4).min(padded_area.height);

        let list_x = padded_area.x + (padded_area.width.saturating_sub(list_width)) / 2;
        let list_y = padded_area.y + (padded_area.height.saturating_sub(list_height)) / 2;

        let list_area = Rect {
            x: list_x,
            y: list_y,
            width: list_width,
            height: list_height,
        };

        let block = Block::default()
            .title(format!(" Select Database ({}) ", tab.databases.len()))
            .title_style(accent_bold())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(SURFACE));
        let items_area = block.inner(list_area);

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(HIGHLIGHT).fg(TEXT))
            .highlight_symbol("> ");

        let mut list_state = ListState::default();
        list_state.select(Some(tab.database_selected));

        frame.render_stateful_widget(list, list_area, &mut list_state);

        // Clicks map to items through the drawn rows and the list's scroll offset.
        self.controller.view_layout.picker = Some(items_area);
        self.controller.view_layout.picker_offset = list_state.offset();
    }

    fn draw_database_view(&mut self, frame: &mut Frame, area: Rect) {
        let sidebar_hidden = self.controller.current_tab().sidebar_hidden;

        let right_area = if sidebar_hidden {
            // Keep a 1-column strip on the left with a right border — mirrors the
            // sidebar's right edge so the layout doesn't feel flush to the terminal wall.
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(1), Constraint::Min(1)])
                .split(area);
            frame.render_widget(
                Block::default()
                    .borders(Borders::RIGHT)
                    .border_style(Style::default().fg(BORDER))
                    .style(Style::default().bg(SURFACE_ALT)),
                cols[0],
            );
            self.controller.view_layout.sidebar = None;
            self.controller.view_layout.sidebar_list = None;
            cols[1]
        } else {
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(40), Constraint::Min(1)])
                .split(area);
            let sidebar_area = main_chunks[0];
            self.draw_sidebar(frame, sidebar_area);
            self.controller.view_layout.sidebar = Some(sidebar_area);
            main_chunks[1]
        };

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(right_area);
        let query_area = right_chunks[0];
        let output_area = right_chunks[1];

        self.controller.view_layout.query = Some(query_area);
        self.controller.view_layout.output = Some(output_area);

        self.draw_query_input(frame, query_area);
        self.draw_query_output(frame, output_area);
    }

    fn draw_sidebar(&mut self, frame: &mut Frame, area: Rect) {
        let is_focused = self.controller.current_tab().focus == Focus::Sidebar;
        let table_count = self.controller.current_tab().sidebar.tables.len();

        let title = if table_count > 0 {
            format!("Explorer ({})", table_count)
        } else {
            "Explorer".to_string()
        };
        let block = panel_block(&title, is_focused, Borders::RIGHT);

        // Sync the persisted scroll offset to the real viewport height so the
        // selection stays visible and click hit-testing matches the screen.
        let list_inner = block.inner(area);
        self.controller.view_layout.sidebar_list = Some(list_inner);
        let offset = self
            .controller
            .current_tab_mut()
            .sidebar
            .scroll_to_selected(list_inner.height as usize);

        let tab = self.controller.current_tab();
        let selected_row = tab.sidebar.selected_row();

        let mut items: Vec<ListItem> = Vec::with_capacity(table_count + 1);
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Tables",
            accent_bold(),
        )])));
        for (i, table) in tab.sidebar.tables.iter().enumerate() {
            let is_last = i == table_count - 1;
            let prefix = if is_last { "└─ " } else { "├─ " };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(prefix, dim()),
                Span::styled(table, text()),
            ])));
        }

        let list = List::new(items)
            .block(block)
            // The selection stays visible, just quieter, when the pane is not focused.
            .highlight_style(
                Style::default()
                    .bg(if is_focused { HIGHLIGHT } else { HIGHLIGHT_DIM })
                    .fg(TEXT),
            )
            .highlight_symbol(if is_focused { "> " } else { "  " });

        let mut list_state = ListState::default();
        list_state.select(Some(selected_row));
        *list_state.offset_mut() = offset;

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn draw_query_input(&mut self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let is_focused = tab.focus == Focus::Query;
        let (_, bg_color) = focus_colors(is_focused);

        let block = panel_block("Query", is_focused, Borders::BOTTOM);

        let tab = self.controller.current_tab_mut();
        tab.query_textarea
            .set_style(Style::default().bg(bg_color).fg(TEXT));
        tab.query_textarea
            .set_cursor_style(Style::default().fg(bg_color).bg(if is_focused {
                TEXT
            } else {
                TEXT_DIM
            }));
        tab.query_textarea.set_cursor_line_style(Style::default());
        tab.query_textarea.set_block(block);
        tab.query_textarea.set_line_number_style(dim());
        frame.render_widget(&tab.query_textarea, area);
    }

    fn draw_query_output(&mut self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let is_focused = tab.focus == Focus::Output;
        let (_, bg_color) = focus_colors(is_focused);

        let row_count = match &tab.query_result {
            Some(QueryResult::Select { rows, .. }) => Some(rows.len()),
            _ => None,
        };
        let title = match (tab.visual_select, row_count) {
            _ if tab.query_error.is_some() => "Results ── error".to_string(),
            (Some(crate::controller::VisualSelect::Cell { .. }), _) => {
                "Results ── VISUAL".to_string()
            }
            (Some(crate::controller::VisualSelect::Line { .. }), _) => {
                "Results ── VISUAL LINE".to_string()
            }
            (None, Some(count)) => format!("Results ({} rows)", count),
            (None, None) => "Results".to_string(),
        };
        let block = panel_block(&title, is_focused, Borders::NONE);

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        // Add padding to output content
        let padded_area = Rect {
            x: inner_area.x + 1,
            y: inner_area.y,
            width: inner_area.width.saturating_sub(2),
            height: inner_area.height,
        };
        self.controller.view_layout.output_table = Some(padded_area);

        let mut segments = Vec::new();
        let tab = self.controller.current_tab();
        match &tab.query_result {
            // A failed query shows its full message here, wrapped: the status
            // bar only has room for the first line.
            _ if tab.query_error.is_some() => {
                let error = tab.query_error.as_deref().unwrap_or_default();
                let mut lines = vec![
                    Line::from(Span::styled("Query failed", bold(ERROR))),
                    Line::from(""),
                ];
                lines.extend(error.lines().map(|line| Line::from(line.to_string())));
                let msg = Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(TEXT).bg(bg_color));
                frame.render_widget(msg, padded_area);
            }
            Some(QueryResult::Select { columns, rows }) => {
                segments =
                    self.draw_result_table(frame, padded_area, columns, rows, is_focused, bg_color);
            }
            Some(QueryResult::Execute { rows_affected }) => {
                let msg = Paragraph::new(format!("{} row(s) affected", rows_affected))
                    .style(Style::default().fg(SUCCESS).bg(bg_color));
                frame.render_widget(msg, padded_area);
            }
            None => {
                let msg = Paragraph::new(Line::from(vec![
                    Span::styled("Press ", dim()),
                    Span::styled("F5", bold(BLUE)),
                    Span::styled(" to execute query", dim()),
                ]))
                .style(Style::default().bg(bg_color));
                frame.render_widget(msg, padded_area);
            }
        }
        self.controller.result_columns = segments;
    }

    fn draw_result_table(
        &self,
        frame: &mut Frame,
        area: Rect,
        columns: &[String],
        rows: &[Vec<String>],
        is_focused: bool,
        bg_color: ratatui::style::Color,
    ) -> Vec<ColumnSegment> {
        if columns.is_empty() {
            let msg = Paragraph::new("No results").style(dim().bg(bg_color));
            frame.render_widget(msg, area);
            return Vec::new();
        }

        let tab = self.controller.current_tab();
        let available_width = area.width as usize;
        if available_width == 0 {
            return Vec::new();
        }

        // Auto-fit widths, then apply any manual resize overrides.
        let col_widths = result_table_widths_with_overrides(
            columns,
            rows,
            available_width,
            &tab.result_col_overrides,
        );

        // Calculate horizontal scroll bounds
        let total_width: usize = col_widths.iter().sum();
        let h_scroll = tab
            .result_h_scroll
            .min(total_width.saturating_sub(available_width));

        // Find visible columns based on h_scroll
        let (start_col, start_col_offset) = {
            let mut x_offset = 0usize;
            let mut result = (0, 0);
            for (i, &w) in col_widths.iter().enumerate() {
                if x_offset + w > h_scroll {
                    result = (i, h_scroll.saturating_sub(x_offset));
                    break;
                }
                x_offset += w;
            }
            result
        };

        // Build visible columns with adjusted widths
        let mut visible_col_widths = Vec::new();
        let mut visible_col_indices = Vec::new();
        let mut remaining_width = available_width;

        for (i, &w) in col_widths.iter().enumerate().skip(start_col) {
            if remaining_width == 0 {
                break;
            }
            let effective_width = if i == start_col {
                w.saturating_sub(start_col_offset)
            } else {
                w
            };
            let col_w = effective_width.min(remaining_width);
            visible_col_widths.push(col_w);
            visible_col_indices.push(i);
            remaining_width = remaining_width.saturating_sub(col_w);
        }

        // Record where each visible column landed on screen so mouse hit-testing
        // (column resize) reads the exact rendered geometry.
        let mut segments = Vec::with_capacity(visible_col_indices.len());
        let mut seg_x = area.x;
        for (&col_idx, &rendered_w) in visible_col_indices.iter().zip(visible_col_widths.iter()) {
            segments.push(ColumnSegment {
                index: col_idx,
                x: seg_x,
                width: rendered_w as u16,
                full_width: col_widths[col_idx],
            });
            seg_x = seg_x.saturating_add(rendered_w as u16);
        }

        let selected_col = tab.result_selected_col;

        // Header row
        let header_cells = visible_col_indices
            .iter()
            .zip(visible_col_widths.iter())
            .map(|(&col_idx, &w)| {
                let text = truncate_str(&columns[col_idx], w.saturating_sub(1));
                let style = if col_idx == selected_col && is_focused {
                    Style::default()
                        .fg(ACCENT)
                        .bg(COLUMN_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    bold(WARNING)
                };
                Cell::from(text).style(style)
            });
        let header = Row::new(header_cells)
            .height(1)
            .style(Style::default().bg(SURFACE_DIM));

        // Data rows
        let visible_height = area.height.saturating_sub(1) as usize;
        let scroll = tab.result_scroll;
        let cursor = tab.result_cursor;
        let visual_range = tab.visual_selection_range();
        let is_cell_visual = matches!(
            tab.visual_select,
            Some(crate::controller::VisualSelect::Cell { .. })
        );
        let is_line_visual = matches!(
            tab.visual_select,
            Some(crate::controller::VisualSelect::Line { .. })
        );

        let visible_rows = rows
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible_height)
            .map(|(idx, row)| {
                let is_cursor_row = idx == cursor;
                let in_visual_range = visual_range.is_some_and(|(s, e)| idx >= s && idx <= e);

                let cells = visible_col_indices
                    .iter()
                    .zip(visible_col_widths.iter())
                    .map(|(&col_idx, &w)| {
                        let cell_text = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                        let display_text = truncate_str(cell_text, w.saturating_sub(1));
                        let in_selection = (is_line_visual && in_visual_range)
                            || (is_cell_visual && in_visual_range && col_idx == selected_col)
                            || (col_idx == selected_col && is_cursor_row && is_focused);
                        let style = if in_selection {
                            Style::default().fg(TEXT).bg(SELECTION)
                        } else if col_idx == selected_col && is_focused {
                            Style::default().fg(TEXT).bg(COLUMN_HIGHLIGHT)
                        } else {
                            text()
                        };
                        Cell::from(display_text).style(style)
                    });

                let row = Row::new(cells).height(1);
                if is_line_visual && in_visual_range {
                    row.style(Style::default().bg(SELECTION))
                } else if is_cursor_row {
                    // Quieter band when unfocused, so the position is not lost.
                    row.style(Style::default().bg(if is_focused {
                        HIGHLIGHT
                    } else {
                        HIGHLIGHT_DIM
                    }))
                } else {
                    row.style(Style::default().bg(bg_color))
                }
            });

        let widths: Vec<Constraint> = visible_col_widths
            .iter()
            .map(|&w| Constraint::Length(w as u16))
            .collect();

        // No inter-column spacing: column widths already include padding, and a zero
        // gap keeps the rendered layout identical to the contiguous width math above,
        // so header-border resize hit-testing lines up exactly with what's drawn.
        let table = Table::new(visible_rows, widths)
            .header(header)
            .column_spacing(0)
            .style(Style::default().bg(bg_color));

        frame.render_widget(table, area);

        segments
    }

    fn draw_popup(&self, frame: &mut Frame) {
        match &self.controller.popup_state {
            PopupState::TemplateList {
                selected,
                filter,
                searching,
            } => {
                popups::draw_template_list(frame, &self.controller, *selected, filter, *searching);
            }
            PopupState::SaveTemplate {
                name,
                connections,
                editing_connections,
            } => {
                popups::draw_save_template(frame, name, connections, *editing_connections);
            }
            PopupState::ConfirmDelete { name, .. } => {
                popups::draw_confirm_delete(frame, name);
            }
            PopupState::RecordDetail {
                row_index,
                selected_field,
                scroll,
            } => {
                popups::draw_record_detail(
                    frame,
                    &self.controller,
                    *row_index,
                    *selected_field,
                    *scroll,
                );
            }
            PopupState::Completion {
                suggestions,
                selected,
                ..
            } => {
                popups::draw_completion(frame, &self.controller, suggestions, *selected);
            }
            PopupState::Help { scroll } => {
                popups::draw_help(frame, *scroll);
            }
            PopupState::None => {}
        }
    }
}
