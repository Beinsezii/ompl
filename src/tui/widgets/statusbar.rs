use super::{Clickable, ContainedWidget};
use crate::library::Library;

use std::sync::{Arc, Weak};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use tui::{
    layout::Rect,
    text::{Span, Spans},
    widgets::Paragraph,
};

#[derive(Clone)]
pub struct StatusBar {
    lib_weak: Weak<Library>,
    pub tagstring: String,
    pub area: Rect,
}

impl StatusBar {
    pub fn new<T: Into<String>>(library: &Arc<Library>, tagstring: T) -> Self {
        Self {
            lib_weak: Arc::downgrade(library),
            tagstring: tagstring.into(),
            area: Rect::default(),
        }
    }
}

impl ContainedWidget for StatusBar {
    fn draw<T: tui::backend::Backend>(
        &mut self,
        frame: &mut tui::terminal::Frame<T>,
        stylesheet: super::StyleSheet,
    ) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };

        frame.render_widget(
            Paragraph::new(Spans::from(vec![
                Span::from(format!(" -- {:.2} ++ | ", library.volume_get())),
                Span::styled(
                    "><",
                    if library.shuffle_get() {
                        stylesheet.base_hi
                    } else {
                        stylesheet.base
                    },
                ),
                Span::from(" :< "),
                Span::styled(
                    "#",
                    if library.stopped() {
                        stylesheet.base_hi
                    } else {
                        stylesheet.base
                    },
                ),
                Span::from(format!(
                    " {} >: | {}",
                    if library.playing() { "::" } else { "/>" },
                    library
                        .track_get()
                        .map(|t| t.tagstring(&self.tagstring))
                        .unwrap_or("???".to_string())
                )),
            ]))
            .style(stylesheet.base),
            self.area,
        );
    }
}

impl Clickable for StatusBar {
    fn process_event(&mut self, event: MouseEvent) -> super::Action {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return super::Action::None,
        };

        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if self
                .area
                .intersects(Rect::new(event.column, event.row, 1, 1))
            {
                // 123456789 1234567890123456
                // -- 0.12 ++ | >< :< # /> >: |
                match event.column {
                    1..=2 => library.volume_add(-0.05),
                    9..=10 => library.volume_add(0.05),
                    14..=15 => library.shuffle_toggle(),
                    17..=18 => library.previous(),
                    20..=20 => library.stop(),
                    22..=23 => library.play_pause(),
                    25..=26 => library.next(),
                    _ => (),
                }
            }
        } else if event.kind == MouseEventKind::ScrollUp || event.kind == MouseEventKind::ScrollDown
        {
            if self
                .area
                .intersects(Rect::new(event.column, event.row, 1, 1))
            {
                if event.column >= 1 && event.column <= 10 {
                    match event.kind {
                        MouseEventKind::ScrollDown => library.volume_add(-0.05),
                        MouseEventKind::ScrollUp => library.volume_add(0.05),
                        _ => (),
                    }
                } else if event.column >= 14 && event.column <= 26 {
                    match event.kind {
                        MouseEventKind::ScrollDown => library.next(),
                        MouseEventKind::ScrollUp => library.previous(),
                        _ => (),
                    }
                }
            }
        }
        super::Action::None
    }
}
