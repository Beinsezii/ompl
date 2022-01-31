use std::cmp::min;
use std::io;
use std::io::Write;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

use crate::library::{Filter, LibEvt, Library};
use crate::{l2, log, LOG_LEVEL, LOG_ORD};

use crossbeam::channel;

use crossterm::{
    cursor, event,
    event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    queue, terminal,
};

use tui::backend::{Backend, CrosstermBackend};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Style};
use tui::text;
use tui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph};
use tui::Terminal;

mod theme;
use theme::Theme;
mod widgets;
use widgets::{ClickableStatefulWidget, ClickableWidget};

// ### FNs ### {{{

/// easy matching key events
macro_rules! km {
    ($ch:expr) => {
        Event::Key(KeyEvent {
            code: KeyCode::Char($ch),
            modifiers: KeyModifiers::NONE,
        })
    };
}

// Can't assign modifiers with $mod for some reason
macro_rules! km_c {
    ($ch:expr) => {
        Event::Key(KeyEvent {
            code: KeyCode::Char($ch),
            modifiers: KeyModifiers::CONTROL,
        })
    };
}
macro_rules! km_s {
    ($ch:expr) => {
        Event::Key(KeyEvent {
            code: KeyCode::Char($ch),
            modifiers: KeyModifiers::SHIFT,
        })
    };
}

/// get crossterm event with optional poll duration.
fn get_event(duration: Option<Duration>) -> Option<Event> {
    match duration {
        Some(delay) => {
            if event::poll(delay).unwrap() {
                Some(event::read().unwrap())
            } else {
                None
            }
        }
        None => Some(event::read().unwrap()),
    }
}

// ### FNs ### }}}

pub const HELP: &str = &"\
TUI Controls:
* ? - Show this help
* Ctrl+c - Exit
* a - Play/Pause
* x - Stop
* n - Next
* p - Previous
* v/V - Volume Increase/Decrease
* h/j/k/l - left/down/up/right
* f - [De]select active item
* F - [De]select all
* Tab - [De]select queue
* i/I - Insert filter after/before
* D - Delete Filter
* / - Search
";

// ### UI ### {{{

// ## Events ## {{{

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZoneEventType {
    Panes {
        pane: usize,
        row: usize,
        column: usize,
    },

    Search,
    Insert(bool),
    Delete,
    Help,

    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ZoneEvent {
    kind: MouseEventKind,
    mods: KeyModifiers,
    event: ZoneEventType,
}

// ## Events ## }}}

// ## MultiBar ## {{{

#[derive(Clone, Debug, PartialEq)]
enum MBDrawMode {
    Default,
    Input {
        title: String,
        contents: String,
        style: Style,
    },
}

impl Default for MBDrawMode {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct MultiBar {
    parent: Rect,
    mode: MBDrawMode,
    help: Rect,
    help_div: Rect,
    search: Rect,
    search_div: Rect,
    insert: Rect,
    insert_before: Rect,
    insert_div: Rect,
    delete: Rect,
}

impl MultiBar {
    pub fn from_rect(rect: Rect) -> Self {
        let s = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(4),  // help
                Constraint::Length(3),  // help_div
                Constraint::Length(6),  // search
                Constraint::Length(3),  // serach_div
                Constraint::Length(13), // insert
                Constraint::Length(1),
                Constraint::Length(8),  // insert_before
                Constraint::Length(3),  // insert_div
                Constraint::Length(13), // delete
                Constraint::Length(0),
            ])
            .split(rect);
        Self {
            parent: rect,
            mode: MBDrawMode::default(),
            help: s[1],
            help_div: s[2],
            search: s[3],
            search_div: s[4],
            insert: s[5],
            insert_before: s[7],
            insert_div: s[8],
            delete: s[9],
        }
    }
    pub fn draw<T: Backend>(&self, frame: &mut tui::terminal::Frame<T>) {
        match &self.mode {
            MBDrawMode::Default => {
                frame.render_widget(Paragraph::new("Help"), self.help);
                frame.render_widget(Paragraph::new(" | "), self.help_div);
                frame.render_widget(Paragraph::new("Search"), self.search);
                frame.render_widget(Paragraph::new(" | "), self.search_div);
                frame.render_widget(Paragraph::new("Insert Filter"), self.insert);
                frame.render_widget(Paragraph::new("[Before]"), self.insert_before);
                frame.render_widget(Paragraph::new(" | "), self.insert_div);
                frame.render_widget(Paragraph::new("Delete Filter"), self.delete);
            }
            MBDrawMode::Input {
                title,
                contents,
                style,
            } => {
                let rects = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(vec![
                        Constraint::Length((title.len() + 3) as u16),
                        Constraint::Length(0),
                    ])
                    .split(self.parent);
                frame.render_widget(
                    Paragraph::new(format!(" {}: ", title)).style(*style),
                    rects[0],
                );
                frame.render_widget(Paragraph::new(format!("{}", contents)), rects[1]);
            }
        }
    }
}

// ## MultiBar ## }}}

// ## DebugBar ## {{{

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DebugBar {
    parent: Rect,
    draw_count: Rect,
}

impl DebugBar {
    pub fn from_rect(rect: Rect) -> Self {
        let s = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(100), // draw_count
            ])
            .split(rect);
        Self {
            parent: rect,
            draw_count: s[0],
        }
    }
    pub fn draw<T: Backend>(&self, frame: &mut tui::terminal::Frame<T>, draw_count: u128) {
        frame.render_widget(
            Paragraph::new(format!(" Draws {} ", draw_count)),
            self.draw_count,
        );
    }
}

// ## DebugBar ## }}}

#[derive(Clone, Debug, PartialEq)]
struct FilterPane {
    tag: String,
    items: Vec<String>,
    index: usize,
    selected: Vec<usize>,
    rect: Rect,
    view: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    Queue,
    Panes(usize),
}

struct UI<T: Backend> {
    lib_weak: Weak<Library>,
    status_bar_area: Rect,
    multi_bar: MultiBar,
    debug_bar: DebugBar,
    panes: Vec<FilterPane>,
    panes_index: usize,
    queue_area: Rect,
    queue_state: widgets::QueueState,
    theme: Theme,
    terminal: Option<Terminal<T>>,
    debug: bool,
    draw_count: u128,
}

impl<T: Backend> UI<T> {
    fn from_library(
        library: Arc<Library>,
        terminal: Terminal<T>,
        theme: Theme,
        debug: bool,
    ) -> Self {
        let mut result = Self {
            lib_weak: Arc::downgrade(&library),
            status_bar_area: Rect::default(),
            multi_bar: MultiBar::default(),
            debug_bar: DebugBar::default(),
            panes: Vec::new(),
            panes_index: 0,
            queue_area: Rect::default(),
            queue_state: widgets::QueueState::default(),
            theme,
            terminal: Some(terminal),
            debug,
            draw_count: 0,
        };
        result.update_from_library();
        result
    }

    // ## UI Data FNs ## {{{

    fn update_from_library(&mut self) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };
        let filter_tree = library.get_filter_tree();
        let old_indicies = self.panes.iter().map(|p| p.index).collect::<Vec<usize>>();
        let old_views = self.panes.iter().map(|p| p.view).collect::<Vec<usize>>();
        let height = self.panes.first().map(|p| p.rect.height).unwrap_or(0) as usize;
        self.panes = filter_tree
            .iter()
            .enumerate()
            .map(|(n, f)| {
                let tracks = if n == 0 {
                    library.get_tracks()
                } else {
                    filter_tree[n - 1].tracks.clone()
                };
                let items = crate::library::get_taglist_sort(&f.filter.tag, &tracks);
                FilterPane {
                    tag: f.filter.tag.clone(),
                    index: min(
                        *old_indicies.get(n).unwrap_or(&0),
                        items.len().saturating_sub(1),
                    ),
                    view: min(
                        *old_views.get(n).unwrap_or(&0),
                        items.len().saturating_sub(height.saturating_sub(2) / 2),
                    ),
                    selected: items
                        .iter()
                        .enumerate()
                        .filter_map(|(n, i)| {
                            if f.filter.items.contains(i) {
                                Some(n)
                            } else {
                                None
                            }
                        })
                        .collect(),
                    items,
                    rect: Rect::default(),
                }
            })
            .collect();
        let len = library.get_queue().len();
        self.queue_state.position = min(self.queue_state.position, len.saturating_sub(1));
        self.queue_state.view = min(
            self.queue_state
                .position
                .saturating_sub(self.queue_area.height as usize / 2),
            len.saturating_sub(self.queue_area.height as usize),
        );
        self.draw();
    }

    fn rebuild_filters(&self) -> Vec<Filter> {
        self.panes
            .iter()
            .map(|p| Filter {
                tag: p.tag.clone(),
                items: p
                    .selected
                    .iter()
                    .map(|s| p.items[*s].clone())
                    .collect::<Vec<String>>(),
            })
            .collect::<Vec<Filter>>()
    }

    fn insert_filter(&mut self, before: bool) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };
        if !self.queue_state.active || self.panes.is_empty() {
            let tag = self.multi_bar_input("Filter").trim().to_string();
            if !tag.is_empty() {
                let mut filters = self.rebuild_filters();
                filters.insert(
                    min(
                        self.panes_index + if before { 0 } else { 1 },
                        self.panes.len().saturating_sub(if before { 1 } else { 0 }),
                    ),
                    Filter {
                        tag,
                        items: Vec::new(),
                    },
                );
                library.set_filters(filters);
                self.update_from_library();
                self.panes_index = min(
                    self.panes_index + if before { 0 } else { 1 },
                    self.panes.len().saturating_sub(1),
                );
            }
            self.queue_state.active = false;
        }
    }

    fn delete_filter(&mut self) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };
        if !self.queue_state.active {
            if !self.panes.is_empty() {
                self.panes.remove(self.panes_index);
                self.panes_index = self.panes_index.saturating_sub(1);
                library.set_filters(self.rebuild_filters());
                self.update_from_library();
                if self.panes.is_empty() {
                    self.queue_state.active = true
                }
            }
        }
    }

    fn active_pane_mut(&mut self) -> Option<&mut FilterPane> {
        self.panes.get_mut(self.panes_index)
    }

    // ## UI Data FNs ## }}}

    // ## UI Layout FNs {{{

    fn lock_view(&mut self, pane: Pane) {
        let (position, height, length, view) = match pane {
            Pane::Queue => (
                self.queue_state.position,
                self.queue_area.height,
                self.lib_weak.upgrade().unwrap().get_queue().len(),
                &mut self.queue_state.view,
            ),
            Pane::Panes(i) => (
                self.panes[i].index,
                self.panes[i].rect.height,
                self.panes[i].items.len(),
                &mut self.panes[i].view,
            ),
        };

        widgets::scroll_to(position, view, height.into(), length);
    }

    fn scroll_view_down(&mut self, pane: Pane) {
        let (height, len, view) = match pane {
            Pane::Queue => unreachable!(),
            Pane::Panes(i) => (
                self.panes[i].rect.height,
                self.panes[i].items.len(),
                &mut self.panes[i].view,
            ),
        };
        let offset = height.saturating_sub(2) as usize / 2;
        *view = min(
            *view + offset,
            len.saturating_sub(height.saturating_sub(2) as usize),
        );

        // Rust doesn't allow mutable references to separate fields at once
        let index = match pane {
            Pane::Queue => unreachable!(),
            Pane::Panes(i) => &mut self.panes[i].index,
        };
        *index = min(*index + offset, len - 1);
    }

    fn scroll_view_up(&mut self, pane: Pane) {
        let (height, index) = match pane {
            Pane::Queue => unreachable!(),
            Pane::Panes(i) => (self.panes[i].rect.height, &mut self.panes[i].index),
        };
        let offset = height.saturating_sub(2) as usize / 2;
        *index = index.saturating_sub(offset);

        // Rust doesn't allow mutable references to separate fields at once
        let view = match pane {
            Pane::Queue => unreachable!(),
            Pane::Panes(i) => &mut self.panes[i].view,
        };
        *view = view.saturating_sub(offset);
    }

    fn build_list<'a>(&self, pane: Pane) -> List<'a> {
        let (items, skip, index, active, selected) = match pane {
            Pane::Queue => unreachable!(),
            Pane::Panes(i) => (
                self.panes[i].items.clone(),
                self.panes[i].view,
                self.panes[i].index,
                !self.queue_state.active && i == self.panes_index,
                self.panes[i].selected.clone(),
            ),
        };
        List::new(
            items
                .into_iter()
                .enumerate()
                .skip(skip)
                .map(|(n, i)| {
                    let mut style = if active {
                        if selected.contains(&n) {
                            self.theme.active_hi
                        } else {
                            self.theme.active
                        }
                    } else {
                        if selected.contains(&n) {
                            self.theme.base_hi
                        } else {
                            self.theme.base
                        }
                    };
                    if n == index {
                        style = style.patch(self.theme.mod_select);
                        if active {
                            style = style.patch(self.theme.mod_select_active)
                        }
                    }
                    ListItem::new(text::Span {
                        content: i.into(),
                        style,
                    })
                })
                .collect::<Vec<ListItem>>(),
        )
    }
    // ## UI Layout FNs }}}

    // ## draw ## {{{
    fn draw(&mut self) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };
        self.draw_count += 1;
        let mut terminal = self.terminal.take();
        terminal
            .as_mut()
            .unwrap()
            .draw(|f| {
                let size = f.size();
                let zones = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(vec![
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Length(if self.debug { 1 } else { 0 }),
                        if self.panes.is_empty() {
                            Constraint::Length(0)
                        } else {
                            Constraint::Percentage(50)
                        },
                        Constraint::Percentage(50),
                    ])
                    .split(size);
                for (n, r) in Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(
                        (0..self.panes.len())
                            .map(|_| Constraint::Ratio(1, self.panes.len() as u32))
                            .collect::<Vec<Constraint>>(),
                    )
                    .split(zones[3])
                    .into_iter()
                    .enumerate()
                {
                    self.panes[n].rect = r;
                }
                self.status_bar_area = zones[0];
                f.render_widget(
                    widgets::StatusBar::new(&library, "title"),
                    self.status_bar_area,
                );
                self.queue_area = zones[4];
                f.render_stateful_widget(
                    widgets::Queue::new(&library, self.theme, &self.queue_state.tagstring),
                    self.queue_area,
                    &mut self.queue_state,
                );
                let bar_mode = self.multi_bar.mode.clone();
                self.multi_bar = MultiBar::from_rect(zones[1]);
                self.multi_bar.mode = bar_mode;
                self.multi_bar.draw(f);
                self.debug_bar = DebugBar::from_rect(zones[2]);
                self.debug_bar.draw(f, self.draw_count);
                for (num, fp) in self.panes.iter().enumerate() {
                    f.render_widget(
                        self.build_list(Pane::Panes(num)).block(
                            Block::default()
                                .border_type(BorderType::Plain)
                                .borders(Borders::ALL)
                                .title(text::Span {
                                    content: fp.tag.as_str().into(),
                                    style: if fp.selected.is_empty() {
                                        self.theme.base_hi
                                    } else {
                                        self.theme.base
                                    },
                                }),
                        ),
                        self.panes[num].rect,
                    );
                }
            })
            .unwrap();
        self.terminal = terminal;
    }
    // ## draw ## }}}

    // ## Popops ## {{{

    pub fn message(&mut self, title: &str, message: &str) {
        let mut terminal = self.terminal.take();

        terminal
            .as_mut()
            .unwrap()
            .draw(|f| {
                f.render_widget(
                    Paragraph::new(message).block(
                        Block::default()
                            .border_type(BorderType::Plain)
                            .borders(Borders::ALL)
                            .title(title),
                    ),
                    f.size(),
                )
            })
            .unwrap();

        loop {
            match get_event(None) {
                Some(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Moved,
                    ..
                })) => (),
                Some(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(_),
                    ..
                })) => (),
                Some(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(_),
                    ..
                })) => (),
                _ => break,
            }
        }

        self.terminal = terminal;
        self.draw();
    }

    pub fn multi_bar_input(&mut self, title: &str) -> String {
        let mut result = String::new();
        self.multi_bar.mode = MBDrawMode::Input {
            title: title.to_owned(),
            contents: result.clone(),
            style: self.theme.active,
        };
        self.draw();
        let esc = loop {
            if let Some(event) = get_event(None) {
                match event {
                    km_c!('c') => break false,
                    Event::Key(KeyEvent { code, .. }) => match code {
                        KeyCode::Esc => break true,
                        KeyCode::Tab => break true,
                        KeyCode::Enter => break true,
                        KeyCode::Backspace => drop(result.pop()),
                        KeyCode::Char(c) => result.push(c),
                        _ => continue,
                    },
                    Event::Mouse(MouseEvent {
                        kind: MouseEventKind::Down(_),
                        ..
                    }) => break false,
                    _ => continue,
                }
                self.multi_bar.mode = MBDrawMode::Input {
                    title: title.to_owned(),
                    contents: result.clone(),
                    style: self.theme.active,
                };
                self.draw();
            };
        };
        self.multi_bar.mode = MBDrawMode::Default;
        self.draw();
        if esc {
            result
        } else {
            String::new()
        }
    }

    pub fn search(&mut self) {
        match self
            .multi_bar_input("Search")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" => (),
            input => {
                let (index, items, view) = if self.queue_state.active {
                    (
                        &mut self.queue_state.position,
                        self.lib_weak
                            .upgrade()
                            .unwrap()
                            .get_taglist_sort(&self.queue_state.tagstring),
                        Pane::Queue,
                    )
                } else {
                    let i = self.panes_index;
                    if let Some(pane) = self.active_pane_mut() {
                        (&mut pane.index, pane.items.clone(), Pane::Panes(i))
                    } else {
                        return;
                    }
                };

                for x in 0..=1 {
                    for (n, i) in items.iter().enumerate() {
                        if if x == 0 {
                            i.trim().to_ascii_lowercase().starts_with(&input)
                        } else {
                            i.trim().to_ascii_lowercase().contains(&input)
                        } {
                            *index = n;
                            self.lock_view(view);
                            self.draw();
                            return;
                        }
                    }
                }
            }
        }
    }

    // ## Popops ## }}}

    // ## convert_event ## {{{
    fn convert_event(&mut self, event: MouseEvent) -> ZoneEvent {
        let point = Rect {
            x: event.column,
            y: event.row,
            height: 1,
            width: 1,
        };
        ZoneEvent {
            kind: event.kind,
            mods: event.modifiers,
            event: if self.multi_bar.parent.intersects(point) {
                if self.multi_bar.help.intersects(point) {
                    ZoneEventType::Help
                } else if self.multi_bar.search.intersects(point) {
                    ZoneEventType::Search
                } else if self.multi_bar.insert.intersects(point) {
                    ZoneEventType::Insert(false)
                } else if self.multi_bar.insert_before.intersects(point) {
                    ZoneEventType::Insert(true)
                } else if self.multi_bar.delete.intersects(point) {
                    ZoneEventType::Delete
                } else {
                    ZoneEventType::None
                }
            } else {
                let mut result = ZoneEventType::None;
                if let Some(library) = self.lib_weak.upgrade() {
                    if widgets::StatusBar::process_event(event, self.status_bar_area, &library)
                        || widgets::Queue::process_stateful_event(
                            event,
                            self.queue_area,
                            &library,
                            &mut self.queue_state,
                        )
                    {
                        self.draw()
                    }
                }
                for (num, pane) in self.panes.iter().enumerate() {
                    if pane.rect.intersects(point) {
                        result = ZoneEventType::Panes {
                            pane: num,
                            row: event.row.saturating_sub(pane.rect.y).into(),
                            column: event.column.saturating_sub(pane.rect.x).into(),
                        };
                        break;
                    }
                }
                result
            },
        }
    }
    // ## convert_event ## }}}

    // ## process_event ## {{{
    fn process_event(&mut self, event: Event) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };
        match event {
            // # Key Events # {{{
            Event::Key(KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
            }) => {
                self.queue_state.active = !self.queue_state.active || self.panes.is_empty();
                self.draw();
            }
            km!('h')
            | Event::Key(KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::NONE,
            }) => {
                if !self.queue_state.active {
                    self.panes_index = self.panes_index.saturating_sub(1);
                    self.draw();
                }
            }
            km!('l')
            | Event::Key(KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::NONE,
            }) => {
                if !self.queue_state.active {
                    self.panes_index =
                        min(self.panes_index + 1, self.panes.len().saturating_sub(1));
                    self.draw();
                }
            }
            km!('j')
            | Event::Key(KeyEvent {
                code: KeyCode::Down,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queue_state.active {
                    self.queue_state.position = min(
                        self.queue_state.position + 1,
                        library.get_queue().len().saturating_sub(1),
                    );
                    self.lock_view(Pane::Queue);
                } else if let Some(pane) = self.active_pane_mut() {
                    pane.index = min(pane.index + 1, pane.items.len().saturating_sub(1));
                    self.lock_view(Pane::Panes(self.panes_index));
                }
                self.draw();
            }
            km!('k')
            | Event::Key(KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queue_state.active {
                    self.queue_state.position = self.queue_state.position.saturating_sub(1);
                    self.lock_view(Pane::Queue);
                } else if let Some(pane) = self.active_pane_mut() {
                    pane.index = pane.index.saturating_sub(1);
                    self.lock_view(Pane::Panes(self.panes_index));
                }
                self.draw();
            }

            km!('f')
            | Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queue_state.active {
                    library.play_track(
                        library
                            .get_queue_sort(&self.queue_state.tagstring)
                            .get(self.queue_state.position)
                            .cloned(),
                    )
                } else if let Some(pane) = self.active_pane_mut() {
                    match pane.selected.iter().position(|i| i == &pane.index) {
                        Some(p) => drop(pane.selected.remove(p)),
                        None => pane.selected.push(pane.index),
                    }
                    library.set_filters(self.rebuild_filters());
                    self.update_from_library();
                }
            }

            // shift enter no worky???
            km_s!('F')
            | Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::SHIFT,
            }) => {
                if !self.queue_state.active {
                    if let Some(pane) = self.active_pane_mut() {
                        match pane.selected.is_empty() {
                            true => pane.selected = (0..pane.items.len()).collect(),
                            false => pane.selected = Vec::new(),
                        }
                    }
                }
            }

            km_s!('D') => self.delete_filter(),
            km!('i') => self.insert_filter(false),
            km_s!('I') => self.insert_filter(true),

            km!('?') => self.message("Help", HELP),
            km!('/') => self.search(),

            km!('a') => library.play_pause(),
            km!('x') => library.stop(),
            km!('n') => library.next(),
            km!('p') => library.previous(),
            km!('v') => library.volume_add(0.05),
            km_s!('V') => library.volume_sub(0.05),

            // # Key Events # }}}

            // # Mouse Events # {{{
            Event::Mouse(event) => match self.convert_event(event) {
                ZoneEvent {
                    kind,
                    event,
                    mods: KeyModifiers::NONE,
                } => match kind {
                    MouseEventKind::Down(button) => match button {
                        MouseButton::Left => match event {
                            ZoneEventType::Panes { pane, row, column } => {
                                self.queue_state.active = false;
                                self.panes_index = pane;
                                if row == 0 && column <= self.panes[pane].tag.len() {
                                    self.panes[pane].selected = vec![];
                                } else if row > 0
                                    && row <= self.panes[pane].rect.height as usize - 2
                                    && row <= self.panes[pane].items.len()
                                {
                                    self.panes[pane].selected =
                                        vec![row - 1 + self.panes[pane].view];
                                } else {
                                    self.draw();
                                    return;
                                }
                                library.set_filters(self.rebuild_filters());
                                self.update_from_library();
                            }
                            ZoneEventType::Search => self.search(),
                            ZoneEventType::Insert(before) => {
                                if self.queue_state.active {
                                    self.queue_state.active = false
                                } else {
                                    self.insert_filter(before)
                                }
                            }
                            ZoneEventType::Delete => {
                                if self.queue_state.active {
                                    self.queue_state.active = false
                                } else {
                                    self.delete_filter()
                                }
                            }
                            ZoneEventType::Help => self.message("Help", HELP),
                            ZoneEventType::None => (),
                        },
                        MouseButton::Right => match event {
                            ZoneEventType::Panes {
                                pane,
                                row,
                                column: _column,
                            } => {
                                self.queue_state.active = false;
                                self.panes_index = pane;
                                if row > 0
                                    && row <= self.panes[pane].rect.height as usize - 2
                                    && row <= self.panes[pane].items.len()
                                {
                                    let sel = row - 1 + self.panes[pane].view;
                                    if let Some(pos) =
                                        self.panes[pane].selected.iter().position(|x| *x == sel)
                                    {
                                        self.panes[pane].selected.remove(pos);
                                    } else {
                                        self.panes[pane].selected.push(sel);
                                    }
                                    library.set_filters(self.rebuild_filters());
                                    self.update_from_library();
                                } else {
                                    self.draw()
                                }
                            }
                            _ => (),
                        },
                        MouseButton::Middle => (),
                    },

                    MouseEventKind::ScrollDown => match event {
                        ZoneEventType::Panes { pane, .. } => {
                            self.scroll_view_down(Pane::Panes(pane));
                            self.queue_state.active = false;
                            self.panes_index = pane;
                            self.draw();
                        }
                        _ => (),
                    },
                    MouseEventKind::ScrollUp => match event {
                        ZoneEventType::Panes { pane, .. } => {
                            self.scroll_view_up(Pane::Panes(pane));
                            self.queue_state.active = false;
                            self.panes_index = pane;
                            self.draw();
                        }
                        _ => (),
                    },

                    _ => (),
                },
                _ => (),
            },
            // # Mouse Events # }}}
            Event::Resize(..) => self.draw(),
            _ => (),
        }
    }
    // ## process_event ## }}}
}

// ### UI ### }}}

// ### tui ### {{{
pub fn tui(library: Arc<Library>) {
    let mut libevt_r = library.get_receiver();
    l2!("Entering interactive terminal...");
    let log_level = LOG_LEVEL.swap(0, LOG_ORD); // TODO: better solution?

    terminal::enable_raw_mode().unwrap();
    let mut stdo = io::stdout();

    queue!(
        stdo,
        terminal::EnterAlternateScreen,
        terminal::Clear(terminal::ClearType::All),
        event::EnableMouseCapture,
        cursor::Hide
    )
    .unwrap();

    let ui = Arc::new(Mutex::new(UI::from_library(
        library,
        Terminal::new(CrosstermBackend::new(io::stdout())).unwrap(),
        Theme::new(Color::Yellow),
        log_level > 0,
    )));

    let uiw_libevt = Arc::downgrade(&ui);

    // end signals
    let (tui_s, tui_r) = channel::bounded::<()>(1);
    let libevt_s = tui_s.clone();

    let _event_jh = thread::spawn(move || {
        loop {
            if let Some(ev) = get_event(None) {
                if ev == km_c!('c') {
                    break;
                }
                // process_event will draw for us
                ui.lock().unwrap().process_event(ev);
            }
        }
        tui_s.send(())
    });

    let _libevt_jh = thread::spawn(move || {
        loop {
            match libevt_r.recv() {
                Ok(action) => match uiw_libevt.upgrade() {
                    Some(ui) => match action {
                        LibEvt::Volume => ui.lock().unwrap().draw(),
                        LibEvt::Play => ui.lock().unwrap().draw(),
                        LibEvt::Pause => ui.lock().unwrap().draw(),
                        LibEvt::Stop => ui.lock().unwrap().draw(),
                        LibEvt::Filter => ui.lock().unwrap().update_from_library(),
                    },
                    None => break,
                },
                Err(_) => break,
            }
        }
        libevt_s.send(())
    });

    drop(tui_r.recv());

    queue!(
        stdo,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0),
        terminal::LeaveAlternateScreen,
        event::DisableMouseCapture,
        cursor::Show
    )
    .unwrap();
    stdo.flush().unwrap();
    terminal::disable_raw_mode().unwrap();

    LOG_LEVEL.store(log_level, LOG_ORD);
}
// ### tui ### }}}
