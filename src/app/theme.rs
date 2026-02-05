use ratatui::style::{Color, Modifier, Style};

// Color scheme
pub const ACCENT: Color = Color::Rgb(230, 150, 60); // Orange accent (Pi style)
pub const BLUE: Color = Color::Rgb(100, 160, 220); // Bright blue
pub const SUCCESS: Color = Color::Rgb(80, 200, 120); // Bright green
pub const WARNING: Color = Color::Rgb(240, 180, 80); // Warm yellow
pub const SURFACE: Color = Color::Rgb(20, 20, 24); // Dark background (focused)
pub const SURFACE_DIM: Color = Color::Rgb(40, 40, 48); // Lighter (unfocused)
pub const TEXT: Color = Color::Rgb(230, 230, 235); // Bright white text
pub const TEXT_DIM: Color = Color::Rgb(130, 130, 140); // Dimmed text
pub const HIGHLIGHT: Color = Color::Rgb(60, 60, 75); // Selection highlight

// Style helpers to reduce repetition
pub fn dim() -> Style {
    Style::default().fg(TEXT_DIM)
}

pub fn text() -> Style {
    Style::default().fg(TEXT)
}

pub fn bold(color: Color) -> Style {
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub fn accent_bold() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Get colors based on focus state: (border_color, background_color)
pub fn focus_colors(is_focused: bool) -> (Color, Color) {
    if is_focused {
        (BLUE, SURFACE)
    } else {
        (TEXT_DIM, SURFACE_DIM)
    }
}
