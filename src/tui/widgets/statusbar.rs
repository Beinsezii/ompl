#![warn(missing_docs)]

use super::{Action, Clickable, ContainedWidget};
use crate::library::Library;

use std::sync::{Arc, Weak};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

#[derive(Clone)]
pub struct StatusBar {
    lib_weak: Weak<Library>,
    pub area: Rect,
}

impl StatusBar {
    pub fn new(library: &Arc<Library>) -> Self {
        Self {
            lib_weak: Arc::downgrade(library),
            area: Rect::default(),
        }
    }
}

impl ContainedWidget for StatusBar {
    fn draw(&mut self, frame: &mut ratatui::terminal::Frame, stylesheet: super::StyleSheet) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::from(format!(
                    " -- {:.2} ++ | ({}) ",
                    library.volume_get(),
                    match library.repeat_get() {
                        None =>
                            if library.shuffle_get() {
                                '-'
                            } else {
                                'X'
                            },
                        Some(false) => '1',
                        Some(true) => ' ',
                    }
                )),
                Span::styled("><", if library.shuffle_get() { stylesheet.base_hi } else { stylesheet.base }),
                Span::from(" :< "),
                Span::styled("#", if library.stopped() { stylesheet.base_hi } else { stylesheet.base }),
                Span::from(format!(
                    " {} >: | {}{}",
                    if library.playing() { "::" } else { "/>" },
                    library
                        .track_get()
                        .map(|t| t.tagstring(library.statusline_get()))
                        .unwrap_or("???".to_string()),
                    // Not sure if I like this at the end yet.
                    match library.times() {
                        Some((cur, tot)) => format!(
                            " | {:02.0}:{:02.0} / {:02.0}:{:02.0}",
                            (cur.as_secs_f32() / 60.0).floor(),
                            (cur.as_secs_f32() % 60.0).floor(),
                            (tot.as_secs_f32() / 60.0).floor(),
                            (tot.as_secs_f32() % 60.0).floor(),
                        ),
                        None => String::new(),
                    }
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

        if let MouseEventKind::Down(button) = event.kind {
            if self.area.intersects(Rect::new(event.column, event.row, 1, 1)) {
                // 123456789 123456789 123456789 1234  +123456789 123456
                // -- 0.12 ++ | (1) >< :< # /> >: | {..} | 00:00 / 00:00
                match button {
                    MouseButton::Left => match event.column {
                        1..=2 => library.volume_add(-0.05),
                        9..=10 => library.volume_add(0.05),
                        14..=16 => library.repeat_toggle(),
                        18..=19 => library.shuffle_toggle(),
                        21..=22 => library.previous(),
                        24..=24 => library.stop(),
                        26..=27 => library.play_pause(),
                        29..=30 => library.next(),
                        _ => (),
                    },
                    MouseButton::Right => {
                        let len_sl = library.statusline_get_format().len() as u16;
                        if event.column >= 34 && event.column < 34 + len_sl {
                            return Action::Statusline;
                        } else if event.column >= 37 + len_sl && event.column < 50 + len_sl {
                            return Action::SeekTo;
                        }
                    }
                    _ => (),
                }
            }
        } else if event.kind == MouseEventKind::ScrollUp || event.kind == MouseEventKind::ScrollDown {
            if self.area.intersects(Rect::new(event.column, event.row, 1, 1)) {
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
        Action::None
    }
}
