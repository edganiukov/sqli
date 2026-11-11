use ratatui::style::{Color, Modifier, Style};

// Color scheme
pub const ACCENT: Color = Color::Rgb(235, 156, 66); // Orange accent (Pi style)
pub const BLUE: Color = Color::Rgb(108, 166, 224); // Bright blue (focused border, hints)
pub const SUCCESS: Color = Color::Rgb(118, 201, 139); // Bright green
pub const WARNING: Color = Color::Rgb(228, 186, 96); // Warm yellow (header text)
pub const ERROR: Color = Color::Rgb(236, 112, 112); // Soft red (failures)

// Backgrounds, darkest first. Panes stay dark whether focused or not, so an
// unfocused pane keeps its contrast; the lighter tone is reserved for chrome.
pub const SURFACE: Color = Color::Rgb(20, 21, 26); // Focused pane, popups
pub const SURFACE_ALT: Color = Color::Rgb(27, 29, 36); // Unfocused pane
pub const SURFACE_DIM: Color = Color::Rgb(38, 40, 48); // Chrome: bars, table header

pub const TEXT: Color = Color::Rgb(228, 229, 235); // Bright white text
pub const TEXT_MUTED: Color = Color::Rgb(170, 176, 196); // Secondary text
pub const TEXT_DIM: Color = Color::Rgb(128, 132, 146); // Dimmed text
pub const BORDER: Color = Color::Rgb(74, 80, 102); // Unfocused pane borders

// Selection layers, in increasing prominence. Kept visually distinct from each
// other and from the pane backgrounds so the focused column never reads as idle.
pub const HIGHLIGHT_DIM: Color = Color::Rgb(43, 46, 59); // Selection in an unfocused pane
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
        (BORDER, SURFACE_ALT)
    }
}
