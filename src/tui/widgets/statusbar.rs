use super::{Clickable, ContainedWidget};
use crate::library::Library;

use std::sync::{Arc, Weak};

use crossterm::event;
use tui::{layout::Rect, widgets::Paragraph};

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
        theme: super::Theme,
    ) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };

        frame.render_widget(
            Paragraph::new(format!(
                //1234567 8901234567890123
                " -- {:.2} ++ | :< # {} >: | {}",
                library.volume_get(),
                if library.playing() { "::" } else { "/>" },
                library
                    .track_get()
                    .map(|t| t.tagstring(&self.tagstring))
                    .unwrap_or("???".to_string())
            ))
            .style(theme.base),
            self.area,
        );
        if library.stopped() && self.area.width > 17 {
            frame.render_widget(
                Paragraph::new("#").style(theme.base_hi),
                Rect {
                    x: self.area.x + 17,
                    width: 1,
                    ..self.area
                },
            )
        }
    }
}

impl Clickable for StatusBar {
    fn process_event(&mut self, event: event::MouseEvent) -> bool {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return false,
        };

        if event.kind == event::MouseEventKind::Down(event::MouseButton::Left) {
            if self
                .area
                .intersects(Rect::new(event.column, event.row, 1, 1))
            {
                match event.column {
                    1..=2 => library.volume_sub(0.05),
                    9..=10 => library.volume_add(0.05),
                    14..=15 => library.previous(),
                    17..=17 => library.stop(),
                    19..=20 => library.play_pause(),
                    22..=23 => library.next(),
                    _ => (),
                }
            }
        }
        false
    }
}
