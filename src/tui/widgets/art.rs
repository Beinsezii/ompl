use super::ContainedWidget;

use crate::library::Library;

use std::sync::Weak;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

pub struct Art {
    pub lib_weak: Weak<Library>,
    pub area: Rect,
}

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

impl ContainedWidget for Art {
    fn draw(&mut self, frame: &mut ratatui::Frame, stylesheet: super::StyleSheet) {
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        let Some(library) = self.lib_weak.upgrade() else { return };
        let (w, h) = (self.area.width as usize, self.area.height as usize * 2);
        if let Some(thumbnail) = library.thumbnail(w, h) {
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
                                    let fg = alpha(*fg);
                                    style = style.fg(Color::Rgb(fg[0], fg[1], fg[2]));
                                    "█"
                                } else {
                                    let (fg, bg) = (alpha(*fg), alpha(*bg));
                                    style = style.fg(Color::Rgb(fg[0], fg[1], fg[2])).bg(Color::Rgb(bg[0], bg[1], bg[2]));
                                    "▀"
                                }
                            } else if fg[3] <= CLIP {
                                style = style.bg(fill);
                                " "
                            } else {
                                let fg = alpha(*fg);
                                style = style.fg(Color::Rgb(fg[0], fg[1], fg[2]));
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

            frame.render_widget(Paragraph::new(lines), self.area)
        } else {
            frame.render_widget(Block::new().style(stylesheet.base).borders(Borders::ALL), self.area)
        }
    }
}