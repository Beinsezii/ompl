use super::{Clickable, ContainedWidget, PaneArray, PaneArrayEvt, Scrollable, Searchable, StyleSheet};
use crate::library::{get_taglist_sort, Library};

use std::sync::{Arc, Weak};

use crossterm::event::{MouseEvent, MouseEventKind};
use tui::{backend::Backend, layout::Rect, terminal::Frame};

// ### struct FilterTreeView {{{

#[derive(Clone)]
pub struct FilterTreeView {
    lib_weak: Weak<Library>,
    pane_array: PaneArray,
}

impl FilterTreeView {
    pub fn new(library: Arc<Library>) -> Self {
        let count = library.filter_count();
        Self {
            lib_weak: Arc::downgrade(&library),
            pane_array: PaneArray::new(false, count),
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

    pub fn positions(&self) -> &[usize] {
        &self.pane_array.positions
    }
    pub fn positions_mut(&mut self) -> &mut Vec<usize> {
        &mut self.pane_array.positions
    }

    pub fn views(&self) -> &[usize] {
        &self.pane_array.views
    }
    pub fn views_mut(&mut self) -> &mut Vec<usize> {
        &mut self.pane_array.views
    }

    pub fn toggle_current(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            let (tags, data) = library.get_filter_tree_display();
            if let Some(mut fi) = library.get_filter_items(self.index()) {
                let item = get_taglist_sort(&tags[self.index()].tag, &data[self.index()])
                    .get(self.positions()[self.index()] as usize)
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
                .get(self.positions()[self.index()])
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

    /// Used when a filter is removed to save positions
    pub fn remove(&mut self) {
        if self.index() < self.positions().len() {
            let i = self.index();
            self.positions_mut().remove(i);
            self.views_mut().remove(i);
        }
    }

    /// Used when a filter is to create new positions
    pub fn insert(&mut self, before: bool) {
        let pos = self.index() + if before { 0 } else { 1 };
        let (poslen, viewlen) = (self.positions().len(), self.views().len());
        self.positions_mut().insert(pos.min(poslen), 0);
        self.views_mut().insert(pos.min(viewlen), 0);
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
    fn draw<T: Backend>(&mut self, frame: &mut Frame<T>, stylesheet: StyleSheet) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };

        let (filters, tracks) = library.get_filter_tree_display();

        let mut items = Vec::<(String, Vec<String>)>::new();
        let mut highlights = Vec::<Vec<String>>::new();

        for (ft, tl) in filters.into_iter().zip(tracks.into_iter()) {
            highlights.push(ft.items); // lightly confusing
            let tl_tags = get_taglist_sort(&ft.tag, &tl);
            items.push((ft.tag, tl_tags));
        }

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
            MouseEventKind::Moved | MouseEventKind::Drag(..) | MouseEventKind::Up(..) => {
                return none
            }
            _ => (),
        }

        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return none,
        };

        let (filters, tracks) = library.get_filter_tree_display();

        let mut items = Vec::<(usize, usize)>::new();

        for (ft, tl) in filters.iter().zip(tracks.iter()) {
            items.push((ft.tag.len(), get_taglist_sort(&ft.tag, &tl).len()));
        }

        match self.pane_array.prep_event(event, &items) {
            PaneArrayEvt::Click => self.select_current(),
            PaneArrayEvt::RClick => self.toggle_current(),
            PaneArrayEvt::ClickTit => self.invert_selection(),
            PaneArrayEvt::RClickTit => self.deselect_all(),
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
