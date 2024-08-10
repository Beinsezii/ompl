#![warn(missing_docs)]

use super::{Clickable, ContainedWidget, PaneArray, PaneArrayEvt, Scrollable, Searchable, StyleSheet};
use crate::library::{get_taglist_sort, LibEvt, Library};

use std::sync::{Arc, Weak};

use ratatui::crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::{layout::Rect, Frame};

// ### struct FilterTreeView {{{

pub struct FilterTreeView {
    lib_weak: Weak<Library>,
    pane_array: PaneArray,
    recv: bus::BusReader<LibEvt>,
    pane_cache: (Vec<(String, Vec<String>)>, Vec<Vec<String>>),
}

impl FilterTreeView {
    pub fn new(library: Arc<Library>) -> Self {
        let count = library.filter_count();
        Self {
            lib_weak: Arc::downgrade(&library),
            pane_array: PaneArray::new(false, count),
            recv: library.get_receiver().unwrap(),
            pane_cache: Default::default(),
        }
    }

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

    pub fn toggle_current(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            let (tags, data) = library.get_filter_tree_display();
            if let Some(mut fi) = library.get_filter_items(self.index()) {
                let item = get_taglist_sort(&tags[self.index()].tag, &data[self.index()])
                    .get(self.pane_array.positions[self.index()] as usize)
                    .cloned();

                if let Some(item) = item {
                    match fi.contains(&item) {
                        true => fi = fi.into_iter().filter(|i| *i != item).collect(),
                        false => fi.push(item),
                    }

                    library.set_filter_items(self.index(), fi);
                }
            }
        }
    }

    pub fn select_current(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            let (tags, data) = library.get_filter_tree_display();
            let item = get_taglist_sort(&tags[self.index()].tag, &data[self.index()])
                .get(self.pane_array.positions[self.index()])
                .cloned();
            if let Some(item) = item {
                library.set_filter_items(self.index(), vec![item]);
            }
        }
    }

    pub fn deselect_all(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            if library.get_filter_items(self.index()).map(|f| f.is_empty()) == Some(false) {
                library.set_filter_items(self.index(), Vec::new());
            }
        }
    }

    pub fn invert_selection(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            let (tags, data) = library.get_filter_tree_display();
            if let Some(fi) = library.get_filter_items(self.index()) {
                library.set_filter_items(
                    self.index(),
                    get_taglist_sort(&tags[self.index()].tag, &data[self.index()])
                        .into_iter()
                        .filter(|i| !fi.contains(i))
                        .collect(),
                );
            }
        }
    }
}

// ### struct FilterTreeView }}}

// ### impl Scrollable, Searchable ### {{{

impl Scrollable for FilterTreeView {
    fn get_fields(&mut self) -> Option<(&mut usize, &mut usize, usize, usize)> {
        self.lib_weak.upgrade().map(|library| {
            let (tags, data) = library.get_filter_tree_display();
            let i = self.index();
            let area = self.area();
            (
                // using fns makes it think it has mutable aliasing
                &mut self.pane_array.positions[i],
                &mut self.pane_array.views[i],
                area.height.saturating_sub(2).into(),
                get_taglist_sort(&tags[i].tag, &data[i]).len(),
            )
        })
    }
}

impl Searchable for FilterTreeView {
    fn get_items<'a>(&self) -> Vec<String> {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return Vec::new(),
        };
        let (tags, data) = library.get_filter_tree_display();
        let i = self.index().min(library.filter_count());
        get_taglist_sort(&tags[i].tag, &data[i])
    }
}

// ### impl Scrollable, Searchable ### }}}

// ### impl ContainedWidget ### {{{
impl ContainedWidget for FilterTreeView {
    fn draw(&mut self, frame: &mut Frame, stylesheet: StyleSheet) {
        let Some(library) = self.lib_weak.upgrade() else { return };

        let mut update = false;
        while let Ok(i) = self.recv.try_recv() {
            // Should only need to update pane item cache
            // If the filters are updated
            if i == LibEvt::Update {
                update = true
            }
        }

        // Cache parsed tagstrings for all frames
        if update || self.pane_cache.0.is_empty() {
            let (filters, tracks) = library.get_filter_tree_display();

            let mut new_items = Vec::<(String, Vec<String>)>::new();
            let mut new_highlights = Vec::<Vec<String>>::new();

            for (filter, track_list) in filters.into_iter().zip(tracks.into_iter()) {
                new_highlights.push(filter.items); // lightly confusing
                let tl_tags = get_taglist_sort(&filter.tag, &track_list);
                new_items.push((filter.tag, tl_tags));
            }

            self.pane_cache = (new_items, new_highlights);
        }

        let (items, highlights) = self.pane_cache.clone();
        self.pane_array.draw_from(frame, stylesheet, items, highlights)
    }
}
// ### impl ContainedWidget ### }}}

// ### impl Clickable ### {{{
impl Clickable for FilterTreeView {
    fn process_event(&mut self, event: MouseEvent) -> super::Action {
        let none = super::Action::None;
        let draw = super::Action::Draw;
        match event.kind {
            MouseEventKind::Moved | MouseEventKind::Up(..) => return none,
            _ => (),
        }

        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return none,
        };

        let (filters, tracks) = library.get_filter_tree_display();

        let mut lengths = Vec::<(usize, usize)>::new();
        let mut highlights = Vec::<Vec<String>>::new();
        let mut taglists = Vec::<Vec<String>>::new();

        for (ft, tl) in filters.into_iter().zip(tracks.into_iter()) {
            let taglist = get_taglist_sort(&ft.tag, &tl);
            lengths.push((ft.tag.len(), taglist.len()));
            highlights.push(ft.items);
            taglists.push(taglist);
        }

        match self.pane_array.prep_event(event, &lengths) {
            PaneArrayEvt::Click => self.select_current(),
            PaneArrayEvt::RClick => self.toggle_current(),
            PaneArrayEvt::ClickTit => self.invert_selection(),
            PaneArrayEvt::RClickTit => self.deselect_all(),
            PaneArrayEvt::RDrag => {
                // if first drag value state != latest drag value state toggle.
                // RDrag is only sent when a new val is pushed to drag_vals so
                // hacky as it is this works fine while pane_array remains
                // oblivious to the actual contents. Bit heavy though...
                if highlights[self.pane_array.index].contains(&taglists[self.pane_array.index][self.pane_array.drag_vals[0]])
                    != highlights[self.pane_array.index].contains(&taglists[self.pane_array.index][*self.pane_array.drag_vals.last().unwrap()])
                {
                    self.toggle_current()
                }
            }
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
// ### impl Clickable ### }}}
