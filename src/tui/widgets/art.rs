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
    fn draw(&mut self, frame: &mut ratatui::prelude::Frame, stylesheet: super::StyleSheet) {
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        let Some(library) = self.lib_weak.upgrade() else { return };
        let (w, h) = (self.area.width as usize, self.area.height as usize * 2);
        if let Some(thumbnail) = library.thumbnail(w, h) {
            let lines: Vec<Line> = thumbnail
                .chunks(2)
                .map(|rows| {
                    let empty = Default::default();
                    let mut bgiter = rows.get(1).unwrap_or(&empty).into_iter();

                    rows[0]
                        .iter()
                        .map(|[r, g, b, _a]| Span {
                            content: "▀".into(),
                            style: Style::default()
                                .fg(Color::Rgb(*r, *g, *b))
                                .bg(if let Some([r, g, b, _a]) = bgiter.next() {
                                    Color::Rgb(*r, *g, *b)
                                } else {
                                    stylesheet.base.bg.unwrap_or(Color::Black)
                                }),
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
