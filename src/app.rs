use crate::controller::{Controller, Focus, Mode, PopupState, SidebarItem, ViewState};
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
const SURFACE: Color = Color::Rgb(30, 30, 35); // Cool dark background (focused)
const SURFACE_LIGHT: Color = Color::Rgb(35, 35, 40); // Slightly lighter (unfocused)
const TEXT: Color = Color::Rgb(220, 220, 225); // Crisp white text
const TEXT_DIM: Color = Color::Rgb(120, 120, 125); // Cool dimmed text
const HIGHLIGHT: Color = Color::Rgb(50, 50, 60); // Cool selection

// UI Helper: Create a styled panel block with title
fn panel_block(title: &str, is_focused: bool, borders: Borders) -> Block<'_> {
    let border_color = if is_focused { BLUE } else { TEXT_DIM };
    let bg_color = if is_focused { SURFACE } else { SURFACE_LIGHT };

    Block::default()
        .title(Line::from(vec![
            Span::styled("── ", Style::default().fg(border_color)),
            Span::styled(
                title,
                Style::default().fg(border_color).add_modifier(Modifier::BOLD),
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
    let popup_width = ((area.width as f32 * width_pct) as u16).max(min_w).min(area.width);
    let popup_height = ((area.height as f32 * height_pct) as u16).max(min_h).min(area.height);
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
        (TEXT_DIM, SURFACE_LIGHT)
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
        match self.controller.mode {
            Mode::Normal => self.controller.handle_normal_mode(key_event),
            Mode::Command => self.controller.handle_command_mode(key_event.code),
        }
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
            ViewState::DatabaseView => self.draw_database_view(frame, chunks[1]),
        };

        // Status line
        let tab = self.controller.current_tab();
        let status_msg = tab.status_message.as_deref().unwrap_or("");

        let status_line = if view_state == ViewState::DatabaseView {
            let conn = tab.connections.get(tab.selected_index);

            let db_name = tab
                .current_database
                .as_ref()
                .map(|db| db.to_string())
                .unwrap_or_else(|| "(none)".to_string());

            let table_name = tab
                .sidebar
                .items
                .get(tab.sidebar.selected)
                .and_then(|item| match item {
                    SidebarItem::Table { table, .. } => Some(format!(" > {}", table)),
                    _ => None,
                })
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
            .style(Style::default().bg(SURFACE_LIGHT))
        } else {
            Paragraph::new(Line::from(vec![Span::styled(
                status_msg,
                Style::default().fg(TEXT),
            )]))
            .style(Style::default().bg(SURFACE_LIGHT))
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
                Style::default().fg(SURFACE).bg(BLUE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_DIM).bg(SURFACE_LIGHT)
            };

            spans.push(Span::styled(tab_name, style));
            spans.push(Span::styled(" ", Style::default().bg(SURFACE)));
        }

        let tabs = Paragraph::new(Line::from(spans)).style(Style::default().bg(SURFACE));
        frame.render_widget(tabs, area);
    }

    fn draw_connection_list(&self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let items: Vec<ListItem> = tab
            .connections
            .iter()
            .map(|conn| {
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
                    Span::styled(
                        format!(" {}:{}", conn.host, conn.port),
                        Style::default().fg(TEXT_DIM),
                    ),
                ]))
            })
            .collect();

        let padded_area = Rect {
            x: area.x + PADDING,
            y: area.y + PADDING,
            width: area.width.saturating_sub(PADDING * 2),
            height: area.height.saturating_sub(PADDING),
        };

        // Calculate centered area.
        let list_width = 60.min(padded_area.width);
        let list_height = (tab.connections.len() as u16 + 4).min(padded_area.height);

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

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(HIGHLIGHT).fg(TEXT))
            .highlight_symbol("> ");

        let mut list_state = ListState::default();
        list_state.select(Some(tab.selected_index));

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

        let items: Vec<ListItem> = tab
            .sidebar
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let (prefix, name, style) = match item {
                    SidebarItem::Database(db) => (
                        "".to_string(),
                        db.clone(),
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    SidebarItem::Table { table, .. } => {
                        let is_last = tab
                            .sidebar
                            .items
                            .get(i + 1)
                            .map(|next| matches!(next, SidebarItem::Database(_)))
                            .unwrap_or(true);
                        let prefix = if is_last { "└─ " } else { "├─ " };
                        (prefix.to_string(), table.clone(), Style::default().fg(TEXT))
                    }
                };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(TEXT_DIM)),
                    Span::styled(name, style),
                ]))
            })
            .collect();

        let block = panel_block("Explorer", is_focused, Borders::RIGHT);

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default().bg(if is_focused { HIGHLIGHT } else { SURFACE_LIGHT }).fg(TEXT),
            )
            .highlight_symbol(if is_focused { "> " } else { "  " });

        let mut list_state = ListState::default();
        list_state.select(Some(tab.sidebar.selected));

        // Calculate scroll offset to keep selection 3 lines from bottom with 1 line padding
        let visible_height = area.height.saturating_sub(2) as usize; // subtract borders
        let selected = tab.sidebar.selected;
        let padding_bottom = 1;

        if visible_height > 0 && selected + padding_bottom >= visible_height {
            let offset = selected + padding_bottom - visible_height + 1;
            *list_state.offset_mut() = offset;
        }

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn draw_query_input(&mut self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let is_focused = tab.focus == Focus::Query;
        let (_, bg_color) = focus_colors(is_focused);

        let block = panel_block("Query", is_focused, Borders::BOTTOM);

        self.controller.query_textarea.set_style(Style::default().bg(bg_color).fg(TEXT));
        self.controller.query_textarea.set_cursor_style(Style::default().bg(if is_focused {
            Color::White
        } else {
            TEXT_DIM
        }));
        self.controller.query_textarea.set_cursor_line_style(Style::default());
        self.controller.query_textarea.set_block(block);
        self.controller.query_textarea.set_line_number_style(Style::default().fg(TEXT_DIM));
        frame.render_widget(&self.controller.query_textarea, area);
    }

    fn draw_query_output(&self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let is_focused = tab.focus == Focus::Output;
        let (_, bg_color) = focus_colors(is_focused);

        let block = panel_block("Results", is_focused, Borders::NONE);

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        match &tab.query_result {
            Some(QueryResult::Select { columns, rows }) => {
                if columns.is_empty() {
                    let msg = Paragraph::new("No results")
                        .style(Style::default().fg(TEXT_DIM).bg(bg_color));
                    frame.render_widget(msg, inner_area);
                } else {
                    // Calculate column widths based on content
                    let available_width = inner_area.width as usize;

                    let mut col_widths: Vec<usize> = columns.iter().map(|h| h.len()).collect();
                    for row in rows.iter() {
                        for (i, cell) in row.iter().enumerate() {
                            if i < col_widths.len() {
                                col_widths[i] = col_widths[i].max(cell.len());
                            }
                        }
                    }
                    // Add padding and cap very wide columns
                    for w in col_widths.iter_mut() {
                        *w = (*w + 2).min(60);
                    }
                    // Scale down to fit available width
                    let total_width: usize = col_widths.iter().sum();
                    if total_width > available_width && available_width > 0 {
                        let scale = available_width as f64 / total_width as f64;
                        let num_cols = col_widths.len();
                        let mut remaining = available_width;
                        for (i, w) in col_widths.iter_mut().enumerate() {
                            if i == num_cols - 1 {
                                *w = remaining;
                            } else {
                                *w = ((*w as f64 * scale) as usize).max(4);
                                remaining = remaining.saturating_sub(*w);
                            }
                        }
                    }

                    let header_cells = columns.iter().zip(col_widths.iter()).map(|(h, &w)| {
                        let text = truncate_str(h, w.saturating_sub(1));
                        Cell::from(text)
                            .style(Style::default().fg(WARNING).add_modifier(Modifier::BOLD))
                    });
                    let header =
                        Row::new(header_cells).height(1).style(Style::default().bg(SURFACE_LIGHT));

                    // Calculate visible rows based on scroll position
                    let visible_height = inner_area.height.saturating_sub(1) as usize; // -1 for header
                    let scroll = tab.result_scroll;
                    let cursor = tab.result_cursor;

                    let visible_rows =
                        rows.iter().enumerate().skip(scroll).take(visible_height).map(
                            |(idx, row)| {
                                let cells = row.iter().zip(col_widths.iter()).map(|(c, &w)| {
                                    let text = truncate_str(c, w.saturating_sub(1));
                                    Cell::from(text).style(Style::default().fg(TEXT))
                                });
                                let row = Row::new(cells).height(1);
                                if idx == cursor && is_focused {
                                    row.style(Style::default().bg(HIGHLIGHT))
                                } else {
                                    row.style(Style::default().bg(bg_color))
                                }
                            },
                        );

                    let widths: Vec<Constraint> =
                        col_widths.iter().map(|&w| Constraint::Length(w as u16)).collect();

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
            PopupState::RecordDetail { row_index, scroll } => {
                self.draw_record_detail_popup(frame, *row_index, *scroll);
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
                let preview: String =
                    t.query.lines().next().unwrap_or("").chars().take(40).collect();

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
            Paragraph::new(input_display).style(Style::default().fg(TEXT).bg(SURFACE_LIGHT)),
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

    fn draw_record_detail_popup(&self, frame: &mut Frame, row_index: usize, scroll: usize) {
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

        let title = format!("Record {} of {}", row_index + 1, rows.len());
        let block = popup_block(&title, BLUE);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        // Calculate the maximum field name width for alignment
        let max_name_width = columns.iter().map(|c| c.len()).max().unwrap_or(0);
        let value_width = inner.width.saturating_sub(max_name_width as u16 + 4) as usize; // 4 for " : " and padding

        // Build lines for each field, wrapping long values
        let mut lines: Vec<Line> = Vec::new();
        for (col, value) in columns.iter().zip(row.iter()) {
            // First line includes the field name
            let field_name = format!("{:>width$}", col, width = max_name_width);

            if value.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled(field_name, Style::default().fg(WARNING)),
                    Span::styled(" : ", Style::default().fg(TEXT_DIM)),
                    Span::styled(
                        "(empty)",
                        Style::default().fg(TEXT_DIM).add_modifier(Modifier::ITALIC),
                    ),
                ]));
            } else {
                // Wrap long values
                let value_lines = wrap_text(value, value_width);
                for (i, line_text) in value_lines.iter().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            Span::styled(field_name.clone(), Style::default().fg(WARNING)),
                            Span::styled(" : ", Style::default().fg(TEXT_DIM)),
                            Span::styled(line_text.clone(), Style::default().fg(TEXT)),
                        ]));
                    } else {
                        // Continuation lines - indent to align with value
                        let indent = " ".repeat(max_name_width + 3);
                        lines.push(Line::from(vec![
                            Span::styled(indent, Style::default()),
                            Span::styled(line_text.clone(), Style::default().fg(TEXT)),
                        ]));
                    }
                }
            }
            // Add empty line between fields for readability
            lines.push(Line::from(""));
        }

        // Clamp scroll to valid range
        let max_scroll = lines.len().saturating_sub(inner.height as usize);
        let actual_scroll = scroll.min(max_scroll);

        // Show scroll indicator in the content if there's more content
        let total_lines = lines.len();
        let visible_lines = inner.height as usize;

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
