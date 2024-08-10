use super::StyleSheet;

use crate::library::Library;

use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::prelude::Buffer;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

pub struct Art {
    pub library: Arc<Library>,
    pub stylesheet: StyleSheet,
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

impl Widget for Art {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let (w, h) = (area.width as usize, area.height as usize * 2);
        if let Some(thumbnail) = self.library.thumbnail(w, h) {
            let fill = self.stylesheet.base.bg.unwrap_or(Color::Black);
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

            Paragraph::new(lines).render(area, buf)
        } else {
            Block::new().style(self.stylesheet.base).borders(Borders::ALL).render(area, buf)
        }
    }
}
