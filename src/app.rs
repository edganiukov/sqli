use crate::completion::{Suggestion, SuggestionKind};
use crate::controller::{Controller, Focus, Mode, PopupState, ViewState};
use crate::db::QueryResult;
use crate::templates::TemplateScope;
use crossterm::event::KeyEvent;
use ratatui::prelude::*;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table,
};

const PADDING: u16 = 1;

// Color scheme
const ACCENT: Color = Color::Rgb(180, 140, 100); // Warm tan
const BLUE: Color = Color::Rgb(70, 115, 150); // Dim blue
const SUCCESS: Color = Color::Rgb(130, 160, 110); // Muted green
const WARNING: Color = Color::Rgb(190, 160, 100); // Muted gold
const SURFACE: Color = Color::Rgb(18, 18, 22); // Dark background (focused)
const SURFACE_DIM: Color = Color::Rgb(38, 38, 44); // Lighter (unfocused)
const TEXT: Color = Color::Rgb(220, 220, 225); // Crisp white text
const TEXT_DIM: Color = Color::Rgb(120, 120, 125); // Cool dimmed text
const HIGHLIGHT: Color = Color::Rgb(50, 50, 60); // Cool selection

// UI Helper: Create a styled panel block with title
fn panel_block(title: &str, is_focused: bool, borders: Borders) -> Block<'_> {
    let border_color = if is_focused { BLUE } else { TEXT_DIM };
    let bg_color = if is_focused { SURFACE } else { SURFACE_DIM };

    Block::default()
        .title(Line::from(vec![
            Span::styled("── ", Style::default().fg(border_color)),
            Span::styled(
                title,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ──", Style::default().fg(border_color)),
        ]))
        .borders(borders)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color))
}

// UI Helper: Create a popup dialog block
fn popup_block(title: &str, border_color: Color) -> Block<'_> {
    Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(SURFACE))
}

// UI Helper: Calculate centered popup area
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let popup_width = width.min(area.width);
    let popup_height = height.min(area.height);
    Rect {
        x: (area.width.saturating_sub(popup_width)) / 2,
        y: (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    }
}

// UI Helper: Calculate centered popup area with percentage
fn centered_rect_pct(area: Rect, width_pct: f32, height_pct: f32, min_w: u16, min_h: u16) -> Rect {
    let popup_width = ((area.width as f32 * width_pct) as u16)
        .max(min_w)
        .min(area.width);
    let popup_height = ((area.height as f32 * height_pct) as u16)
        .max(min_h)
        .min(area.height);
    Rect {
        x: (area.width.saturating_sub(popup_width)) / 2,
        y: (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    }
}

// UI Helper: Get colors based on focus state
fn focus_colors(is_focused: bool) -> (Color, Color) {
    if is_focused {
        (BLUE, SURFACE)
    } else {
        (TEXT_DIM, SURFACE_DIM)
    }
}

// UI Helper: Truncate string to max width with ellipsis
fn truncate_str(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        s.chars().take(max_width).collect()
    } else {
        let truncated: String = s.chars().take(max_width - 3).collect();
        format!("{}...", truncated)
    }
}

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
            ViewState::ConnectionList => {
                self.draw_connection_list(frame, chunks[1]);
            }
            ViewState::DatabaseList => {
                self.draw_database_list(frame, chunks[1]);
            }
            ViewState::DatabaseView => self.draw_database_view(frame, chunks[1]),
        };

        // Status line
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

        let status_line = if view_state == ViewState::DatabaseView {
            let conn = tab.connections.get(tab.selected_index);

            let db_name = tab
                .current_database
                .as_ref()
                .map(|db| db.to_string())
                .unwrap_or_else(|| "(none)".to_string());

            let table_name = tab
                .sidebar
                .tables
                .get(tab.sidebar.selected)
                .map(|t| format!(" > {}", t))
                .unwrap_or_default();

            let is_readonly = conn.map(|c| c.readonly).unwrap_or(false);
            let ro_suffix = if is_readonly { " [RO]" } else { "" };

            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {}{}{} ", db_name, ro_suffix, table_name),
                    Style::default().fg(TEXT),
                ),
                Span::styled(status_msg, Style::default().fg(TEXT_DIM)),
            ]))
            .style(Style::default().bg(SURFACE_DIM))
        } else {
            Paragraph::new(Line::from(vec![Span::styled(
                status_msg,
                Style::default().fg(TEXT),
            )]))
            .style(Style::default().bg(SURFACE_DIM))
        };
        frame.render_widget(status_line, chunks[2]);

        // Command line
        let command_line = if self.controller.mode == Mode::Command {
            let command_text = format!(":{}", self.controller.command_buffer);
            frame.set_cursor_position((command_text.len() as u16, chunks[3].y));
            Paragraph::new(command_text).style(Style::default().fg(TEXT).bg(SURFACE))
        } else {
            Paragraph::new(":help for commands").style(Style::default().fg(TEXT_DIM).bg(SURFACE))
        };
        frame.render_widget(command_line, chunks[3]);

        // Draw popup overlay if active
        self.draw_popup(frame);
    }

    fn draw_tabs(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![];
        for (i, tab) in self.controller.tabs.iter().enumerate() {
            let tab_name = format!(" {} ", tab.name);
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
                        format!("[{}]", conn.db_type.as_str()),
                        Style::default().fg(TEXT_DIM),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        &conn.name,
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(location, Style::default().fg(TEXT_DIM)),
                ]))
            })
            .collect();

        let padded_area = Rect {
            x: area.x + PADDING,
            y: area.y + PADDING,
            width: area.width.saturating_sub(PADDING * 2),
            height: area.height.saturating_sub(PADDING),
        };

        // Calculate centered area - size based on largest group (or all connections)
        let list_width = 60.min(padded_area.width);
        let has_groups = tab.connection_groups.len() > 1;
        let extra_height = if has_groups { 2 } else { 0 }; // 1 for tabs, 1 for spacing
        // Use total connections count for consistent sizing
        let list_height = (tab.connections.len() as u16 + 4 + extra_height).min(padded_area.height);

        let list_x = padded_area.x + (padded_area.width.saturating_sub(list_width)) / 2;
        let list_y = padded_area.y + (padded_area.height.saturating_sub(list_height)) / 2;

        let list_area = Rect {
            x: list_x,
            y: list_y,
            width: list_width,
            height: list_height,
        };

        let block = Block::default()
            .title(" Connections ")
            .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(TEXT_DIM))
            .style(Style::default().bg(SURFACE));

        // Render block and get inner area with horizontal padding
        let inner_area = block.inner(list_area);
        frame.render_widget(block, list_area);

        let content_area = Rect {
            x: inner_area.x + 1,
            y: inner_area.y,
            width: inner_area.width.saturating_sub(2),
            height: inner_area.height,
        };

        // If we have groups, draw group tabs at the top
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
                    spans.push(Span::styled(" │ ", Style::default().fg(TEXT_DIM)));
                }
                let style = if i == tab.selected_group {
                    Style::default()
                        .fg(ACCENT)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    Style::default().fg(TEXT_DIM)
                };
                spans.push(Span::styled(group, style));
            }

            let group_tabs = Paragraph::new(Line::from(spans));
            frame.render_widget(group_tabs, tabs_area);

            // Adjust list area to be below group tabs
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
            .map(|db| ListItem::new(Line::from(Span::styled(db, Style::default().fg(TEXT)))))
            .collect();

        let padded_area = Rect {
            x: area.x + PADDING,
            y: area.y + PADDING,
            width: area.width.saturating_sub(PADDING * 2),
            height: area.height.saturating_sub(PADDING),
        };

        // Calculate centered area
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
            .title(" Select Database ")
            .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(TEXT_DIM))
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
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(40), // sidebar
                Constraint::Min(1),     // main area
            ])
            .split(area);

        self.draw_sidebar(frame, main_chunks[0]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(35), // query input
                Constraint::Percentage(65), // query output
            ])
            .split(main_chunks[1]);

        self.draw_query_input(frame, right_chunks[0]);
        self.draw_query_output(frame, right_chunks[1]);
    }

    fn draw_sidebar(&self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let is_focused = tab.focus == Focus::Sidebar;

        // Build items: "Tables" header + table entries with tree lines
        let table_count = tab.sidebar.tables.len();
        let mut items: Vec<ListItem> = Vec::with_capacity(table_count + 1);

        // Add "Tables" header (not selectable, just visual)
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Tables",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )])));

        // Add tables with tree-style prefixes
        for (i, table) in tab.sidebar.tables.iter().enumerate() {
            let is_last = i == table_count - 1;
            let prefix = if is_last { "└─ " } else { "├─ " };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(TEXT_DIM)),
                Span::styled(table, Style::default().fg(TEXT)),
            ])));
        }

        let block = panel_block("Explorer", is_focused, Borders::RIGHT);

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(if is_focused { HIGHLIGHT } else { SURFACE_DIM })
                    .fg(TEXT),
            )
            .highlight_symbol(if is_focused { "> " } else { "  " });

        let mut list_state = ListState::default();
        // Offset by 1 to account for "Tables" header
        list_state.select(Some(tab.sidebar.selected + 1));

        // Calculate scroll offset to keep selection visible
        // Add 1 for header, subtract 2 for borders
        let visible_height = area.height.saturating_sub(2) as usize;
        let selected = tab.sidebar.selected + 1; // +1 for header

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

        self.controller
            .query_textarea
            .set_style(Style::default().bg(bg_color).fg(TEXT));
        self.controller
            .query_textarea
            .set_cursor_style(Style::default().bg(if is_focused {
                Color::White
            } else {
                TEXT_DIM
            }));
        self.controller
            .query_textarea
            .set_cursor_line_style(Style::default());
        self.controller.query_textarea.set_block(block);
        self.controller
            .query_textarea
            .set_line_number_style(Style::default().fg(TEXT_DIM));
        frame.render_widget(&self.controller.query_textarea, area);
    }

    fn draw_query_output(&self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let is_focused = tab.focus == Focus::Output;
        let (_, bg_color) = focus_colors(is_focused);

        let title = match tab.visual_select {
            Some(crate::controller::VisualSelect::Cell { .. }) => "Results ── VISUAL",
            Some(crate::controller::VisualSelect::Line { .. }) => "Results ── VISUAL LINE",
            None => "Results",
        };
        let block = panel_block(title, is_focused, Borders::NONE);

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        match &tab.query_result {
            Some(QueryResult::Select { columns, rows }) => {
                if columns.is_empty() {
                    let msg = Paragraph::new("No results")
                        .style(Style::default().fg(TEXT_DIM).bg(bg_color));
                    frame.render_widget(msg, inner_area);
                } else {
                    let available_width = inner_area.width as usize;
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
                    // Add padding and apply min/max
                    for w in col_widths.iter_mut() {
                        *w = (*w + 2).clamp(min_col_width, max_col_width);
                    }

                    // Calculate horizontal scroll bounds
                    let total_width: usize = col_widths.iter().sum();
                    let h_scroll = tab.result_h_scroll;
                    let max_h_scroll = total_width.saturating_sub(available_width);
                    let h_scroll = h_scroll.min(max_h_scroll);

                    // Find which columns are visible based on h_scroll
                    let mut x_offset = 0usize;
                    let mut start_col = 0usize;
                    let mut start_col_offset = 0usize;

                    for (i, &w) in col_widths.iter().enumerate() {
                        if x_offset + w > h_scroll {
                            start_col = i;
                            start_col_offset = h_scroll.saturating_sub(x_offset);
                            break;
                        }
                        x_offset += w;
                    }

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
                                Style::default().fg(WARNING).add_modifier(Modifier::BOLD)
                            };
                            Cell::from(text).style(style)
                        });
                    let header = Row::new(header_cells)
                        .height(1)
                        .style(Style::default().bg(SURFACE_DIM));

                    // Calculate visible rows based on scroll position
                    let visible_height = inner_area.height.saturating_sub(1) as usize; // -1 for header
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
                            let in_visual_range =
                                visual_range.is_some_and(|(s, e)| idx >= s && idx <= e);
                            let cells = visible_col_indices
                                .iter()
                                .zip(visible_col_widths.iter())
                                .map(|(&col_idx, &w)| {
                                    let cell_text =
                                        row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                                    let text = truncate_str(cell_text, w.saturating_sub(1));
                                    let style = if is_line_visual && in_visual_range {
                                        // Line visual: highlight entire row
                                        Style::default().fg(TEXT).bg(BLUE)
                                    } else if is_cell_visual
                                        && in_visual_range
                                        && col_idx == selected_col
                                    {
                                        // Cell visual: highlight only the selected column
                                        Style::default().fg(TEXT).bg(BLUE)
                                    } else if col_idx == selected_col && is_cursor_row {
                                        // Selected cell (current row + current column)
                                        Style::default().fg(TEXT).bg(BLUE)
                                    } else if col_idx == selected_col && is_focused {
                                        // Selected column, other rows
                                        Style::default().fg(TEXT).bg(HIGHLIGHT)
                                    } else {
                                        Style::default().fg(TEXT)
                                    };
                                    Cell::from(text).style(style)
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

                    frame.render_widget(table, inner_area);
                }
            }
            Some(QueryResult::Execute { rows_affected }) => {
                let msg = Paragraph::new(format!("{} row(s) affected", rows_affected))
                    .style(Style::default().fg(SUCCESS).bg(bg_color));
                frame.render_widget(msg, inner_area);
            }
            None => {
                let msg = Paragraph::new(Line::from(vec![
                    Span::styled("Press ", Style::default().fg(TEXT_DIM)),
                    Span::styled("F5", Style::default().fg(BLUE).add_modifier(Modifier::BOLD)),
                    Span::styled(" to execute query", Style::default().fg(TEXT_DIM)),
                ]))
                .style(Style::default().bg(bg_color));
                frame.render_widget(msg, inner_area);
            }
        }
    }

    fn draw_popup(&self, frame: &mut Frame) {
        match &self.controller.popup_state {
            PopupState::TemplateList {
                selected,
                filter,
                searching,
            } => {
                self.draw_template_list_popup(frame, *selected, filter, *searching);
            }
            PopupState::SaveTemplate { name, scope } => {
                self.draw_save_template_popup(frame, name, scope);
            }
            PopupState::ConfirmDelete { name, .. } => {
                self.draw_confirm_delete_popup(frame, name);
            }
            PopupState::RecordDetail {
                row_index,
                selected_field,
                scroll,
            } => {
                self.draw_record_detail_popup(frame, *row_index, *selected_field, *scroll);
            }
            PopupState::Completion {
                suggestions,
                selected,
                ..
            } => {
                self.draw_completion_popup(frame, suggestions, *selected);
            }
            PopupState::None => {}
        }
    }

    fn draw_template_list_popup(
        &self,
        frame: &mut Frame,
        selected: usize,
        filter: &str,
        searching: bool,
    ) {
        let area = frame.area();
        let all_templates = &self.controller.template_list_cache;

        // Filter templates by search filter
        let filter_lower = filter.to_lowercase();
        let templates: Vec<_> = all_templates
            .iter()
            .filter(|t| t.name.to_lowercase().contains(&filter_lower))
            .collect();

        let popup_area = centered_rect_pct(area, 0.6, 0.7, 40, 10);
        frame.render_widget(Clear, popup_area);

        // Render block first and get inner area
        let block = popup_block("Templates (/ search, Ctrl+G edit, Ctrl+D delete)", BLUE);
        let inner_area = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        // Split inner area if searching (list + search bar inside the border)
        let (list_area, search_area) = if searching {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(inner_area);
            (chunks[0], Some(chunks[1]))
        } else {
            (inner_area, None)
        };

        let items: Vec<ListItem> = templates
            .iter()
            .map(|t| {
                let scope_str = match &t.scope {
                    TemplateScope::Global => "[global]".to_string(),
                    TemplateScope::Connection(name) => format!("[{}]", name),
                };
                let preview: String = t
                    .query
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(40)
                    .collect();

                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(scope_str, Style::default().fg(TEXT_DIM)),
                        Span::raw(" "),
                        Span::styled(
                            &t.name,
                            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(Span::styled(
                        format!("  {}", preview),
                        Style::default().fg(TEXT_DIM),
                    )),
                ])
            })
            .collect();

        let list = List::new(items)
            .highlight_style(Style::default().bg(HIGHLIGHT).fg(TEXT))
            .highlight_symbol("> ");

        let mut list_state = ListState::default();
        list_state.select(Some(selected.min(templates.len().saturating_sub(1))));

        frame.render_stateful_widget(list, list_area, &mut list_state);

        // Draw search input at bottom if searching (inside the border)
        if let Some(search_area) = search_area {
            let search_text = format!("/{}_", filter);
            let search_paragraph = Paragraph::new(search_text).style(Style::default().fg(TEXT));

            frame.render_widget(search_paragraph, search_area);
        }
    }

    fn draw_save_template_popup(&self, frame: &mut Frame, name: &str, scope: &TemplateScope) {
        let area = frame.area();
        let popup_area = centered_rect(area, 50, 9);
        frame.render_widget(Clear, popup_area);

        let block = popup_block("Save Template", BLUE);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Label
                Constraint::Length(1), // Input
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // Scope toggle
                Constraint::Length(1), // Help
            ])
            .margin(1)
            .split(inner);

        frame.render_widget(
            Paragraph::new("Template name:").style(Style::default().fg(TEXT)),
            chunks[0],
        );

        let input_display = format!("{}_", name);
        frame.render_widget(
            Paragraph::new(input_display).style(Style::default().fg(TEXT).bg(SURFACE_DIM)),
            chunks[1],
        );

        let scope_text = match scope {
            TemplateScope::Global => "[x] Global (all connections)",
            TemplateScope::Connection(conn) => &format!("[ ] Local ({})", conn),
        };
        frame.render_widget(
            Paragraph::new(format!("Tab to toggle: {}", scope_text))
                .style(Style::default().fg(TEXT_DIM)),
            chunks[3],
        );

        frame.render_widget(
            Paragraph::new("Enter to save, Esc to cancel").style(Style::default().fg(TEXT_DIM)),
            chunks[4],
        );
    }

    fn draw_confirm_delete_popup(&self, frame: &mut Frame, template_name: &str) {
        let area = frame.area();
        let popup_area = centered_rect(area, 40, 5);
        frame.render_widget(Clear, popup_area);

        let block = popup_block("Confirm Delete", WARNING);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .margin(1)
            .split(inner);

        frame.render_widget(
            Paragraph::new(format!("Delete '{}'? [y/N]", template_name))
                .style(Style::default().fg(TEXT))
                .alignment(Alignment::Center),
            chunks[0],
        );
    }

    fn draw_record_detail_popup(
        &self,
        frame: &mut Frame,
        row_index: usize,
        selected_field: usize,
        scroll: usize,
    ) {
        let tab = self.controller.current_tab();

        let Some(QueryResult::Select { columns, rows }) = &tab.query_result else {
            return;
        };

        let Some(row) = rows.get(row_index) else {
            return;
        };

        let area = frame.area();
        // Large popup - 90% width, 80% height
        let popup_area = centered_rect_pct(area, 0.9, 0.8, 60, 20);
        frame.render_widget(Clear, popup_area);

        let title = format!(
            "Record {} of {} ── Field {} of {}",
            row_index + 1,
            rows.len(),
            selected_field + 1,
            columns.len()
        );
        let block = popup_block(&title, BLUE);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        // Calculate the maximum field name width for alignment
        let max_name_width = columns.iter().map(|c| c.len()).max().unwrap_or(0);
        let value_width = inner.width.saturating_sub(max_name_width as u16 + 4) as usize; // 4 for " : " and padding

        // Build lines for each field, tracking which line each field starts at
        let mut lines: Vec<Line> = Vec::new();
        let mut field_start_lines: Vec<usize> = Vec::new();

        for (field_idx, (col, value)) in columns.iter().zip(row.iter()).enumerate() {
            field_start_lines.push(lines.len());

            let is_selected = field_idx == selected_field;
            let field_name = format!("{:>width$}", col, width = max_name_width);

            // Style based on selection
            let name_style = if is_selected {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(WARNING)
            };
            let value_style = if is_selected {
                Style::default().fg(TEXT).bg(HIGHLIGHT)
            } else {
                Style::default().fg(TEXT)
            };
            let empty_style = if is_selected {
                Style::default()
                    .fg(TEXT_DIM)
                    .bg(HIGHLIGHT)
                    .add_modifier(Modifier::ITALIC)
            } else {
                Style::default().fg(TEXT_DIM).add_modifier(Modifier::ITALIC)
            };

            if value.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled(field_name, name_style),
                    Span::styled(" : ", Style::default().fg(TEXT_DIM)),
                    Span::styled("(empty)", empty_style),
                ]));
            } else {
                // Wrap long values
                let value_lines = wrap_text(value, value_width);
                for (i, line_text) in value_lines.iter().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            Span::styled(field_name.clone(), name_style),
                            Span::styled(" : ", Style::default().fg(TEXT_DIM)),
                            Span::styled(line_text.clone(), value_style),
                        ]));
                    } else {
                        // Continuation lines - indent to align with value
                        let indent = " ".repeat(max_name_width + 3);
                        lines.push(Line::from(vec![
                            Span::styled(indent, Style::default()),
                            Span::styled(line_text.clone(), value_style),
                        ]));
                    }
                }
            }
            // Add empty line between fields for readability
            lines.push(Line::from(""));
        }

        // Auto-scroll to keep selected field visible
        let visible_lines = inner.height as usize;
        let selected_line = field_start_lines.get(selected_field).copied().unwrap_or(0);

        let actual_scroll = if selected_line < scroll {
            // Selected field is above viewport - scroll up
            selected_line
        } else if selected_line >= scroll + visible_lines {
            // Selected field is below viewport - scroll down
            selected_line.saturating_sub(visible_lines / 2)
        } else {
            // Keep current scroll if selected is visible
            scroll
        };

        // Clamp scroll to valid range
        let max_scroll = lines.len().saturating_sub(visible_lines);
        let actual_scroll = actual_scroll.min(max_scroll);

        let total_lines = lines.len();

        let paragraph = Paragraph::new(lines)
            .style(Style::default().bg(SURFACE))
            .scroll((actual_scroll as u16, 0));

        frame.render_widget(paragraph, inner);

        // Draw scroll indicator on the right edge if content overflows
        if total_lines > visible_lines {
            let h = inner.height as usize;
            let thumb_h = (h * visible_lines / total_lines).max(1);
            let thumb_pos = if max_scroll > 0 {
                actual_scroll * (h - thumb_h) / max_scroll
            } else {
                0
            };

            let scrollbar: String = (0..h)
                .map(|i| {
                    if i >= thumb_pos && i < thumb_pos + thumb_h {
                        "█"
                    } else {
                        "░"
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            let scrollbar_area = Rect {
                x: inner.x + inner.width.saturating_sub(1),
                y: inner.y,
                width: 1,
                height: inner.height,
            };
            frame.render_widget(
                Paragraph::new(scrollbar).style(Style::default().fg(TEXT_DIM)),
                scrollbar_area,
            );
        }
    }

    fn draw_completion_popup(
        &self,
        frame: &mut Frame,
        suggestions: &[Suggestion],
        selected: usize,
    ) {
        // Position popup near the cursor in the query textarea
        let area = frame.area();

        // Calculate popup position based on cursor
        let (cursor_row, cursor_col) = self.controller.query_textarea.cursor();

        // Query panel starts after sidebar (40 cols) + tab bar (1 row)
        // Textarea has line numbers (~3 chars)
        let base_x = 40u16 + 3; // sidebar + line numbers
        let base_y = 2u16; // tab bar + title

        // Position popup below cursor line
        let popup_x = (base_x + cursor_col as u16).min(area.width.saturating_sub(32));
        let popup_y = (base_y + cursor_row as u16 + 1).min(area.height.saturating_sub(12));

        let popup_width = 30u16;
        let popup_height = (suggestions.len() as u16).min(10);

        let popup_area = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width.min(area.width.saturating_sub(popup_x)),
            height: popup_height.min(area.height.saturating_sub(popup_y)),
        };

        frame.render_widget(Clear, popup_area);

        // Simple border - just left edge, lighter background
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(BLUE))
            .style(Style::default().bg(SURFACE_DIM));

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let items: Vec<ListItem> = suggestions
            .iter()
            .map(|s| {
                let kind_indicator = match s.kind {
                    SuggestionKind::Keyword => Span::styled("k ", Style::default().fg(TEXT_DIM)),
                    SuggestionKind::Table => Span::styled("t ", Style::default().fg(SUCCESS)),
                    SuggestionKind::Column => Span::styled("c ", Style::default().fg(WARNING)),
                };
                ListItem::new(Line::from(vec![
                    kind_indicator,
                    Span::styled(&s.text, Style::default().fg(TEXT)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .highlight_style(Style::default().bg(HIGHLIGHT).fg(TEXT))
            .highlight_symbol(">");

        let mut list_state = ListState::default();
        list_state.select(Some(selected));

        frame.render_stateful_widget(list, inner, &mut list_state);
    }
}

/// Wrap text to fit within a given width, breaking on word boundaries when possible
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    for line in text.lines() {
        if line.len() <= max_width {
            lines.push(line.to_string());
            continue;
        }

        let mut current = String::new();
        for word in line.split_whitespace() {
            let needed = if current.is_empty() {
                word.len()
            } else {
                current.len() + 1 + word.len()
            };

            if needed <= max_width {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            } else if word.len() > max_width {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                for chunk in word.as_bytes().chunks(max_width) {
                    lines.push(String::from_utf8_lossy(chunk).into_owned());
                }
            } else {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                current = word.to_string();
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}
