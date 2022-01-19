use tui::style::{Style, Color, Modifier};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub base: Style,
    pub base_hi: Style,
    pub active: Style,
    pub active_hi: Style,
    pub mod_select: Style,
    pub mod_select_active: Style,
}

impl Theme {
    pub fn new(accent: Color) -> Self {
        Self {
            base: Style::default(),
            base_hi: Style::default().fg(Color::Black).bg(Color::White),
            active: Style::default().fg(accent),
            active_hi: Style::default().fg(Color::Black).bg(accent),
            mod_select: Style::default().add_modifier(Modifier::UNDERLINED),
            mod_select_active: Style::default().add_modifier(Modifier::BOLD),
        }
    }
}

