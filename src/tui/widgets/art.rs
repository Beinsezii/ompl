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

impl ContainedWidget for Art {
    fn draw(&mut self, frame: &mut ratatui::Frame, stylesheet: super::StyleSheet) {
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        let Some(library) = self.lib_weak.upgrade() else { return };
        let (w, h) = (self.area.width as usize, self.area.height as usize * 2);
        if let Some(thumbnail) = library.thumbnail(w, h) {
            let fill = stylesheet.base.bg.unwrap_or(Color::Black);
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
                                if bg == fg {
                                    // All alpha only draw space to support
                                    // terminal emulator transparency
                                    if fg[3] == 0 {
                                        style = style.bg(fill);
                                        " "
                                    // Uniform solid only draw block to avoid
                                    // terminal emulator transparency
                                    } else {
                                        style = style.fg(Color::Rgb(fg[0], fg[1], fg[2]));
                                        "█"
                                    }
                                } else {
                                    style = style.fg(Color::Rgb(fg[0], fg[1], fg[2])).bg(Color::Rgb(bg[0], bg[1], bg[2]));
                                    "▀"
                                }
                            } else if fg[3] == 0 {
                                style = style.bg(fill);
                                " "
                            } else {
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
