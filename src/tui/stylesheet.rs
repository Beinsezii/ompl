#![warn(missing_docs)]

use crate::library::Theme;
use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyleSheet {
    pub base: Style,
    pub base_sel: Style,
    pub base_hi: Style,
    pub base_hi_sel: Style,
    pub active: Style,
    pub active_sel: Style,
    pub active_hi: Style,
    pub active_hi_sel: Style,
}

const COLORMAP: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

impl From<crate::library::Color> for Color {
    fn from(value: crate::library::Color) -> Self {
        match value {
            crate::library::Color::None => Color::Reset,
            crate::library::Color::Term(n) => COLORMAP[n as usize],
            crate::library::Color::RGB(rgb) => Color::Rgb(rgb[0], rgb[1], rgb[2]),
        }
    }
}

impl From<Theme> for StyleSheet {
    fn from(value: Theme) -> Self {
        let fg = value.fg.into();
        let bg = value.bg.into();
        let acc = value.acc.into();
        let fg_alt = if bg == Color::Reset { Color::Black } else { bg };
        let bg_alt = if fg == Color::Reset { Color::White } else { fg };

        Self {
            base: Style::default().fg(fg).bg(bg),

            base_sel: Style::default().fg(fg).bg(bg).add_modifier(Modifier::UNDERLINED),

            base_hi: Style::default().fg(fg_alt).bg(bg_alt),

            base_hi_sel: Style::default().fg(fg_alt).bg(bg_alt).add_modifier(Modifier::UNDERLINED),

            active: Style::default().fg(acc).bg(bg),

            active_sel: Style::default()
                .fg(acc)
                .bg(bg)
                .add_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::BOLD),

            active_hi: Style::default().fg(fg_alt).bg(acc),

            active_hi_sel: Style::default()
                .fg(fg_alt)
                .bg(acc)
                .add_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::BOLD),
        }
    }
}
