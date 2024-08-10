#![warn(missing_docs)]

use super::{Clickable, ContainedWidget, PaneArray, PaneArrayEvt, Scrollable, Searchable, StyleSheet};
use crate::library::Library;

use std::sync::{Arc, Weak};

use ratatui::crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::{layout::Rect, Frame};

// ### struct QueueTable {{{

#[derive(Clone)]
pub struct QueueTable {
    lib_weak: Weak<Library>,
    pane_array: PaneArray,
}

impl QueueTable {
    pub fn new(library: Arc<Library>) -> Self {
        let mut pane_array = PaneArray::new(true, 1);
        pane_array.active = true;
        Self {
            lib_weak: Arc::downgrade(&library),
            pane_array,
        }
    }

    #[allow(unused)]
    pub fn area(&self) -> Rect {
        self.pane_array.area
    }
    pub fn area_mut(&mut self) -> &mut Rect {
        &mut self.pane_array.area
    }

    pub fn active(&self) -> bool {
        self.pane_array.active
    }
    pub fn active_mut(&mut self) -> &mut bool {
        &mut self.pane_array.active
    }

    pub fn index(&self) -> usize {
        self.pane_array.index
    }
    pub fn index_mut(&mut self) -> &mut usize {
        &mut self.pane_array.index
    }

    pub fn position(&self) -> usize {
        self.pane_array.positions[0]
    }

    fn get_rows(&self) -> Vec<Vec<String>> {
        let mut rows = Vec::<Vec<String>>::new();
        let Some(library) = self.lib_weak.upgrade() else { return rows };

        let mut tags = library.get_sorters();

        // if nothing then fetch title since title will always exist
        if tags.is_empty() {
            tags.push("title".to_string())
        }

        let items = tags.iter().map(|t| library.get_taglist(t)).collect::<Vec<Vec<String>>>();

        for x in 0..items[0].len() {
            rows.push(items.iter().map(|i| i[x].clone()).collect::<Vec<String>>());
        }

        rows
    }
}

// ### struct QueueTable }}}

// ### impl ContainedWidget {{{
impl ContainedWidget for QueueTable {
    fn draw(&mut self, frame: &mut Frame, area: Rect, stylesheet: StyleSheet) {
        self.pane_array.area = area;
        let Some(library) = self.lib_weak.upgrade() else { return };

        let mut items = Vec::<(String, Vec<String>)>::new();
        let highlights = Vec::<Vec<String>>::new();

        for ts in library.get_sorters() {
            let list = library.get_taglist(&ts);
            items.push((ts, list));
        }

        if items.is_empty() {
            items.push(("[unsorted]".to_string(), library.get_taglist("title")))
        }

        self.pane_array.draw_from(frame, stylesheet, items, highlights);
    }
}
// ### impl ContainedWidget }}}

// ### impl Scrollable, Searchable {{{

impl Scrollable for QueueTable {
    fn get_fields(&mut self) -> Option<(&mut usize, &mut usize, usize, usize)> {
        self.lib_weak.upgrade().map(|library| {
            (
                &mut self.pane_array.positions[0],
                &mut self.pane_array.views[0],
                self.pane_array.area.height.saturating_sub(2).into(),
                library.get_queue().len(),
            )
        })
    }
}

impl Searchable for QueueTable {
    fn get_items<'a>(&self) -> Vec<String> {
        self.get_rows().into_iter().map(|v| v[self.index()].clone()).collect::<Vec<String>>()
    }
}

// ### impl Scrollable, Searchable }}}

// ### impl Clickable {{{
impl Clickable for QueueTable {
    fn process_event(&mut self, event: MouseEvent) -> super::Action {
        let none = super::Action::None;
        let draw = super::Action::Draw;
        match event.kind {
            MouseEventKind::Moved | MouseEventKind::Drag(..) | MouseEventKind::Up(..) => return none,
            _ => (),
        }

        let Some(library) = self.lib_weak.upgrade() else { return none };

        let mut items = Vec::<(usize, usize)>::new();

        for s in library.get_sorters().into_iter() {
            items.push((s.len(), library.get_queue().len()))
        }

        let queue = library.get_queue();

        if items.is_empty() {
            items.push(("[unsorted]".len(), queue.len()))
        }

        let oldpos = self.position();

        match self.pane_array.prep_event(event, &items) {
            PaneArrayEvt::Click => library.play_track(queue.get(self.position()).cloned()),
            PaneArrayEvt::RClick => {
                if self.position() == oldpos {
                    self.scroll_by_n_lock(0)
                };
                return draw;
            }
            PaneArrayEvt::ClickTit => (),
            PaneArrayEvt::RClickTit => (),
            PaneArrayEvt::RDrag => (),
            PaneArrayEvt::ScrollUp => {
                self.scroll_up();
                return draw;
            }
            PaneArrayEvt::ScrollDown => {
                self.scroll_down();
                return draw;
            }
            PaneArrayEvt::Action(a) => return a,
        }

        none
    }
}
// ### impl Clickable }}}
