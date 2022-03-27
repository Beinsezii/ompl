use crate::library::Library;

use super::{scroll_down, scroll_up, ClickableStatefulWidget, Theme};

use crossterm::event;
use tui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{StatefulWidget, Widget},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Queue {
    theme: Theme,
    items: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueueState {
    pub active: bool,
    pub position: usize,
    pub view: usize,
    pub tagstring: String,
}

impl Default for QueueState {
    fn default() -> Self {
        Self {
            active: true,
            position: 0,
            view: 0,
            tagstring: String::from("title"),
        }
    }
}

impl Queue {
    pub fn new<T: AsRef<Library>, U: Into<String> + AsRef<str>>(
        library: T,
        theme: Theme,
        tagstring: U,
    ) -> Self {
        Self {
            items: {
                // doesn't use get_taglist_sort as that method also dedupes.
                let mut taglist = library.as_ref().get_taglist(tagstring);
                taglist.sort();
                taglist
            },
            theme,
        }
    }
}

impl StatefulWidget for Queue {
    type State = QueueState;
    fn render(self, area: Rect, buff: &mut Buffer, state: &mut Self::State) {
        let block = tui::widgets::Block::default()
            .borders(tui::widgets::Borders::ALL)
            .title("Queue");
        let outer = area;
        let area = block.inner(outer);
        block.render(outer, buff);

        for (y, i) in (0..area.height).zip(self.items.iter().skip(state.view)) {
            buff.set_stringn(
                area.x,
                y + area.y,
                i,
                area.width.into(),
                if state.active {
                    if state.position == y as usize + state.view {
                        self.theme.active_sel
                    } else {
                        self.theme.active
                    }
                } else {
                    if state.position == y as usize + state.view {
                        self.theme.base_sel
                    } else {
                        self.theme.base
                    }
                },
            );
        }
    }
}

impl ClickableStatefulWidget for Queue {
    fn process_stateful_event<T: AsRef<crate::Library>>(
        event: event::MouseEvent,
        area: tui::layout::Rect,
        library: T,
        state: &mut Self::State,
    ) -> bool {
        match event.kind {
            event::MouseEventKind::Moved
            | event::MouseEventKind::Drag(..)
            | event::MouseEventKind::Up(..) => return false,
            _ => (),
        }

        let library: &Library = library.as_ref();
        let inner = tui::widgets::Block::default()
            .borders(tui::widgets::Borders::ALL)
            .inner(area);
        let point = Rect::new(event.column, event.row, 1, 1);

        if area.intersects(point) {
            state.active = true;
            let index = state.view
                + (event.row as usize)
                    .saturating_sub(1)
                    .saturating_sub(area.y as usize);

            match event.kind {
                event::MouseEventKind::Down(event::MouseButton::Left) => {
                    match library.get_queue_sort(&state.tagstring).get(index) {
                        Some(track) if inner.intersects(point) => {
                            state.position = index;
                            library.play_track(track.clone().into());
                            false
                        }
                        _ => true,
                    }
                }
                event::MouseEventKind::ScrollDown => {
                    scroll_down(
                        &mut state.position,
                        &mut state.view,
                        area.height as usize,
                        library.get_queue().len(),
                    );
                    true
                }
                event::MouseEventKind::ScrollUp => {
                    scroll_up(&mut state.position, &mut state.view, area.height as usize);
                    true
                }
                _ => true,
            }
        } else {
            if state.active {
                state.active = false;
                true
            } else {
                false
            }
        }
    }
}
