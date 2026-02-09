use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::completion::{Suggestion, SuggestionKind};
use crate::controller::Controller;
use crate::db::QueryResult;
use crate::templates::TemplateScope;

use super::theme::*;
use super::widgets::*;

/// Draw template list popup
pub fn draw_template_list(
    frame: &mut Frame,
    controller: &Controller,
    selected: usize,
    filter: &str,
    searching: bool,
) {
    let area = frame.area();
    let all_templates = &controller.template_list_cache;

    // Filter templates by search filter
    let filter_lower = filter.to_lowercase();
    let templates: Vec<_> = all_templates
        .iter()
        .filter(|t| t.name.to_lowercase().contains(&filter_lower))
        .collect();

    let popup_area = centered_rect_pct(area, 0.6, 0.7, 40, 10);
    frame.render_widget(Clear, popup_area);

    // Render block first and get inner area
    let title = format!("Templates ({})", all_templates.len());
    let block = popup_block(&title, BLUE);
    let block_inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Reserve space for help line at bottom
    let inner_area = Rect {
        x: block_inner.x,
        y: block_inner.y,
        width: block_inner.width,
        height: block_inner.height.saturating_sub(2),
    };
    let help_area = Rect {
        x: block_inner.x,
        y: block_inner.y + block_inner.height.saturating_sub(1),
        width: block_inner.width,
        height: 1,
    };

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
                TemplateScope::Global => "global".to_string(),
                TemplateScope::Connections(names) => names.join(","),
            };
            let preview: String = t
                .query
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(50)
                .collect();

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(&t.name, bold(TEXT)),
                    Span::styled(format!("  ({})", scope_str), dim()),
                ]),
                Line::from(Span::styled(format!("  {}", preview), dim())),
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
        let search_paragraph = Paragraph::new(search_text).style(text());

        frame.render_widget(search_paragraph, search_area);
    }

    // Help line at bottom
    let help = Line::from(vec![
        Span::styled("/", Style::default().fg(TEXT)),
        Span::styled(" search  ", dim()),
        Span::styled("^G", Style::default().fg(TEXT)),
        Span::styled(" edit  ", dim()),
        Span::styled("^D", Style::default().fg(TEXT)),
        Span::styled(" delete  ", dim()),
        Span::styled("Esc", Style::default().fg(TEXT)),
        Span::styled(" close", dim()),
    ]);
    frame.render_widget(Paragraph::new(help).alignment(Alignment::Center), help_area);
}

/// Draw save template popup
pub fn draw_save_template(
    frame: &mut Frame,
    name: &str,
    connections: &str,
    editing_connections: bool,
) {
    let area = frame.area();
    let popup_area = centered_rect(area, 50, 10);
    frame.render_widget(Clear, popup_area);

    let block = popup_block("Save Template", BLUE);
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Name label
            Constraint::Length(1), // Name input
            Constraint::Length(1), // Connections label
            Constraint::Length(1), // Connections input
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Help
        ])
        .margin(1)
        .split(inner);

    let name_focused = !editing_connections;

    // Name label and input
    frame.render_widget(
        Paragraph::new("Template name:").style(Style::default().fg(if name_focused {
            TEXT
        } else {
            TEXT_DIM
        })),
        chunks[0],
    );

    let name_display = if name_focused {
        format!("{}_", name)
    } else {
        name.to_string()
    };
    frame.render_widget(
        Paragraph::new(name_display).style(Style::default().fg(TEXT).bg(if name_focused {
            SURFACE_DIM
        } else {
            SURFACE
        })),
        chunks[1],
    );

    // Connections label and input
    frame.render_widget(
        Paragraph::new("Connections (empty = global):")
            .style(Style::default().fg(if editing_connections { TEXT } else { TEXT_DIM })),
        chunks[2],
    );

    let conn_display = if editing_connections {
        format!("{}_", connections)
    } else if connections.is_empty() {
        "(global)".to_string()
    } else {
        connections.to_string()
    };
    frame.render_widget(
        Paragraph::new(conn_display).style(
            Style::default()
                .fg(if connections.is_empty() && !editing_connections {
                    TEXT_DIM
                } else {
                    TEXT
                })
                .bg(if editing_connections {
                    SURFACE_DIM
                } else {
                    SURFACE
                }),
        ),
        chunks[3],
    );

    frame.render_widget(
        Paragraph::new("Tab/↑↓ switch field | Enter save | Esc cancel").style(dim()),
        chunks[5],
    );
}

/// Draw confirm delete popup
pub fn draw_confirm_delete(frame: &mut Frame, template_name: &str) {
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
            .style(text())
            .alignment(Alignment::Center),
        chunks[0],
    );
}

/// Draw record detail popup
pub fn draw_record_detail(
    frame: &mut Frame,
    controller: &Controller,
    row_index: usize,
    selected_field: usize,
    scroll: usize,
) {
    let tab = controller.current_tab();

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
        "Record {}/{}  ─  Field {}/{}",
        row_index + 1,
        rows.len(),
        selected_field + 1,
        columns.len()
    );
    let block = popup_block(&title, BLUE);
    let block_inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Reserve space for help line at bottom
    let inner = Rect {
        x: block_inner.x,
        y: block_inner.y,
        width: block_inner.width,
        height: block_inner.height.saturating_sub(2),
    };
    let help_area = Rect {
        x: block_inner.x,
        y: block_inner.y + block_inner.height.saturating_sub(1),
        width: block_inner.width,
        height: 1,
    };

    // Calculate the maximum field name width for alignment
    let max_name_width = columns.iter().map(|c| c.len()).max().unwrap_or(0);
    let left_padding = 2;
    let value_width = inner
        .width
        .saturating_sub(max_name_width as u16 + 4 + left_padding) as usize;

    // Build lines for each field, tracking which line each field starts at
    let mut lines: Vec<Line> = Vec::new();
    let mut field_start_lines: Vec<usize> = Vec::new();

    for (field_idx, (col, value)) in columns.iter().zip(row.iter()).enumerate() {
        field_start_lines.push(lines.len());

        let is_selected = field_idx == selected_field;
        let field_name = format!("{:<width$}", col, width = max_name_width);

        // Style based on selection
        let name_style = if is_selected {
            accent_bold()
        } else {
            Style::default().fg(WARNING)
        };
        let value_style = if is_selected {
            Style::default().fg(TEXT).bg(HIGHLIGHT)
        } else {
            text()
        };
        let empty_style = if is_selected {
            Style::default()
                .fg(TEXT_DIM)
                .bg(HIGHLIGHT)
                .add_modifier(Modifier::ITALIC)
        } else {
            Style::default().fg(TEXT_DIM).add_modifier(Modifier::ITALIC)
        };

        let pad = "  ";
        if value.is_empty() {
            lines.push(Line::from(vec![
                Span::raw(pad),
                Span::styled(field_name, name_style),
                Span::styled(" : ", dim()),
                Span::styled("(empty)", empty_style),
            ]));
        } else {
            // Wrap long values
            let value_lines = wrap_text(value, value_width);
            for (i, line_text) in value_lines.iter().enumerate() {
                if i == 0 {
                    lines.push(Line::from(vec![
                        Span::raw(pad),
                        Span::styled(field_name.clone(), name_style),
                        Span::styled(" : ", dim()),
                        Span::styled(line_text.clone(), value_style),
                    ]));
                } else {
                    // Continuation lines - indent to align with value
                    let indent = " ".repeat(max_name_width + 3 + pad.len());
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
        selected_line
    } else if selected_line >= scroll + visible_lines {
        selected_line.saturating_sub(visible_lines / 2)
    } else {
        scroll
    };

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
        frame.render_widget(Paragraph::new(scrollbar).style(dim()), scrollbar_area);
    }

    // Help line at bottom
    let help = Line::from(vec![
        Span::styled("j/k", Style::default().fg(TEXT)),
        Span::styled(" navigate  ", dim()),
        Span::styled("y", Style::default().fg(TEXT)),
        Span::styled(" copy value  ", dim()),
        Span::styled("Esc", Style::default().fg(TEXT)),
        Span::styled(" close", dim()),
    ]);
    frame.render_widget(Paragraph::new(help).alignment(Alignment::Center), help_area);
}

/// Draw completion popup
pub fn draw_completion(
    frame: &mut Frame,
    controller: &Controller,
    suggestions: &[Suggestion],
    selected: usize,
) {
    let area = frame.area();

    // Calculate popup position based on cursor
    let (cursor_row, cursor_col) = controller.current_tab().query_textarea.cursor();

    // Query panel starts after sidebar + tab bar (1 row)
    // Layout: sidebar is 40 cols (Constraint::Length(40) in draw_database_view)
    // Textarea has line numbers (~4 chars including padding)
    const SIDEBAR_WIDTH: u16 = 40;
    const LINE_NUMBER_WIDTH: u16 = 4;
    let base_x = SIDEBAR_WIDTH + LINE_NUMBER_WIDTH;
    let base_y = 2u16; // tab bar (1) + query block title (1)

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
                SuggestionKind::Keyword => Span::styled("k ", dim()),
                SuggestionKind::Table => Span::styled("t ", Style::default().fg(SUCCESS)),
                SuggestionKind::Column => Span::styled("c ", Style::default().fg(WARNING)),
            };
            ListItem::new(Line::from(vec![
                kind_indicator,
                Span::styled(&s.text, text()),
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
