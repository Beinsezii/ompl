use super::{Clickable, ContainedWidget, Scrollable, Theme};
use crate::library::Library;

use std::sync::{Arc, Weak};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use tui::{
    backend::Backend,
    layout::{Constraint, Rect},
    terminal::Frame,
    widgets::{Block, Borders, Cell, Row, Table},
};

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
}

impl ContainedWidget for QueueTable {
    fn draw<T: Backend>(&self, frame: &mut Frame<T>, theme: Theme) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };

        let mut unsort = false;
        let mut tags = library.get_sort_tagstrings();
        // if nothing then fetch title since title will always exist
        if tags.is_empty() {
            unsort = true;
            tags.push("title".to_string())
        }
        let count = tags.len();

        let items = tags
            .iter()
            .map(|t| library.get_taglist(t))
            .collect::<Vec<Vec<String>>>();

        // only read for header after this point
        if unsort {
            tags = vec!["[unsorted]".to_string()]
        }

        let mut rows = Vec::<Vec<String>>::new();
        for x in 0..items[0].len() {
            rows.push(items.iter().map(|i| i[x].clone()).collect::<Vec<String>>());
        }

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
                                .map(|cell| {
                                    Cell::from(cell).style(if self.active {
                                        if vnum == self.position {
                                            theme.active_sel
                                        } else {
                                            theme.active
                                        }
                                    } else {
                                        theme.base
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
                        .map(|cell| {
                            Cell::from(cell.clone()).style(if self.active {
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

        if self.area.intersects(point) {
            self.active = true;
            match event.kind {
                MouseEventKind::ScrollUp => self.scroll_up(),
                MouseEventKind::ScrollDown => self.scroll_down(),
                #[allow(non_snake_case)]
                MouseEventKind::Down(MouseButton::Left) => {
                    let (zX, zY) = (event.column - self.area.x, event.row - self.area.y);
                    if zX >= 1 && zX < self.area.width && zY >= 2 && zY < self.area.height {
                        if let Some(track) = library.get_queue().get(zY as usize + self.view - 2) {
                            self.position = zY as usize + self.view - 2;
                            library.play_track(Some(track.clone()))
                        }
                    }
                }
                _ => (),
            }
        } else {
            self.active = false;
        }

        true
    }
}
