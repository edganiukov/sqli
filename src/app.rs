use crate::controller::{Controller, Mode, ViewState};
use crossterm::event::KeyEvent;
use ratatui::prelude::*;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph};

const PADDING_HORIZONTAL: u16 = 1;
const PADDING_VERTICAL: u16 = 1;

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

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // tab bar
                Constraint::Min(1),    // main window
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

        let command_text = match self.controller.mode {
            Mode::Normal => String::new(),
            Mode::Command => format!(":{}", self.controller.command_buffer),
        };

        let command_line = Paragraph::new(command_text.clone());
        frame.render_widget(command_line, chunks[2]);

        if self.controller.mode == Mode::Command {
            frame.set_cursor_position((command_text.len() as u16, chunks[2].y));
        }
    }

    fn draw_tabs(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![];
        for (i, tab) in self.controller.tabs.iter().enumerate() {
            let tab_name = format!(" {} ", tab.name);
            let style = if i == self.controller.current_tab {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            spans.push(Span::styled(tab_name, style));
            if i < self.controller.tabs.len() - 1 {
                spans.push(Span::raw(" "));
            }
        }

        let tabs = Paragraph::new(Line::from(spans));
        frame.render_widget(tabs, area);
    }

    fn draw_connection_list(&self, frame: &mut Frame, area: Rect) {
        let tab = self.controller.current_tab();
        let items: Vec<ListItem> = tab
            .connections
            .iter()
            .map(|conn| {
                let line = format!(
                    "{} ({}) - {}:{}",
                    conn.name,
                    conn.db_type.as_str(),
                    conn.host,
                    conn.port
                );
                ListItem::new(line)
            })
            .collect();

        let padded_area = Rect {
            x: area.x + PADDING_HORIZONTAL,
            y: area.y + PADDING_VERTICAL,
            width: area.width.saturating_sub(PADDING_HORIZONTAL * 2),
            height: area.height.saturating_sub(PADDING_VERTICAL),
        };

        // Calculate centered area.
        let list_width = 60.min(padded_area.width);
        let list_height = tab.connections.len() as u16;

        let list_x = padded_area.x + (padded_area.width.saturating_sub(list_width)) / 2;
        let list_y = padded_area.y + (padded_area.height.saturating_sub(list_height)) / 2;

        let list_area = Rect {
            x: list_x,
            y: list_y,
            width: list_width,
            height: list_height,
        };

        let list = List::new(items)
            .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
            .highlight_symbol(">> ");

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

        let content = if let Some(conn) = tab.connections.get(tab.selected_index) {
            vec![Line::from(vec![
                Span::styled(
                    "Host: ",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}:{}", conn.host, conn.port),
                    Style::default().fg(Color::Yellow),
                ),
            ])]
        } else {
            vec![Line::from("No connection")]
        };

        let sidebar = Paragraph::new(content).block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_type(BorderType::Thick)
                .border_style(Style::default().fg(Color::DarkGray))
                .padding(Padding::new(
                    PADDING_HORIZONTAL,
                    PADDING_HORIZONTAL,
                    PADDING_VERTICAL,
                    PADDING_VERTICAL,
                )),
        );
        frame.render_widget(sidebar, area);
    }

    fn draw_query_input(&mut self, frame: &mut Frame, area: Rect) {
        self.controller.query_textarea.set_block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_type(BorderType::Thick)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        self.controller.query_textarea.set_line_number_style(Style::default().fg(Color::DarkGray));
        frame.render_widget(&self.controller.query_textarea, area);
    }

    fn draw_query_output(&self, frame: &mut Frame, area: Rect) {
        let output =
            Paragraph::new("Query results...").block(Block::default().padding(Padding::new(
                PADDING_HORIZONTAL,
                PADDING_HORIZONTAL,
                PADDING_VERTICAL,
                PADDING_VERTICAL,
            )));
        frame.render_widget(output, area);
    }
}
