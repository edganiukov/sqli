use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

use super::theme::{ACCENT, BLUE, SURFACE, SURFACE_DIM, TEXT_DIM};

pub const PADDING: u16 = 1;

/// Create a styled panel block with title (for main UI panels)
pub fn panel_block(title: &str, is_focused: bool, borders: Borders) -> Block<'_> {
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

/// Create a popup dialog block with rounded borders
pub fn popup_block(title: &str, border_color: Color) -> Block<'_> {
    Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(SURFACE))
}

/// Calculate centered popup area with fixed dimensions
pub fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let popup_width = width.min(area.width);
    let popup_height = height.min(area.height);
    Rect {
        x: (area.width.saturating_sub(popup_width)) / 2,
        y: (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    }
}

/// Calculate centered popup area with percentage-based dimensions
pub fn centered_rect_pct(
    area: Rect,
    width_pct: f32,
    height_pct: f32,
    min_w: u16,
    min_h: u16,
) -> Rect {
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

/// Truncate string to max width with ellipsis
pub fn truncate_str(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        s.chars().take(max_width).collect()
    } else {
        let truncated: String = s.chars().take(max_width - 3).collect();
        format!("{}...", truncated)
    }
}

/// Wrap text to fit within a given width, breaking on word boundaries when possible
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
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
            } else if word.chars().count() > max_width {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                // Split by characters, not bytes, to avoid breaking UTF-8
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(max_width) {
                    lines.push(chunk.iter().collect());
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
