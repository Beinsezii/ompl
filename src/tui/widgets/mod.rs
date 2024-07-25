#![warn(missing_docs)]

use std::cmp::Ordering;

pub use super::stylesheet::StyleSheet;
pub use super::Action;
mod statusbar;
pub use statusbar::StatusBar;
mod menubar;
pub use menubar::{MTree, MenuBar};
mod filtertreeview;
pub use filtertreeview::FilterTreeView;
mod queuetable;
pub use queuetable::QueueTable;
mod seeker;
pub use seeker::Seeker;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    terminal::Frame,
    text::Span,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crossterm::event::{MouseButton, MouseEventKind};

/// Creates constraints for a distance that meet 3 criteria:
/// 1) Fills the whole width with no gaps
/// 2) Creates sections of equal or close as possible size
/// 3) Creates sections that align.
///    The "center" of a 2 count will align with the "center" of a 4 count
///
/// The current method of adding remainders seems to achieve this in my limited testing
/// It's the little things that make a good program
pub fn equal_constraints(width: u16, n: u16) -> Vec<Constraint> {
    let mut constraints = vec![];
    let mut remainder = 0.0;
    for _i in 1..n {
        let l = width as f32 / n as f32;
        remainder += l % 1.0;
        constraints.push(Constraint::Length(if remainder >= 1.0 {
            remainder -= 1.0;
            l.ceil()
        } else {
            l.floor()
        } as u16))
    }
    constraints.push(Constraint::Min(1));
    constraints
}

/// Self-contained widget does it's own state and render management
pub trait ContainedWidget {
    fn draw(&mut self, frame: &mut Frame, stylesheet: StyleSheet);
}

pub trait Clickable {
    fn process_event(&mut self, event: crossterm::event::MouseEvent) -> Action;
}

// ### Scrollable ### {{{

pub trait Scrollable {
    /// cursor position, view offset, height of view, max length
    fn get_fields(&mut self) -> Option<(&mut usize, &mut usize, usize, usize)>;

    /// scroll down half view length
    fn scroll_down(&mut self) {
        if let Some((position, view, height, length)) = self.get_fields() {
            scroll_by_n(height as i32 / 2, position, view, height, length)
        }
    }

    /// scroll up half view length
    fn scroll_up(&mut self) {
        if let Some((position, view, height, length)) = self.get_fields() {
            scroll_by_n(-(height as i32 / 2), position, view, height, length)
        }
    }

    /// move position by N and lock view to center
    fn scroll_by_n_lock(&mut self, n: i32) {
        if let Some((position, view, height, length)) = self.get_fields() {
            scroll_by_n_lock(n, position, view, height, length)
        }
    }

    /// move position and view down by N
    fn scroll_by_n(&mut self, n: i32) {
        if let Some((position, view, height, length)) = self.get_fields() {
            scroll_by_n(n, position, view, height, length)
        }
    }
}

pub trait Searchable: Scrollable {
    fn get_items<'a>(&self) -> Vec<String>;

    fn find(&mut self, query: &str) {
        let items = self.get_items();

        for x in 0..=1 {
            for (n, i) in items.iter().enumerate() {
                if if x == 0 {
                    i.trim().to_ascii_lowercase().starts_with(query)
                } else {
                    i.trim().to_ascii_lowercase().contains(query)
                } {
                    self.scroll_by_n(i32::MIN);
                    self.scroll_by_n_lock(n as i32);
                    return;
                }
            }
        }
    }
}

pub fn scroll_by_n(n: i32, position: &mut usize, view: &mut usize, height: usize, length: usize) {
    *position = (n + *position as i32).max(0).min(length as i32 - 1) as usize;
    *view = (n + *view as i32).max(0).min(length.saturating_sub(height) as i32) as usize;
}

pub fn scroll_by_n_lock(n: i32, position: &mut usize, view: &mut usize, height: usize, length: usize) {
    *position = (n + *position as i32).max(0).min(length.saturating_sub(1) as i32) as usize;
    *view = position.saturating_sub(height / 2).min(length.saturating_sub(height));
}

// ### Scrollable ### }}}

// ### PaneArray ### {{

#[derive(Clone, Debug)]
pub struct PaneArray {
    joined: bool,
    shown_items: Vec<String>,
    pub area: Rect,
    pub active: bool,
    pub index: usize,
    pub positions: Vec<usize>,
    pub views: Vec<usize>,
    pub drag_vals: Vec<usize>,
}

const PA_LONG: &'static str = "<<++::--++>>";
const PA_SHORT: &'static str = "<+:-+>";

/// Updates positions before sending
pub enum PaneArrayEvt {
    Click,
    ClickTit,
    RClick,
    RClickTit,
    RDrag,
    ScrollUp,
    ScrollDown,
    Action(Action),
}

impl PaneArray {
    pub fn new(joined: bool, count: usize) -> Self {
        Self {
            joined,
            shown_items: Vec::new(),
            area: Rect::default(),
            active: false,
            index: 0,
            positions: vec![0; if joined { 1 } else { count }],
            views: vec![0; if joined { 1 } else { count }],
            drag_vals: Vec::new(),
        }
    }

    // # prep_event # {{{
    pub fn prep_event(&mut self, event: crossterm::event::MouseEvent, items: &[(usize, usize)]) -> PaneArrayEvt {
        let none = PaneArrayEvt::Action(Action::None);
        match event.kind {
            MouseEventKind::Moved | MouseEventKind::Up(..) => return none,
            _ => (),
        }

        let point = Rect::new(event.column, event.row, 1, 1);

        // localize drag state to this current iter,
        // so if it exits early for any reason the drag is cancelled
        let mut drag_vals_tmp = Vec::new();
        std::mem::swap(&mut drag_vals_tmp, &mut self.drag_vals);

        if self.area.intersects(point) {
            self.active = true;
            for (num, zone) in Layout::default()
                .direction(Direction::Horizontal)
                .constraints(equal_constraints(self.area.width, items.len() as u16))
                .split(self.area)
                .into_iter()
                .enumerate()
            {
                if zone.intersects(point) {
                    self.index = num;
                    let num_join = if self.joined { 0 } else { num };

                    match event.kind {
                        MouseEventKind::ScrollUp => {
                            return PaneArrayEvt::ScrollUp;
                        }
                        MouseEventKind::ScrollDown => {
                            return PaneArrayEvt::ScrollDown;
                        }
                        #[allow(non_snake_case)]
                        MouseEventKind::Down(button) => {
                            // event coords parented to pane
                            let (zX, zY) = (event.column - zone.x, event.row - zone.y);
                            let footer = if zone.width < PA_LONG.len() as u16 {
                                PA_SHORT.len()
                            } else {
                                PA_LONG.len()
                            } as u16;
                            // click title
                            if zX >= 1 && zX <= items[num].0 as u16 && zY == 0 {
                                match button {
                                    MouseButton::Left => return PaneArrayEvt::ClickTit,

                                    MouseButton::Right => return PaneArrayEvt::RClickTit,

                                    MouseButton::Middle => (),
                                }
                            // click footer
                            } else if zY == zone.height.saturating_sub(1)
                                && zX > zone.width.saturating_sub(footer + 2)
                                && zX < zone.width.saturating_sub(1)
                            {
                                let i = footer.saturating_sub(zone.width.saturating_sub(zX + 1));
                                let step = footer / PA_SHORT.len() as u16;
                                if i < step {
                                    return PaneArrayEvt::Action(Action::MoveLeft);
                                } else if i < step * 2 {
                                    return PaneArrayEvt::Action(Action::InsertBefore);
                                } else if i < step * 3 {
                                    return PaneArrayEvt::Action(Action::Edit);
                                } else if i < step * 4 {
                                    return PaneArrayEvt::Action(Action::Delete);
                                } else if i < step * 5 {
                                    return PaneArrayEvt::Action(Action::InsertAfter);
                                } else {
                                    return PaneArrayEvt::Action(Action::MoveRight);
                                }

                            // click in list
                            } else if zX > 0
                                && zX < zone.width - 1
                                && zY > 0
                                && zY < zone.height - 1
                                && usize::from(zY) < items[num].1.saturating_sub(self.views[num_join]) + 1
                            {
                                match button {
                                    MouseButton::Left => {
                                        self.positions[num_join] = zY as usize + self.views[num_join] - 1;
                                        return PaneArrayEvt::Click;
                                    }
                                    MouseButton::Right => {
                                        let pos = zY as usize + self.views[num_join] - 1;
                                        self.positions[num_join] = pos;
                                        self.drag_vals = vec![pos];
                                        return PaneArrayEvt::RClick;
                                    }
                                    _ => (),
                                }
                            }
                        } // match ::Down()
                        // barebones copy for testing/ease
                        #[allow(non_snake_case)]
                        MouseEventKind::Drag(MouseButton::Right) => {
                            let (zX, zY) = (event.column - zone.x, event.row - zone.y);
                            // click in list
                            if zX > 0
                                && zX < zone.width - 1
                                && zY > 0
                                && zY < zone.height - 1
                                && usize::from(zY) < items[num].1.saturating_sub(self.views[num_join]) + 1
                            {
                                std::mem::swap(&mut drag_vals_tmp, &mut self.drag_vals);
                                let pos = zY as usize + self.views[num_join] - 1;
                                let do_draw = self.positions[num_join] != pos;
                                self.positions[num_join] = pos;
                                if !self.drag_vals.contains(&pos) && !self.drag_vals.is_empty() {
                                    self.drag_vals.push(pos);
                                    return PaneArrayEvt::RDrag;
                                } else if do_draw {
                                    return PaneArrayEvt::Action(Action::Draw);
                                }
                            }
                        }
                        _ => (),
                    } // match event

                    break;
                } // zone intersects
            }
        } else {
            // area intersects
            self.active = false
        }
        none
    }
    // # prep_event # }}}

    // # draw_from # {{{
    pub fn draw_from(&mut self, frame: &mut Frame, stylesheet: StyleSheet, items: Vec<(String, Vec<String>)>, highlights: Vec<Vec<String>>) {
        // clamp index
        self.index = self.index.min(items.len().saturating_sub(1));

        // Find updated items and views
        if !self.joined && self.shown_items.iter().cmp(items.iter().map(|i| &i.0)) != Ordering::Equal {
            let new_items: Vec<String> = items.iter().map(|i| i.0.clone()).collect();
            let mut positions = vec![0; new_items.len()];
            let mut views = vec![0; new_items.len()];

            // In case of duplicates
            let mut matches = Vec::new();

            for (item, (position, view)) in self.shown_items.iter().zip(self.positions.iter().zip(self.views.iter())) {
                if let Some(i) = new_items
                    .iter()
                    .enumerate()
                    .find_map(|(n, ni)| (ni == item && !matches.contains(&n)).then_some(n))
                {
                    matches.push(i);
                    (positions[i], views[i]) = (*position, *view);
                }
            }

            self.shown_items = new_items;
            self.positions = positions;
            self.views = views;
        }

        if items.len() == 0 {
            return;
        };

        for (num, (area, item)) in Layout::default()
            .direction(Direction::Horizontal)
            .constraints(equal_constraints(self.area.width, items.len() as u16))
            .split(self.area)
            .into_iter()
            .zip(items.into_iter())
            .enumerate()
        {
            let num_join = if self.joined { 0 } else { num };

            // clamp scroll
            scroll_by_n(
                0,
                &mut self.positions[num_join],
                &mut self.views[num_join],
                self.area.height.saturating_sub(2).into(),
                item.1.len(),
            );

            frame.render_widget(
                List::new(
                    item.1
                        .into_iter()
                        .enumerate()
                        .map(|(n, s)| {
                            ListItem::new(s.clone()).style(if self.active && num == self.index {
                                match highlights.get(num).unwrap_or(&vec![]).contains(&s) {
                                    true => match n == self.positions[num_join] {
                                        true => stylesheet.active_hi_sel,
                                        false => stylesheet.active_hi,
                                    },
                                    false => match n == self.positions[num_join] {
                                        true => stylesheet.active_sel,
                                        false => stylesheet.active,
                                    },
                                }
                            } else {
                                match highlights.get(num).unwrap_or(&vec![]).contains(&s) {
                                    true => match n == self.positions[num_join] {
                                        true => stylesheet.base_hi_sel,
                                        false => stylesheet.base_hi,
                                    },
                                    false => match n == self.positions[num_join] {
                                        true => stylesheet.base_sel,
                                        false => stylesheet.base,
                                    },
                                }
                            })
                        })
                        .skip(self.views[num_join])
                        .collect::<Vec<ListItem>>(),
                )
                .block(
                    Block::default()
                        .title(Span::styled(
                            item.0,
                            if self.active && num == self.index {
                                match highlights.get(num).unwrap_or(&vec![]).is_empty() {
                                    true => stylesheet.active,
                                    false => stylesheet.active_hi,
                                }
                            } else {
                                match highlights.get(num).unwrap_or(&vec![]).is_empty() {
                                    true => stylesheet.base,
                                    false => stylesheet.base_hi,
                                }
                            },
                        ))
                        .borders(Borders::ALL)
                        .style(if self.active && num == self.index {
                            stylesheet.active
                        } else {
                            stylesheet.base
                        }),
                ),
                *area,
            );
            frame.render_widget(
                Paragraph::new(if area.width < PA_LONG.len() as u16 { PA_SHORT } else { PA_LONG }).alignment(Alignment::Right),
                Rect {
                    x: area.x,
                    y: area.y + area.height.saturating_sub(1),
                    width: area.width.saturating_sub(1),
                    // needs to handle 0 else crash bang
                    height: area.height.min(1),
                },
            );
        }
    }
    // # draw_from # }}}
}

// ### PaneArray ### }}}
