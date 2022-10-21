pub use super::theme::Theme;
mod statusbar;
pub use statusbar::StatusBar;
mod menubar;
pub use menubar::{MTree, MenuBar};
mod filtertreeview;
pub use filtertreeview::FilterTreeView;
mod queuetable;
pub use queuetable::QueueTable;

use tui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    terminal::Frame,
    text::Span,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crossterm::event::{MouseButton, MouseEventKind};

pub fn equal_constraints(width: u16, n: u16) -> Vec<Constraint> {
    let mut constraints = vec![Constraint::Length(width / n); n.saturating_sub(1).into()];
    constraints.push(Constraint::Min(1));
    constraints
}

/// Self-contained widget does it's own state and render management
pub trait ContainedWidget {
    fn draw<T: tui::backend::Backend>(&mut self, frame: &mut Frame<T>, theme: Theme);
}

pub trait Clickable {
    fn process_event(&mut self, event: crossterm::event::MouseEvent) -> bool;
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
    *view = (n + *view as i32)
        .max(0)
        .min(length.saturating_sub(height) as i32) as usize;
}

pub fn scroll_by_n_lock(
    n: i32,
    position: &mut usize,
    view: &mut usize,
    height: usize,
    length: usize,
) {
    *position = (n + *position as i32)
        .max(0)
        .min(length.saturating_sub(1) as i32) as usize;
    *view = position
        .saturating_sub(height / 2)
        .min(length.saturating_sub(height));
}

// ### Scrollable ### }}}

// ### PaneArray ### {{

#[derive(Clone, Debug)]
pub struct PaneArray {
    joined: bool,
    pub area: Rect,
    pub active: bool,
    pub index: usize,
    pub positions: Vec<usize>,
    pub views: Vec<usize>,
}

const PA_LONG: &'static str = "<<xx>>";
const PA_SHORT: &'static str = "<x>";

/// Updates positions before sending
pub enum PaneArrayEvt {
    Click,
    ClickTit,
    RClick,
    RClickTit,
    ScrollUp,
    ScrollDown,
    Delete,
    MoveLeft,
    MoveRight,
    // // TODO
    // // These need a way for widgets to request text input
    // // Either that or widgets need to be able to signal actions back to TUI main
    // // More restructuring yay
    // Edit,
    // InsertBefore,
    // InsertAfter,
}

impl PaneArray {
    pub fn new(joined: bool, count: usize) -> Self {
        Self {
            joined,
            area: Rect::default(),
            active: false,
            index: 0,
            positions: vec![0; if joined { 1 } else { count }],
            views: vec![0; if joined { 1 } else { count }],
        }
    }

    // # prep_event # {{{
    pub fn prep_event(
        &mut self,
        event: crossterm::event::MouseEvent,
        items: &[(usize, usize)],
    ) -> Option<PaneArrayEvt> {
        match event.kind {
            MouseEventKind::Moved | MouseEventKind::Drag(..) | MouseEventKind::Up(..) => {
                return None
            }
            _ => (),
        }

        let point = Rect::new(event.column, event.row, 1, 1);

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
                            return Some(PaneArrayEvt::ScrollUp);
                        }
                        MouseEventKind::ScrollDown => {
                            return Some(PaneArrayEvt::ScrollDown);
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
                                    MouseButton::Left => return Some(PaneArrayEvt::ClickTit),

                                    MouseButton::Right => return Some(PaneArrayEvt::RClickTit),

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
                                    return Some(PaneArrayEvt::MoveLeft);
                                // } else if i < step * 2 {
                                //     return Some(PaneArrayEvt::InsertBefore);
                                // } else if i < step * 3 {
                                //     return Some(PaneArrayEvt::Edit);
                                } else if i < step * 2 {
                                    return Some(PaneArrayEvt::Delete);
                                // } else if i < step * 5 {
                                //     return Some(PaneArrayEvt::InsertAfter);
                                } else {
                                    return Some(PaneArrayEvt::MoveRight);
                                }

                            // click in list
                            } else if zX > 0
                                && zX < zone.width - 1
                                && zY > 0
                                && zY < zone.height - 1
                                && usize::from(zY)
                                    < items[num].1.saturating_sub(self.views[num_join]) + 1
                            {
                                match button {
                                    MouseButton::Left => {
                                        self.positions[num_join] =
                                            zY as usize + self.views[num_join] - 1;
                                        return Some(PaneArrayEvt::Click);
                                    }
                                    MouseButton::Right => {
                                        self.positions[num_join] =
                                            zY as usize + self.views[num_join] - 1;
                                        return Some(PaneArrayEvt::RClick);
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
        None
    }
    // # prep_event # }}}

    // # draw_from # {{{
    pub fn draw_from<T: tui::backend::Backend>(
        &mut self,
        frame: &mut Frame<T>,
        theme: Theme,
        items: Vec<(String, Vec<String>)>,
        highlights: Vec<Vec<String>>,
    ) {
        if items.len() == 0 {
            return;
        };

        // clamp index
        self.index = self.index.min(items.len().saturating_sub(1));

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
                                        true => theme.active_hi_sel,
                                        false => theme.active_hi,
                                    },
                                    false => match n == self.positions[num_join] {
                                        true => theme.active_sel,
                                        false => theme.active,
                                    },
                                }
                            } else {
                                match highlights.get(num).unwrap_or(&vec![]).contains(&s) {
                                    true => match n == self.positions[num_join] {
                                        true => theme.base_hi_sel,
                                        false => theme.base_hi,
                                    },
                                    false => match n == self.positions[num_join] {
                                        true => theme.base_sel,
                                        false => theme.base,
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
                                    true => theme.active,
                                    false => theme.active_hi,
                                }
                            } else {
                                match highlights.get(num).unwrap_or(&vec![]).is_empty() {
                                    true => theme.base,
                                    false => theme.base_hi,
                                }
                            },
                        ))
                        .borders(Borders::ALL)
                        .style(if self.active && num == self.index {
                            theme.active
                        } else {
                            theme.base
                        }),
                ),
                area,
            );
            frame.render_widget(
                Paragraph::new(if area.width < PA_LONG.len() as u16 {
                    PA_SHORT
                } else {
                    PA_LONG
                })
                .alignment(Alignment::Right),
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
