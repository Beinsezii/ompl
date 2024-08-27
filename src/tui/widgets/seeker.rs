#![warn(missing_docs)]

use super::{Action, Clickable, ContainedWidget, StyleSheet};
use crate::library::Library;

use std::sync::{Arc, Weak};
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Sparkline, Widget};

pub struct Seeker {
    lib_weak: Weak<Library>,
    previous: u16,
    area: Rect,
}

impl Seeker {
    pub fn new(library: &Arc<Library>) -> Self {
        Self {
            lib_weak: Arc::downgrade(library),
            previous: u16::MAX,
            area: Rect::default(),
        }
    }
}

impl ContainedWidget for Seeker {
    fn render(&mut self, buf: &mut Buffer, area: Rect, stylesheet: StyleSheet) {
        self.area = area;
        let Some(library) = self.lib_weak.upgrade() else { return };
        let Some(seekable) = library.seekable() else { return };
        if seekable {
            let Some(waveform) = library.waveform(area.width.into()) else { return };
            let Some(times) = library.times() else { return };
            let data = waveform.into_iter().map(|n| (n * 1024.0) as u64).collect::<Vec<u64>>();
            let max = *data.iter().max().unwrap();

            let ratio = times.0.as_secs_f32() / times.1.as_secs_f32();
            let split = (area.width as f32 * ratio + 0.5).round() as u16;

            let [past, future] = *Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(split), Constraint::Length(area.width.saturating_sub(split))])
                .split(area)
            else {
                unreachable!()
            };

            Sparkline::default()
                .max(max)
                .data(&data[..past.width as usize])
                .style(stylesheet.active)
                .render(past, buf);

            Sparkline::default()
                .max(max)
                .data(&data[past.width as usize..])
                .style(stylesheet.base)
                .render(future, buf);
        } else {
            Sparkline::default().max(4).data(&vec![1; area.width.into()]).render(area, buf);
        }
    }
}

impl Clickable for Seeker {
    fn process_event(&mut self, event: MouseEvent) -> Action {
        let none = Action::None;
        let Some(library) = self.lib_weak.upgrade() else { return none };
        let Some(times) = library.times() else { return none };
        if self.area.intersects(Rect::new(event.column, event.row, 1, 1)) {
            match event.kind {
                MouseEventKind::Down(MouseButton::Left) => library.seek(Duration::from_secs_f32(
                    (event.column as f32 / self.area.width as f32) * times.1.as_secs_f32(),
                )),
                MouseEventKind::Drag(MouseButton::Left) => {
                    if event.column != self.previous {
                        library.seek(Duration::from_secs_f32(
                            (event.column as f32 / self.area.width as f32) * times.1.as_secs_f32(),
                        ))
                    }
                }
                _ => (),
            }
            self.previous = event.column
        }

        none
    }
}
