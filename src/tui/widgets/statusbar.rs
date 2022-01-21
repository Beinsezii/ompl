use crate::library::Library;

use super::ClickableWidget;
use crossterm::event;
use tui::{buffer::Buffer, layout::Rect, widgets::Widget};

#[derive(Clone, Debug, PartialEq)]
pub struct StatusBar {
    volume: f32,
    playing: bool,
    tagstring: String,
}

impl StatusBar {
    pub fn new<T: AsRef<Library>, U: Into<String>>(library: T, tagstring: U) -> Self {
        let library: &Library = library.as_ref();
        Self {
            volume: library.volume_get(),
            playing: library.playing(),
            tagstring: library
                .track_get()
                .map(|t| t.tagstring(tagstring))
                .unwrap_or(String::from("???")),
        }
    }
}

impl Widget for StatusBar {
    fn render(self, area: Rect, buff: &mut Buffer) {
        buff.set_string(
            area.x,
            area.y,
            format!(
                //1234567 8901234567890123
                " -- {:.2} ++ | :< # {} >: | {}",
                self.volume,
                if self.playing { "::" } else { "/>" },
                self.tagstring
            ),
            tui::style::Style::default(),
        )
    }
}

impl ClickableWidget for StatusBar {
    fn process_event<T: AsRef<Library>>(event: event::MouseEvent, area: Rect, library: T) -> bool {
        let library: &Library = library.as_ref();

        if event.kind == event::MouseEventKind::Down(event::MouseButton::Left) {
            if area.intersects(Rect::new(event.column, event.row, 1, 1)) {
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
