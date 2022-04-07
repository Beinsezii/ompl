use super::{Clickable, ContainedWidget, Scrollable, Searchable, Theme};
use crate::library::Library;

use std::sync::{Arc, Weak};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use tui::{
    backend::Backend,
    layout::{Constraint, Rect},
    terminal::Frame,
    widgets::{Block, Borders, Cell, Row, Table},
};

// ### struct QueueTable {{{

#[derive(Clone)]
pub struct QueueTable {
    lib_weak: Weak<Library>,
    pub area: Rect,
    pub active: bool,
    pub index: usize,
    pub position: usize,
    pub view: usize,
}

impl QueueTable {
    pub fn new(library: Arc<Library>) -> Self {
        Self {
            lib_weak: Arc::downgrade(&library),
            area: Rect::default(),
            active: true,
            index: 0,
            position: 0,
            view: 0,
        }
    }

    fn get_rows(&self) -> Vec<Vec<String>> {
        let mut rows = Vec::<Vec<String>>::new();
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return rows,
        };

        let mut tags = library.get_sort_tagstrings();

        // if nothing then fetch title since title will always exist
        if tags.is_empty() {
            tags.push("title".to_string())
        }

        let items = tags
            .iter()
            .map(|t| library.get_taglist(t))
            .collect::<Vec<Vec<String>>>();

        for x in 0..items[0].len() {
            rows.push(items.iter().map(|i| i[x].clone()).collect::<Vec<String>>());
        }

        rows
    }
}

// ### struct QueueTable }}}

// ### impl ContainedWidget {{{
impl ContainedWidget for QueueTable {
    fn draw<T: Backend>(&mut self, frame: &mut Frame<T>, theme: Theme) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };

        // clamp scroll
        self.scroll_by_n(0);

        let mut tags = library.get_sort_tagstrings();

        let count = tags.len().max(1);

        self.index = self.index.min(count.saturating_sub(1));

        if tags.is_empty() {
            tags = vec!["[unsorted]".to_string()]
        }

        let rows = self.get_rows();

        let width = (self.area.width.saturating_sub(4) / count as u16).saturating_sub(1);

        let constraints = vec![Constraint::Length(width); count];

        frame.render_widget(
            Table::new(
                rows.into_iter()
                    .enumerate()
                    .skip(self.view)
                    .map(|(vnum, r)| {
                        Row::new(
                            r.into_iter()
                                .enumerate()
                                .map(|(hnum, cell)| {
                                    Cell::from(cell).style(if self.active && hnum == self.index {
                                        if vnum == self.position {
                                            theme.active_sel
                                        } else {
                                            theme.active
                                        }
                                    } else {
                                        if vnum == self.position {
                                            theme.base_sel
                                        } else {
                                            theme.base
                                        }
                                    })
                                })
                                .collect::<Vec<Cell>>(),
                        )
                    })
                    .collect::<Vec<Row>>(),
            )
            .column_spacing(2)
            .header(
                Row::new(
                    tags.iter()
                        .enumerate()
                        .map(|(hnum, cell)| {
                            Cell::from(cell.clone()).style(if self.active && hnum == self.index {
                                theme.active_hi
                            } else {
                                theme.base_hi
                            })
                        })
                        .collect::<Vec<Cell>>(),
                ), // .bottom_margin(1),
            )
            .widths(&constraints)
            .block(Block::default().title("Queue").borders(Borders::ALL)),
            self.area,
        );
    }
}
// ### impl ContainedWidget }}}

// ### impl Scrollable, Searchable {{{

impl Scrollable for QueueTable {
    fn get_fields(&mut self) -> Option<(&mut usize, &mut usize, usize, usize)> {
        self.lib_weak.upgrade().map(|library| {
            (
                &mut self.position,
                &mut self.view,
                self.area.height.saturating_sub(3).into(),
                library.get_queue().len(),
            )
        })
    }
}

impl Searchable for QueueTable {
    fn get_items<'a>(&self) -> Vec<String> {
        self.get_rows()
            .into_iter()
            .map(|v| v[self.index].clone())
            .collect::<Vec<String>>()
    }
}

// ### impl Scrollable, Searchable }}}

// ### impl Clickable {{{
impl Clickable for QueueTable {
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

        let prior = self.active;

        if self.area.intersects(point) {
            self.active = true;

            #[allow(non_snake_case)]
            let (zX, zY) = (event.column - self.area.x, event.row - self.area.y);
            let len = library.get_sort_tagstrings().len();
            // trust me
            self.index = (((zX.min(self.area.width.saturating_sub(4)) as f32
                / self.area.width.saturating_sub(2) as f32)
                * len as f32) as usize
                % len.max(1))
            .min(len.saturating_sub(1));

            match event.kind {
                MouseEventKind::ScrollUp => {
                    self.scroll_up();
                    return true;
                }
                MouseEventKind::ScrollDown => {
                    self.scroll_down();
                    return true;
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if zX >= 1 && zX < self.area.width && zY >= 2 && zY < self.area.height {
                        if let Some(track) = library.get_queue().get(zY as usize + self.view - 2) {
                            self.position = zY as usize + self.view - 2;
                            library.play_track(Some(track.clone()))
                        }
                    }
                    return true;
                }
                _ => (),
            }
        } else {
            self.active = false;
        }

        prior != self.active
    }
}
// ### impl Clickable }}}
