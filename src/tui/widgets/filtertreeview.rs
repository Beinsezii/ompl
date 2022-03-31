use super::{Clickable, ContainedWidget, Scrollable, Theme};
use crate::library::{get_taglist_sort, Filter, FilteredTracks, Library, Track};

use std::sync::{Arc, Weak};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    terminal::Frame,
    widgets::{Block, Borders, List, ListItem},
};

/// converts filteredtracks into a formate usable by the tree view
/// the data index is offset by -1 according to tagstrings
/// so the first tagstring will display *all* data rather than first filter level
/// so on blah blah.
pub fn tree2view(
    tree: Vec<FilteredTracks>,
    tracks: Vec<Arc<Track>>,
) -> (Vec<String>, Vec<Vec<Arc<Track>>>) {
    let mut data = vec![tracks];
    let mut tags = Vec::new();

    for ft in tree.into_iter() {
        tags.push(ft.filter.tag);
        data.push(ft.tracks);
    }

    data.pop();

    (tags, data)
}

// ### struct FilterTreeView {{{

#[derive(Clone)]
pub struct FilterTreeView {
    lib_weak: Weak<Library>,
    pub area: Rect,
    pub active: bool,
    pub index: usize,
    pub positions: Vec<usize>,
    pub views: Vec<usize>,
}

impl FilterTreeView {
    pub fn new(library: Arc<Library>) -> Self {
        let count = library.filter_count();
        Self {
            lib_weak: Arc::downgrade(&library),
            area: Rect::default(),
            active: false,
            index: 0,
            positions: vec![0; count],
            views: vec![0; count],
        }
    }

    pub fn toggle_current(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            let (tags, data) = tree2view(library.get_filter_tree(), library.get_tracks());
            if let Some(mut fi) = library.get_filter_items(self.index) {
                let item = get_taglist_sort(&tags[self.index], &data[self.index])
                    .remove(self.positions[self.index] as usize);

                match fi.contains(&item) {
                    true => fi = fi.into_iter().filter(|i| *i != item).collect(),
                    false => fi.push(item),
                }

                library.set_filter_items(self.index, fi);
            }
        }
    }

    pub fn select_current(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            let (tags, data) = tree2view(library.get_filter_tree(), library.get_tracks());
            library.set_filter_items(
                self.index,
                vec![get_taglist_sort(&tags[self.index], &data[self.index])
                    .remove(self.positions[self.index])],
            );
        }
    }

    pub fn deselect_all(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            if library.get_filter_items(self.index).map(|f| f.is_empty()) == Some(false) {
                library.set_filter_items(self.index, Vec::new());
            }
        }
    }

    pub fn invert_selection(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            let (tags, data) = tree2view(library.get_filter_tree(), library.get_tracks());
            if let Some(fi) = library.get_filter_items(self.index) {
                library.set_filter_items(
                    self.index,
                    get_taglist_sort(&tags[self.index], &data[self.index])
                        .into_iter()
                        .filter(|i| !fi.contains(i))
                        .collect(),
                );
            }
        }
    }
}

// ### struct FilterTreeView }}}

// ### impl Scrollable ### {{{
impl Scrollable for FilterTreeView {
    fn get_fields(&mut self) -> Option<(&mut usize, &mut usize, usize, usize)> {
        self.lib_weak.upgrade().map(|library| {
            let (tags, data) = tree2view(library.get_filter_tree(), library.get_tracks());
            (
                &mut self.positions[self.index],
                &mut self.views[self.index],
                self.area.height.saturating_sub(2).into(),
                get_taglist_sort(&tags[self.index], &data[self.index]).len(),
            )
        })
    }
}
// ### impl Scrollable ### }}}

// ### impl ContainedWidget ### {{{
impl ContainedWidget for FilterTreeView {
    fn draw<T: Backend>(&self, frame: &mut Frame<T>, theme: Theme) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };
        let ft = library.get_filter_tree();
        let count = ft.len();
        if count == 0 {
            return;
        };
        let filters = ft.iter().map(|f| f.filter.clone()).collect::<Vec<Filter>>();
        let data = tree2view(ft, library.get_tracks()).1;

        // make sure last one always fills
        let mut constraints =
            vec![Constraint::Length(self.area.width / count as u16); count.saturating_sub(1)];
        constraints.push(Constraint::Min(1));

        for (num, ((area, filter), tracks)) in Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(self.area)
            .into_iter()
            .zip(filters.into_iter())
            .zip(data.into_iter())
            .enumerate()
        {
            frame.render_widget(
                List::new(
                    get_taglist_sort(&filter.tag, &tracks)
                        .into_iter()
                        .enumerate()
                        .map(|(n, s)| {
                            ListItem::new(s.clone()).style(if self.active && num == self.index {
                                match filter.items.contains(&s) {
                                    true => match n == self.positions[num] {
                                        true => theme.active_hi_sel,
                                        false => theme.active_hi,
                                    },
                                    false => match n == self.positions[num] {
                                        true => theme.active_sel,
                                        false => theme.active,
                                    },
                                }
                            } else {
                                match filter.items.contains(&s) {
                                    true => match n == self.positions[num] {
                                        true => theme.base_hi_sel,
                                        false => theme.base_hi,
                                    },
                                    false => match n == self.positions[num] {
                                        true => theme.base_sel,
                                        false => theme.base,
                                    },
                                }
                            })
                        })
                        .skip(self.views[num])
                        .collect::<Vec<ListItem>>(),
                )
                .block(Block::default().title(filter.tag).borders(Borders::ALL)),
                area,
            )
        }
    }
}
// ### impl ContainedWidget ### }}}

// ### impl Clickable ### {{{
impl Clickable for FilterTreeView {
    fn process_event(&mut self, event: MouseEvent) -> bool {
        match event.kind {
            MouseEventKind::Moved | MouseEventKind::Drag(..) | MouseEventKind::Up(..) => {
                return false
            }
            _ => (),
        }
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return false,
        };

        let point = Rect::new(event.column, event.row, 1, 1);

        if self.area.intersects(point) {
            self.active = true;
            let (tags, data) = tree2view(library.get_filter_tree(), library.get_tracks());
            for (num, zone) in Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![Constraint::Ratio(1, data.len() as u32); data.len()])
                .split(self.area)
                .into_iter()
                .enumerate()
            {
                if zone.intersects(point) {
                    self.index = num;

                    match event.kind {
                        MouseEventKind::ScrollUp => {
                            self.scroll_up();
                            return true;
                        }
                        MouseEventKind::ScrollDown => {
                            self.scroll_down();
                            return true;
                        }
                        #[allow(non_snake_case)]
                        MouseEventKind::Down(button) => {
                            let (zX, zY) = (event.column - zone.x, event.row - zone.y);
                            // click title
                            if zX >= 1 && zX <= tags[num].len() as u16 && zY == 0 {
                                match button {
                                    MouseButton::Left => {
                                        self.invert_selection();
                                        return false;
                                    }
                                    MouseButton::Right => self.deselect_all(),

                                    MouseButton::Middle => (),
                                }
                            // click in list
                            } else if zX > 0
                                && zX < zone.width - 1
                                && zY > 0
                                && zY < zone.height - 1
                                && usize::from(zY)
                                    < get_taglist_sort(&tags[num], &data[num])
                                        .len()
                                        .saturating_sub(self.views[num])
                                        + 1
                            {
                                match button {
                                    MouseButton::Left => {
                                        self.positions[num] = zY as usize + self.views[num] - 1;
                                        self.select_current();
                                        return false;
                                    }
                                    MouseButton::Right => {
                                        self.positions[num] = zY as usize + self.views[num] - 1;
                                        self.toggle_current();
                                        return false;
                                    }
                                    _ => (),
                                }
                            }
                        } // match ::Down()
                        _ => (),
                    } // match event

                    break;
                } // zone intersects
            }
        } else {
            // area intersects
            self.active = false
        }
        true
    }
}
// ### impl Clickable ### }}}
