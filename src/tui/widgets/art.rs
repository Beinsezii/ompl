#![warn(missing_docs)]

use super::{Action, Clickable, ContainedWidget, StyleSheet};

use crate::{library::Library, logging::*};

use std::sync::{Arc, Weak};

use ratatui::crossterm::{
    event::{MouseButton, MouseEvent, MouseEventKind},
    style::available_color_count,
};
use ratatui::layout::Rect;
use ratatui::prelude::Buffer;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

// ### FNs {{{

// blend alpha onto black canvas
fn alpha([r, g, b, a]: [u8; 4]) -> [u8; 3] {
    if a == u8::MAX {
        return [r, g, b];
    } else if a == 0 {
        return [0, 0, 0];
    };
    let a = a as f32 / u8::MAX as f32;
    [r, g, b].map(|c| (c as f32 * a).round() as u8)
}
fn pixel2col(rgba: [u8; 4], quantize: bool) -> Color {
    let [r, g, b] = alpha(rgba);
    if quantize {
        let rgb = [r, g, b].map(|c| c.min(254) / (u8::MAX / 3));
        // prioritize green > red > blue
        match rgb {
            [0, 0, 0] => Color::Black,
            [0, 0, 1] => Color::Blue,
            [0, 0, 2] => Color::LightBlue,
            [0, 1, 0] => Color::Green,
            [0, 1, 1] => Color::Cyan,
            [0, 1, 2] => Color::Cyan,
            [0, 2, 0] => Color::LightGreen,
            [0, 2, 1] => Color::LightCyan,
            [0, 2, 2] => Color::LightCyan,
            [1, 0, 0] => Color::Red,
            [1, 0, 1] => Color::Magenta,
            [1, 0, 2] => Color::Magenta,
            [1, 1, 0] => Color::Yellow,
            [1, 1, 1] => Color::DarkGray,
            [1, 1, 2] => Color::LightBlue,
            [1, 2, 0] => Color::LightYellow,
            [1, 2, 1] => Color::LightGreen,
            [1, 2, 2] => Color::LightCyan,
            [2, 0, 0] => Color::LightRed,
            [2, 0, 1] => Color::LightMagenta,
            [2, 0, 2] => Color::LightMagenta,
            [2, 1, 0] => Color::Yellow,
            [2, 1, 1] => Color::LightRed,
            [2, 1, 2] => Color::LightMagenta,
            [2, 2, 0] => Color::LightYellow,
            [2, 2, 1] => Color::LightYellow,
            [2, 2, 2] => Color::White,

            _ => {
                error!("### TUI::Art bad color array {:?} ###", rgb);
                return Color::Magenta;
            }
        }
    } else {
        Color::Rgb(r, g, b)
    }
}

//}}}

pub struct Art {
    lib_weak: Weak<Library>,
    pub area: Rect,
}

impl Art {
    pub fn new(library: &Arc<Library>) -> Self {
        Self {
            lib_weak: Arc::downgrade(library),
            area: Rect::default(),
        }
    }
}

impl ContainedWidget for Art {
    // {{{
    fn render(&mut self, buf: &mut Buffer, area: Rect, stylesheet: StyleSheet)
    where
        Self: Sized,
    {
        self.area = area;
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        let Some(library) = self.lib_weak.upgrade() else { return };
        let (w, h) = (self.area.width as usize, self.area.height as usize * 2);
        if let Some(thumbnail) = library.thumbnail(w, h) {
            let quantize = available_color_count() <= 16;
            let fill = stylesheet.base.bg.unwrap_or(Color::Black);
            // clip at 5% or less
            const CLIP: u8 = u8::MAX / 20;
            let lines: Vec<Line> = thumbnail
                .chunks(2)
                .map(|rows| {
                    let empty = Default::default();
                    let mut bgiter = rows.get(1).unwrap_or(&empty).into_iter();

                    rows[0]
                        .iter()
                        .map(|fg| {
                            let mut style = Style::default();

                            // no alpha blending because the 16 terminal colors aren't readable
                            let content = if let Some(bg) = bgiter.next() {
                                // All alpha only draw space to support
                                // terminal emulator transparency
                                if bg[3] <= CLIP && fg[3] <= CLIP {
                                    style = style.bg(fill);
                                    " "
                                // Uniform solid only draw block to avoid
                                // terminal emulator transparency
                                } else if bg == fg {
                                    style = style.fg(pixel2col(*fg, quantize));
                                    "█"
                                } else {
                                    style = style.fg(pixel2col(*fg, quantize)).bg(pixel2col(*bg, quantize));
                                    "▀"
                                }
                            } else if fg[3] <= CLIP {
                                style = style.bg(fill);
                                " "
                            } else {
                                style = style.fg(pixel2col(*fg, quantize));
                                "▀"
                            };

                            Span {
                                content: content.into(),
                                style,
                            }
                        })
                        .collect::<Vec<Span>>()
                        .into()
                })
                .collect();

            Paragraph::new(lines).render(self.area, buf)
        } else {
            Block::new().style(stylesheet.base).borders(Borders::ALL).render(self.area, buf)
        }
    }
} // }}}

impl Clickable for Art {
    fn process_event(&mut self, event: MouseEvent) -> Action {
        if event.kind == MouseEventKind::Down(MouseButton::Left) && self.area.intersects(Rect::new(event.column, event.row, 1, 1)) {
            Action::ArtView
        } else {
            Action::None
        }
    }
}
