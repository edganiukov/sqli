mod popups;
pub mod theme;
pub mod widgets;

use crate::controller::{Controller, Focus, Mode, PopupState, ViewState};
use crate::db::QueryResult;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table,
};
use tui_textarea::CursorMove;

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

    fn draw_tabs(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![];
        for (i, tab) in self.controller.tabs.iter().enumerate() {
            let tab_name = format!(" [{}:{}] ", tab.id, tab.name);
            let style = if i == self.controller.current_tab {
                Style::default()
                    .fg(SURFACE)
                    .bg(BLUE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_DIM).bg(SURFACE_DIM)
            };

            spans.push(Span::styled(tab_name, style));
            spans.push(Span::styled(" ", Style::default().bg(SURFACE)));
        }

        let tabs = Paragraph::new(Line::from(spans)).style(Style::default().bg(SURFACE));
        frame.render_widget(tabs, area);
    }

    fn draw_status_line(&self, frame: &mut Frame, area: Rect, view_state: ViewState) {
        let tab = self.controller.current_tab();

        let status_msg = if tab.loading {
            let spinner = self.controller.spinner_char();
            format!(
                "{} {}",
                spinner,
                tab.status_message.as_deref().unwrap_or("")
            )
        } else {
            tab.status_message.as_deref().unwrap_or("").to_string()
        };

        // Pending key indicator
        let pending_indicator = if self.controller.pending_ctrl_w {
            " ^W- "
        } else if self.controller.pending_escape {
            " Esc- "
        } else if self.controller.current_tab().pending_g {
            " g- "
        } else {
            ""
        };

        let status_line = if view_state == ViewState::DatabaseView {
            let conn = tab.connections.get(tab.connected_index);
            let db_name = tab.current_database.as_deref().unwrap_or("(none)");
            let table_name = tab
                .sidebar
                .tables
                .get(tab.sidebar.selected)
                .map(|t| format!(" > {}", t))
                .unwrap_or_default();
            let is_readonly = conn.map(|c| c.readonly).unwrap_or(false);
            let ro_suffix = if is_readonly { " [RO]" } else { "" };

            let left = format!(" {}{}{} ", db_name, ro_suffix, table_name);
            let left_len = left.len() + status_msg.len();
            let fill = (area.width as usize).saturating_sub(left_len + pending_indicator.len());

            Paragraph::new(Line::from(vec![
                Span::styled(left, text()),
                Span::styled(status_msg, dim()),
                Span::styled(" ".repeat(fill), Style::default()),
                Span::styled(pending_indicator, Style::default().fg(ACCENT)),
            ]))
            .style(Style::default().bg(SURFACE_DIM))
        } else {
            let fill =
                (area.width as usize).saturating_sub(status_msg.len() + pending_indicator.len());

            Paragraph::new(Line::from(vec![
                Span::styled(&status_msg, text()),
                Span::styled(" ".repeat(fill), Style::default()),
                Span::styled(pending_indicator, Style::default().fg(ACCENT)),
            ]))
            .style(Style::default().bg(SURFACE_DIM))
        };

        frame.render_widget(status_line, area);
    }

    fn draw_command_line(&self, frame: &mut Frame, area: Rect) {
        let command_line = if self.controller.mode == Mode::Command {
            let command_text = format!(":{}", self.controller.command_buffer);
            frame.set_cursor_position((command_text.len() as u16, area.y));
            Paragraph::new(command_text).style(Style::default().fg(TEXT).bg(SURFACE))
        } else {
            Paragraph::new(":help for commands").style(dim().bg(SURFACE))
        };
        frame.render_widget(command_line, area);
    }

    fn draw_connection_list(&self, frame: &mut Frame, area: Rect) {
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
            .border_style(dim())
            .style(Style::default().bg(SURFACE));

        let inner_area = block.inner(list_area);
        frame.render_widget(block, list_area);

        let content_area = Rect {
            x: inner_area.x + 1,
            y: inner_area.y,
            width: inner_area.width.saturating_sub(2),
            height: inner_area.height,
        };

        if has_groups {
            let tabs_area = Rect {
                x: content_area.x,
                y: content_area.y,
                width: content_area.width,
                height: 1,
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

            let list_content_area = Rect {
                x: content_area.x,
                y: content_area.y + 2,
                width: content_area.width,
                height: content_area.height.saturating_sub(2),
            };

            let list = List::new(items)
                .highlight_style(Style::default().bg(HIGHLIGHT).fg(TEXT))
                .highlight_symbol("> ");

            let mut list_state = ListState::default();
            list_state.select(Some(
                tab.selected_index.min(filtered.len().saturating_sub(1)),
            ));

            frame.render_stateful_widget(list, list_content_area, &mut list_state);
        } else {
            let list = List::new(items)
                .highlight_style(Style::default().bg(HIGHLIGHT).fg(TEXT))
                .highlight_symbol("> ");

            let mut list_state = ListState::default();
            list_state.select(Some(tab.selected_index));

            frame.render_stateful_widget(list, content_area, &mut list_state);
        }
    }

    fn draw_database_list(&self, frame: &mut Frame, area: Rect) {
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
            .border_style(dim())
            .style(Style::default().bg(SURFACE));

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(HIGHLIGHT).fg(TEXT))
            .highlight_symbol("> ");

        let mut list_state = ListState::default();
        list_state.select(Some(tab.database_selected));

        frame.render_stateful_widget(list, list_area, &mut list_state);
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
                    .border_style(Style::default().fg(TEXT_DIM))
                    .style(Style::default().bg(SURFACE_DIM)),
                cols[0],
            );
            cols[1]
        } else {
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(40), Constraint::Min(1)])
                .split(area);
            self.draw_sidebar(frame, main_chunks[0]);
            main_chunks[1]
        };

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(right_area);

        self.draw_query_input(frame, right_chunks[0]);
        self.draw_query_output(frame, right_chunks[1]);
    }

    fn draw_sidebar(&self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let is_focused = tab.focus == Focus::Sidebar;

        let table_count = tab.sidebar.tables.len();
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

        let title = if table_count > 0 {
            format!("Explorer ({})", table_count)
        } else {
            "Explorer".to_string()
        };
        let block = panel_block(&title, is_focused, Borders::RIGHT);

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(if is_focused { HIGHLIGHT } else { SURFACE_DIM })
                    .fg(TEXT),
            )
            .highlight_symbol(if is_focused { "> " } else { "  " });

        let mut list_state = ListState::default();
        list_state.select(Some(tab.sidebar.selected + 1));

        let visible_height = area.height.saturating_sub(2) as usize;
        let selected = tab.sidebar.selected + 1;

        if visible_height > 0 && selected >= visible_height {
            let offset = selected - visible_height + 1;
            *list_state.offset_mut() = offset;
        }

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
            .set_cursor_style(Style::default()
                .fg(bg_color)
                .bg(if is_focused { TEXT } else { TEXT_DIM }));
        tab.query_textarea.set_cursor_line_style(Style::default());
        tab.query_textarea.set_block(block);
        tab.query_textarea.set_line_number_style(dim());
        frame.render_widget(&tab.query_textarea, area);
    }

    fn draw_query_output(&self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let is_focused = tab.focus == Focus::Output;
        let (_, bg_color) = focus_colors(is_focused);

        let row_count = match &tab.query_result {
            Some(QueryResult::Select { rows, .. }) => Some(rows.len()),
            _ => None,
        };
        let title = match (tab.visual_select, row_count) {
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

        match &tab.query_result {
            Some(QueryResult::Select { columns, rows }) => {
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
    }

    fn draw_result_table(
        &self,
        frame: &mut Frame,
        area: Rect,
        columns: &[String],
        rows: &[Vec<String>],
        is_focused: bool,
        bg_color: ratatui::style::Color,
    ) {
        if columns.is_empty() {
            let msg = Paragraph::new("No results").style(dim().bg(bg_color));
            frame.render_widget(msg, area);
            return;
        }

        let tab = self.controller.current_tab();
        let available_width = area.width as usize;
        let min_col_width: usize = 12;
        let max_col_width: usize = 50;

        // Calculate column widths based on content
        let mut col_widths: Vec<usize> = columns.iter().map(|h| h.len()).collect();
        for row in rows.iter() {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }
        for w in col_widths.iter_mut() {
            *w = (*w + 2).clamp(min_col_width, max_col_width);
        }

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
                        .bg(HIGHLIGHT)
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
                let is_cursor_row = idx == cursor && is_focused;
                let in_visual_range = visual_range.is_some_and(|(s, e)| idx >= s && idx <= e);

                let cells = visible_col_indices
                    .iter()
                    .zip(visible_col_widths.iter())
                    .map(|(&col_idx, &w)| {
                        let cell_text = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                        let display_text = truncate_str(cell_text, w.saturating_sub(1));
                        let style = if is_line_visual && in_visual_range {
                            Style::default().fg(TEXT).bg(BLUE)
                        } else if is_cell_visual && in_visual_range && col_idx == selected_col {
                            Style::default().fg(TEXT).bg(BLUE)
                        } else if col_idx == selected_col && is_cursor_row {
                            Style::default().fg(TEXT).bg(BLUE)
                        } else if col_idx == selected_col && is_focused {
                            Style::default().fg(TEXT).bg(HIGHLIGHT)
                        } else {
                            text()
                        };
                        Cell::from(display_text).style(style)
                    });

                let row = Row::new(cells).height(1);
                if is_line_visual && in_visual_range {
                    row.style(Style::default().bg(BLUE))
                } else if is_cursor_row {
                    row.style(Style::default().bg(HIGHLIGHT))
                } else {
                    row.style(Style::default().bg(bg_color))
                }
            });

        let widths: Vec<Constraint> = visible_col_widths
            .iter()
            .map(|&w| Constraint::Length(w as u16))
            .collect();

        let table = Table::new(visible_rows, widths)
            .header(header)
            .style(Style::default().bg(bg_color));

        frame.render_widget(table, area);
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
