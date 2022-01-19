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
                //   456789 012345678901234
                "Vol --{:.2}++ | :< # {} >: | {}",
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
                    4..=5 => library.volume_sub(0.05),
                    10..=11 => library.volume_add(0.05),
                    15..=16 => library.previous(),
                    18..=18 => library.stop(),
                    20..=21 => library.play_pause(),
                    23..=24 => library.next(),
                    _ => (),
                }
            }
        }
        false
    }
}
