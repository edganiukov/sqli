use ratatui::style::{Color, Modifier, Style};

// Color scheme
pub const ACCENT: Color = Color::Rgb(235, 156, 66); // Orange accent (Pi style)
pub const BLUE: Color = Color::Rgb(108, 166, 224); // Bright blue (focused border, hints)
pub const SUCCESS: Color = Color::Rgb(118, 201, 139); // Bright green
pub const WARNING: Color = Color::Rgb(228, 186, 96); // Warm yellow (header text)
pub const SURFACE: Color = Color::Rgb(20, 21, 26); // Dark background (focused)
pub const SURFACE_DIM: Color = Color::Rgb(38, 40, 48); // Lighter (unfocused, header, status)
pub const TEXT: Color = Color::Rgb(228, 229, 235); // Bright white text
pub const TEXT_DIM: Color = Color::Rgb(128, 132, 146); // Dimmed text

// Selection layers, in increasing prominence. Kept visually distinct from each
// other and from SURFACE_DIM so the focused column never reads as an idle pane.
pub const HIGHLIGHT: Color = Color::Rgb(58, 60, 74); // List selection / cursor row band
pub const COLUMN_HIGHLIGHT: Color = Color::Rgb(36, 52, 86); // Selected column band (cool blue)
pub const SELECTION: Color = Color::Rgb(64, 104, 162); // Active cell / visual selection

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
