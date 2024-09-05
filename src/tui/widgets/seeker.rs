#![warn(missing_docs)]

use super::{Action, Clickable, ContainedWidget, StyleSheet};
use crate::library::{Library, Track};

use std::sync::{Arc, Weak};
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Sparkline, Widget};

pub struct Seeker {
    lib_weak: Weak<Library>,
    previous: Option<u16>,
    area: Rect,
    sparkwave: Option<(usize, Arc<Track>, Vec<u64>)>,
}

impl Seeker {
    pub fn new(library: &Arc<Library>) -> Self {
        Self {
            lib_weak: Arc::downgrade(library),
            previous: None,
            area: Rect::default(),
            sparkwave: None,
        }
    }
}

impl ContainedWidget for Seeker {
    fn render(&mut self, buf: &mut Buffer, area: Rect, stylesheet: StyleSheet) {
        self.area = area;
        let Some(library) = self.lib_weak.upgrade() else { return };
        let Some(seekable) = library.seekable() else { return };
        let Some(track) = library.track_get() else { return };
        if seekable {
            let sparklen: usize = area.width.into();
            let Some((start, end)) = library.times() else { return };
            let ratio = start.as_secs_f32() / end.as_secs_f32();
            let split = (sparklen as f32 * ratio + 0.5).round() as u16;

            let Some(sparkwave) = (if self.sparkwave.as_ref().map(|(l, t, _v)| (*l, t)) == Some((sparklen, &track)) {
                self.sparkwave.as_ref().map(|t| &t.2)
            } else {
                self.sparkwave = None;
                if let Some(waveform) = library.waveform(sparklen) {
                    let new_sparkwave = waveform.into_iter().map(|n| (n * u16::MAX as f32) as u64).collect::<Vec<u64>>();
                    self.sparkwave = Some((sparklen, track, new_sparkwave));
                    self.sparkwave.as_ref().map(|t| &t.2)
                } else {
                    None
                }
            }) else {
                return;
            };

            let max = *sparkwave.iter().max().unwrap();

            let [past, future] = *Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(split), Constraint::Length(area.width.saturating_sub(split))])
                .split(area)
            else {
                unreachable!("Sparkline past/future split was not 2")
            };

            Sparkline::default()
                .max(max)
                .data(&sparkwave[..past.width as usize])
                .style(stylesheet.active)
                .render(past, buf);

            Sparkline::default()
                .max(max)
                .data(&sparkwave[past.width as usize..])
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
                MouseEventKind::Down(MouseButton::Left) => {
                    library.seek(Duration::from_secs_f32(
                        (event.column as f32 / self.area.width as f32) * times.1.as_secs_f32(),
                    ));
                    self.previous = Some(event.column)
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(previous) = self.previous {
                        if event.column != previous {
                            library.seek(Duration::from_secs_f32(
                                (event.column as f32 / self.area.width as f32) * times.1.as_secs_f32(),
                            ));
                            self.previous = Some(event.column)
                        }
                    }
                }
                _ => (),
            }
        } else {
            self.previous = None
        }

        none
    }
}
