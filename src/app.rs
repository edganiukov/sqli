use crate::controller::{Controller, Focus, Mode, SidebarItem, ViewState};
use crate::db::QueryResult;
use crossterm::event::KeyEvent;
use ratatui::prelude::*;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table,
};

const PADDING: u16 = 1;

// Color scheme
const ACCENT: Color = Color::Rgb(180, 140, 100); // Warm tan
const BLUE: Color = Color::Rgb(70, 115, 150); // Dim blue
const SUCCESS: Color = Color::Rgb(130, 160, 110); // Muted green
const WARNING: Color = Color::Rgb(190, 160, 100); // Muted gold
const SURFACE: Color = Color::Rgb(30, 30, 35); // Cool dark background
const SURFACE_LIGHT: Color = Color::Rgb(45, 45, 50); // Cool medium gray
const TEXT: Color = Color::Rgb(220, 220, 225); // Crisp white text
const TEXT_DIM: Color = Color::Rgb(120, 120, 125); // Cool dimmed text
const HIGHLIGHT: Color = Color::Rgb(50, 50, 60); // Cool selection

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
            let db_info = tab
                .current_database
                .as_ref()
                .map(|db| format!(" {} ", db))
                .unwrap_or_else(|| " No database ".to_string());

            let conn_info =
                tab.connected_db.as_ref().map(|c| format!(" {} ", c)).unwrap_or_default();

            Paragraph::new(Line::from(vec![
                Span::styled(conn_info, Style::default().fg(TEXT).bg(SURFACE_LIGHT)),
                Span::styled(db_info, Style::default().fg(SURFACE).bg(BLUE)),
                Span::raw(" "),
                Span::styled(status_msg, Style::default().fg(TEXT)),
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
                        // Check if next item is a database or end (this table is last in group)
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

        let border_color = if is_focused { BLUE } else { TEXT_DIM };
        let bg_color = if is_focused { SURFACE } else { SURFACE_LIGHT };

        let block = Block::default()
            .title(Line::from(vec![
                Span::styled("── ", Style::default().fg(border_color)),
                Span::styled(
                    "Explorer",
                    Style::default().fg(border_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ──", Style::default().fg(border_color)),
            ]))
            .borders(Borders::RIGHT)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(bg_color));

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default().bg(if is_focused { HIGHLIGHT } else { SURFACE_LIGHT }).fg(TEXT),
            )
            .highlight_symbol(if is_focused { "> " } else { "  " });

        let mut list_state = ListState::default();
        list_state.select(Some(tab.sidebar.selected));

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn draw_query_input(&mut self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let is_focused = tab.focus == Focus::Query;

        let border_color = if is_focused { BLUE } else { TEXT_DIM };
        let bg_color = if is_focused { SURFACE } else { SURFACE_LIGHT };

        let block = Block::default()
            .title(Line::from(vec![
                Span::styled("── ", Style::default().fg(border_color)),
                Span::styled(
                    "Query",
                    Style::default().fg(border_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ──", Style::default().fg(border_color)),
            ]))
            .borders(Borders::BOTTOM)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(bg_color));

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
        let bg_color = if is_focused { SURFACE } else { SURFACE_LIGHT };

        let border_color = if is_focused { BLUE } else { TEXT_DIM };

        let block = Block::default()
            .title(Line::from(vec![
                Span::styled("── ", Style::default().fg(border_color)),
                Span::styled(
                    "Results",
                    Style::default().fg(border_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ──", Style::default().fg(border_color)),
            ]))
            .style(Style::default().bg(bg_color));

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
                    let mut col_widths: Vec<usize> = columns.iter().map(|h| h.len()).collect();
                    for row in rows.iter() {
                        for (i, cell) in row.iter().enumerate() {
                            if i < col_widths.len() {
                                col_widths[i] = col_widths[i].max(cell.len());
                            }
                        }
                    }
                    // Add padding
                    for w in col_widths.iter_mut() {
                        *w += 2;
                    }
                    // Scale down if total exceeds available width
                    let total_width: usize = col_widths.iter().sum();
                    let available_width = inner_area.width as usize;
                    if total_width > available_width {
                        let scale = available_width as f64 / total_width as f64;
                        for w in col_widths.iter_mut() {
                            *w = ((*w as f64 * scale) as usize).max(4);
                        }
                    }

                    let header_cells = columns.iter().map(|h| {
                        Cell::from(h.as_str())
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
                                let cells = row.iter().map(|c| {
                                    Cell::from(c.as_str()).style(Style::default().fg(TEXT))
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
                    Span::styled(" or ", Style::default().fg(TEXT_DIM)),
                    Span::styled(
                        "Ctrl+J",
                        Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to execute query", Style::default().fg(TEXT_DIM)),
                ]))
                .style(Style::default().bg(bg_color));
                frame.render_widget(msg, inner_area);
            }
        }
    }
}
