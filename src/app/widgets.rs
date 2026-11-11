use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::theme::{ACCENT, BLUE, SURFACE, TEXT_MUTED, focus_colors};

pub const PADDING: u16 = 1;

/// Create a styled panel block with title (for main UI panels)
pub fn panel_block(title: &str, is_focused: bool, borders: Borders) -> Block<'_> {
    let (border_color, bg_color) = focus_colors(is_focused);
    // An unfocused title stays readable; only the frame around it recedes.
    let title_color = if is_focused { BLUE } else { TEXT_MUTED };

    Block::default()
        .title(Line::from(vec![
            Span::styled("── ", Style::default().fg(border_color)),
            Span::styled(
                title,
                Style::default()
                    .fg(title_color)
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

/// Truncate `s` to at most `max_width` terminal columns, ending in an
/// ellipsis when something was cut. Widths are display columns, not bytes or
/// chars, so wide (CJK) and multi-byte text stays aligned.
pub fn truncate_str(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    let (budget, ellipsis) = if max_width <= 3 {
        (max_width, "")
    } else {
        (max_width - 3, "...")
    };
    let mut truncated = take_width(s, budget).to_string();
    truncated.push_str(ellipsis);
    truncated
}

/// The longest prefix of `s` that fits in `max_width` terminal columns.
fn take_width(s: &str, max_width: usize) -> &str {
    let mut used = 0;
    for (index, ch) in s.char_indices() {
        used += ch.width().unwrap_or(0);
        if used > max_width {
            return &s[..index];
        }
    }
    s
}

/// Pad `s` with spaces on the right to `width` terminal columns.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let padding = width.saturating_sub(s.width());
    format!("{}{}", s, " ".repeat(padding))
}

/// Wrap text to fit within a given width (in terminal columns), breaking on
/// word boundaries when possible
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    for line in text.lines() {
        if line.width() <= max_width {
            lines.push(line.to_string());
            continue;
        }

        let mut current = String::new();
        for word in line.split_whitespace() {
            let needed = if current.is_empty() {
                word.width()
            } else {
                current.width() + 1 + word.width()
            };

            if needed <= max_width {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            } else if word.width() > max_width {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                // Too long for one line: cut it into pieces that each fit.
                let mut rest = word;
                while !rest.is_empty() {
                    let mut piece = take_width(rest, max_width);
                    if piece.is_empty() {
                        // A single character wider than the line still has to go somewhere.
                        let first = rest.chars().next().map_or(0, char::len_utf8);
                        piece = &rest[..first];
                    }
                    lines.push(piece.to_string());
                    rest = &rest[piece.len()..];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_counts_columns_not_bytes() {
        // 5 chars, 6 bytes, 5 columns: fits as is.
        assert_eq!(truncate_str("héllo", 5), "héllo");
        // Each CJK character is two columns wide.
        assert_eq!(truncate_str("日本語テキスト", 14), "日本語テキスト");
        assert_eq!(truncate_str("日本語テキスト", 8), "日本...");
        assert_eq!(truncate_str("日本語テキスト", 8).width(), 7);
    }

    #[test]
    fn truncate_ascii_is_unchanged() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
        assert_eq!(truncate_str("hello", 3), "hel");
        assert_eq!(truncate_str("hello", 0), "");
    }

    #[test]
    fn pad_aligns_wide_text() {
        assert_eq!(pad_to_width("id", 4), "id  ");
        assert_eq!(pad_to_width("名前", 6).width(), 6);
        assert_eq!(pad_to_width("too long", 3), "too long");
    }

    #[test]
    fn wrap_measures_columns() {
        // 12 columns of CJK do not fit in 10 even though it is only 6 chars.
        assert_eq!(wrap_text("日本語日本語", 10), ["日本語日本", "語"]);
        assert_eq!(wrap_text("aé bé cé", 5), ["aé bé", "cé"]);
        for line in wrap_text("naïve café über straße", 6) {
            assert!(line.width() <= 6, "{line:?} is too wide");
        }
    }
}
