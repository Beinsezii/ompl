use tui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub base: Style,
    pub base_sel: Style,
    pub base_hi: Style,
    pub base_hi_sel: Style,
    pub active: Style,
    pub active_sel: Style,
    pub active_hi: Style,
    pub active_hi_sel: Style,
}

impl Theme {
    pub fn new(accent: Color) -> Self {
        Self {
            base: Style::default(),
            base_sel: Style::default().add_modifier(Modifier::UNDERLINED),
            base_hi: Style::default().fg(Color::Black).bg(Color::White),
            base_hi_sel: Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::UNDERLINED),
            active: Style::default().fg(accent),
            active_sel: Style::default()
                .fg(accent)
                .add_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::BOLD),
            active_hi: Style::default().fg(Color::Black).bg(accent),
            active_hi_sel: Style::default()
                .fg(Color::Black)
                .bg(accent)
                .add_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::BOLD),
        }
    }
}
