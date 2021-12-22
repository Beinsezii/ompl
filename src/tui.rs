use std::cmp::min;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use crate::library::{Filter, Library, Track};
use crate::{l2, log, Action, LOG_LEVEL, LOG_ORD};

use crossbeam::channel::Receiver;

use crossterm::{
    cursor, event,
    event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    queue, terminal,
};

use tui::backend::{Backend, CrosstermBackend};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::text;
use tui::widgets;
use tui::widgets::{Block, Borders, List, ListItem, Paragraph};
use tui::Terminal;

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

// ## Theme ## {{{

#[derive(Clone, Copy, Debug, PartialEq)]
struct Theme {
    base: Style,
    base_hi: Style,
    active: Style,
    active_hi: Style,
    mod_select: Style,
    mod_select_active: Style,
}

impl Theme {
    fn new(accent: Color) -> Self {
        Self {
            base: Style::default(),
            base_hi: Style::default().fg(Color::Black).bg(Color::White),
            active: Style::default().fg(accent),
            active_hi: Style::default().fg(Color::Black).bg(accent),
            mod_select: Style::default().add_modifier(Modifier::UNDERLINED),
            mod_select_active: Style::default().add_modifier(Modifier::BOLD),
        }
    }
}

// ## Theme ## }}}

// ## Events ## {{{

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZoneEventType {
    Queue(usize),
    Panes {
        pane: usize,
        row: usize,
        column: usize,
    },

    VolAdd,
    VolSub,
    Next,
    Stop,
    PlayPause,
    Prev,

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

// ## StatusBar ## {{{

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StatusBar {
    parent: Rect,
    vol_stat: Rect,
    vol_sub: Rect,
    vol_add: Rect,
    control_div: Rect,
    prev: Rect,
    stop: Rect,
    play_pause: Rect,
    next: Rect,
    track_div: Rect,
    track: Rect,
}

impl StatusBar {
    pub fn from_rect(rect: Rect) -> Self {
        let s = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(10), // vol_stat
                Constraint::Length(1),  // vol_sub
                Constraint::Length(1),  // vol_add
                Constraint::Length(3),  // control_div
                Constraint::Length(2),  // prev
                Constraint::Length(1),
                Constraint::Length(1), // stop
                Constraint::Length(1),
                Constraint::Length(3), // play_pause
                Constraint::Length(1),
                Constraint::Length(2), // next
                Constraint::Length(3), // track_div
                Constraint::Min(0),    // track
            ])
            .split(rect);
        Self {
            parent: rect,
            vol_stat: s[0],
            vol_sub: s[1],
            vol_add: s[2],
            control_div: s[3],
            prev: s[4],
            stop: s[6],
            play_pause: s[8],
            next: s[10],
            track_div: s[11],
            track: s[12],
        }
    }
    pub fn draw<T: Backend>(&self, frame: &mut tui::terminal::Frame<T>, library: &Arc<Library>) {
        frame.render_widget(
            Paragraph::new(format!(" Vol {:.2} ", library.volume_get())),
            self.vol_stat,
        );
        frame.render_widget(Paragraph::new("-"), self.vol_sub);
        frame.render_widget(Paragraph::new("+"), self.vol_add);
        frame.render_widget(Paragraph::new(" | "), self.control_div);
        frame.render_widget(Paragraph::new(":<"), self.prev);
        frame.render_widget(Paragraph::new("#"), self.stop);
        frame.render_widget(Paragraph::new("::>"), self.play_pause);
        frame.render_widget(Paragraph::new(">:"), self.next);
        frame.render_widget(Paragraph::new(" | "), self.track_div);
        frame.render_widget(
            Paragraph::new(
                library
                    .track_get()
                    .map(|t| t.tags().get("title").cloned())
                    .flatten()
                    .unwrap_or("???".to_owned()),
            ),
            self.track,
        )
    }
}

// ## Bar ## }}}

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
    pub fn draw<T: Backend>(&self, frame: &mut tui::terminal::Frame<T>, _library: &Arc<Library>) {
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

#[derive(Debug)]
struct UI<T: Backend> {
    status_bar: StatusBar,
    multi_bar: MultiBar,
    panes: Vec<FilterPane>,
    panes_index: usize,
    queue: Vec<Arc<Track>>,
    queue_sel: bool,
    queue_pos: usize,
    queue_rect: Rect,
    queue_view: usize,
    theme: Theme,
    terminal: Option<Terminal<T>>,
}

impl<T: Backend> UI<T> {
    fn from_library(library: &Arc<Library>, terminal: Terminal<T>, theme: Theme) -> Self {
        let mut result = Self {
            status_bar: StatusBar::default(),
            multi_bar: MultiBar::default(),
            panes: Vec::new(),
            panes_index: 0,
            queue: Vec::new(),
            queue_sel: true,
            queue_pos: 0,
            queue_rect: Rect::default(),
            queue_view: 0,
            theme,
            terminal: Some(terminal),
        };
        result.update_from_library(library);
        result
    }

    // ## UI Data FNs ## {{{

    fn update_from_library(&mut self, library: &Arc<Library>) {
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
                let items = crate::library::get_all_tag_sort(&f.filter.tag, &tracks);
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
        self.queue = library.get_queue();
        crate::library::sort_by_tag("title", &mut self.queue);
        self.draw(library);
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

    fn insert_filter(&mut self, library: &Arc<Library>, before: bool) {
        if !self.queue_sel || self.panes.is_empty() {
            let tag = self.multi_bar_input("Filter", library).trim().to_string();
            if !tag.is_empty() {
                let mut filters = self.rebuild_filters();
                filters.insert(
                    min(
                        self.panes_index + if before { 0 } else { 1 },
                        self.panes.len().saturating_sub(if before{1} else {0}),
                    ),
                    Filter {
                        tag,
                        items: Vec::new(),
                    },
                );
                library.set_filters(filters);
                self.update_from_library(library);
                self.panes_index = min(
                    self.panes_index + if before { 0 } else { 1 },
                    self.panes.len().saturating_sub(if before{1} else {0}),
                );
            }
        }
    }

    fn delete_filter(&mut self, library: &Arc<Library>) {
        if !self.queue_sel {
            if !self.panes.is_empty() {
                self.panes.remove(self.panes_index);
            }
            self.panes_index = self.panes_index.saturating_sub(1);
            library.set_filters(self.rebuild_filters());
            self.update_from_library(library);
            if self.panes.is_empty() {
                self.queue_sel = true
            }
        }
    }

    fn active_pane_mut(&mut self) -> Option<&mut FilterPane> {
        self.panes.get_mut(self.panes_index)
    }

    // ## UI Data FNs ## }}}

    // ## UI Layout FNs {{{

    fn lock_view(&mut self, pane: Pane) {
        let (index, height, len, view) = match pane {
            Pane::Queue => (
                self.queue_pos,
                self.queue_rect.height,
                self.queue.len(),
                &mut self.queue_view,
            ),
            Pane::Panes(i) => (
                self.panes[i].index,
                self.panes[i].rect.height,
                self.panes[i].items.len(),
                &mut self.panes[i].view,
            ),
        };

        *view = min(
            index.saturating_sub(height.saturating_sub(2) as usize / 2),
            len.saturating_sub(height.saturating_sub(2) as usize),
        )
    }

    fn scroll_view_down(&mut self, pane: Pane) {
        let (height, len, view) = match pane {
            Pane::Queue => (
                self.queue_rect.height,
                self.queue.len(),
                &mut self.queue_view,
            ),
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
            Pane::Queue => &mut self.queue_pos,
            Pane::Panes(i) => &mut self.panes[i].index,
        };
        *index = min(*index + offset, len - 1);
    }

    fn scroll_view_up(&mut self, pane: Pane) {
        let (height, index) = match pane {
            Pane::Queue => (self.queue_rect.height, &mut self.queue_pos),
            Pane::Panes(i) => (self.panes[i].rect.height, &mut self.panes[i].index),
        };
        let offset = height.saturating_sub(2) as usize / 2;
        *index = index.saturating_sub(offset);

        // Rust doesn't allow mutable references to separate fields at once
        let view = match pane {
            Pane::Queue => &mut self.queue_view,
            Pane::Panes(i) => &mut self.panes[i].view,
        };
        *view = view.saturating_sub(offset);
    }

    fn build_list<'a>(&self, pane: Pane) -> List<'a> {
        let (items, skip, index, active, selected) = match pane {
            Pane::Queue => (
                crate::library::get_all_tag("title", &self.queue),
                self.queue_view,
                self.queue_pos,
                self.queue_sel,
                Vec::new(),
            ),
            Pane::Panes(i) => (
                self.panes[i].items.clone(),
                self.panes[i].view,
                self.panes[i].index,
                !self.queue_sel && i == self.panes_index,
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
    fn draw(&mut self, library: &Arc<Library>) {
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
                    .split(zones[2])
                    .into_iter()
                    .enumerate()
                {
                    self.panes[n].rect = r;
                }
                self.queue_rect = zones[3];
                f.render_widget(
                    self.build_list(Pane::Queue).block(
                        Block::default()
                            .border_type(widgets::BorderType::Plain)
                            .borders(Borders::ALL)
                            .title("Queue"),
                    ),
                    self.queue_rect,
                );
                self.status_bar = StatusBar::from_rect(zones[0]);
                self.status_bar.draw(f, library);
                let bar_mode = self.multi_bar.mode.clone();
                self.multi_bar = MultiBar::from_rect(zones[1]);
                self.multi_bar.mode = bar_mode;
                self.multi_bar.draw(f, library);
                for (num, fp) in self.panes.iter().enumerate() {
                    f.render_widget(
                        self.build_list(Pane::Panes(num)).block(
                            Block::default()
                                .border_type(widgets::BorderType::Plain)
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
                            .border_type(widgets::BorderType::Plain)
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
    }

    pub fn multi_bar_input(&mut self, title: &str, library: &Arc<Library>) -> String {
        let mut result = String::new();
        let esc = loop {
            self.multi_bar.mode = MBDrawMode::Input {
                title: title.to_owned(),
                contents: result.clone(),
                style: self.theme.active,
            };
            self.draw(library);
            if let Some(event) = get_event(None) {
                match event {
                    km_c!('c') => break false,
                    Event::Key(KeyEvent { code, .. }) => match code {
                        KeyCode::Esc => break true,
                        KeyCode::Tab => break true,
                        KeyCode::Enter => break true,
                        KeyCode::Backspace => drop(result.pop()),
                        KeyCode::Char(c) => result.push(c),
                        _ => (),
                    },
                    Event::Mouse(MouseEvent {
                        kind: MouseEventKind::Down(_),
                        ..
                    }) => break false,
                    _ => (),
                }
            };
        };
        self.multi_bar.mode = MBDrawMode::Default;
        self.draw(library);
        if esc {
            result
        } else {
            String::new()
        }
    }

    pub fn search(&mut self, library: &Arc<Library>) {
        match self
            .multi_bar_input("Search", library)
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" => (),
            input => {
                if self.queue_sel {
                    for (n, t) in self.queue.iter().enumerate() {
                        if let Some(tag) = t.tags().get("title") {
                            if tag.trim().to_ascii_lowercase().starts_with(&input) {
                                self.queue_pos = n;
                                self.lock_view(Pane::Queue);
                                break;
                            }
                        }
                    }
                } else {
                    if let Some(pane) = self.active_pane_mut() {
                        for (n, i) in pane.items.iter().enumerate() {
                            if i.trim().to_ascii_lowercase().starts_with(&input) {
                                pane.index = n;
                                self.lock_view(Pane::Panes(self.panes_index));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    // ## Popops ## }}}

    // ## convert_event ## {{{
    fn convert_event(&self, event: MouseEvent) -> ZoneEvent {
        let point = Rect {
            x: event.column,
            y: event.row,
            height: 1,
            width: 1,
        };
        ZoneEvent {
            kind: event.kind,
            mods: event.modifiers,
            event: if self.queue_rect.intersects(point) {
                ZoneEventType::Queue(event.row.saturating_sub(self.queue_rect.y).into())
            } else if self.status_bar.parent.intersects(point) {
                if self.status_bar.vol_sub.intersects(point) {
                    ZoneEventType::VolSub
                } else if self.status_bar.vol_add.intersects(point) {
                    ZoneEventType::VolAdd
                } else if self.status_bar.prev.intersects(point) {
                    ZoneEventType::Prev
                } else if self.status_bar.stop.intersects(point) {
                    ZoneEventType::Stop
                } else if self.status_bar.play_pause.intersects(point) {
                    ZoneEventType::PlayPause
                } else if self.status_bar.next.intersects(point) {
                    ZoneEventType::Next
                } else {
                    ZoneEventType::None
                }
            } else if self.multi_bar.parent.intersects(point) {
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
    fn process_event(&mut self, event: Event, library: &Arc<Library>) {
        match event {
            // # Key Events # {{{
            Event::Key(KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
            }) => self.queue_sel = !self.queue_sel || self.panes.is_empty(),
            km!('h')
            | Event::Key(KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::NONE,
            }) => {
                if !self.queue_sel {
                    self.panes_index = self.panes_index.saturating_sub(1)
                }
            }
            km!('l')
            | Event::Key(KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::NONE,
            }) => {
                if !self.queue_sel {
                    self.panes_index = min(self.panes_index + 1, self.panes.len().saturating_sub(1))
                }
            }
            km!('j')
            | Event::Key(KeyEvent {
                code: KeyCode::Down,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queue_sel {
                    self.queue_pos = min(self.queue_pos + 1, self.queue.len().saturating_sub(1));
                    self.lock_view(Pane::Queue);
                } else if let Some(pane) = self.active_pane_mut() {
                    pane.index = min(pane.index + 1, pane.items.len().saturating_sub(1));
                    self.lock_view(Pane::Panes(self.panes_index));
                }
            }
            km!('k')
            | Event::Key(KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queue_sel {
                    self.queue_pos = self.queue_pos.saturating_sub(1);
                    self.lock_view(Pane::Queue);
                } else if let Some(pane) = self.active_pane_mut() {
                    pane.index = pane.index.saturating_sub(1);
                    self.lock_view(Pane::Panes(self.panes_index));
                }
            }

            km!('f')
            | Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queue_sel {
                    library.play_track(self.queue.get(self.queue_pos).cloned())
                } else if let Some(pane) = self.active_pane_mut() {
                    match pane.selected.iter().position(|i| i == &pane.index) {
                        Some(p) => drop(pane.selected.remove(p)),
                        None => pane.selected.push(pane.index),
                    }
                    library.set_filters(self.rebuild_filters());
                    self.update_from_library(&library);
                }
            }

            // shift enter no worky???
            km_s!('F')
            | Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::SHIFT,
            }) => {
                if !self.queue_sel {
                    if let Some(pane) = self.active_pane_mut() {
                        match pane.selected.is_empty() {
                            true => pane.selected = (0..pane.items.len()).collect(),
                            false => pane.selected = Vec::new(),
                        }
                    }
                }
            }

            km_s!('D') => {
                if self.queue_sel {
                    self.queue_sel = false
                } else {
                    self.delete_filter(library)
                }
            }
            km!('i') => {
                if self.queue_sel {
                    self.queue_sel = false
                } else {
                    self.insert_filter(library, false)
                }
            }
            km_s!('I') => {
                if self.queue_sel {
                    self.queue_sel = false
                } else {
                    self.insert_filter(library, true)
                }
            }

            km!('?') => self.message("Help", HELP),
            km!('/') => self.search(library),

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
                            ZoneEventType::Queue(index) => {
                                self.queue_sel = true;
                                if index > 0
                                    && index <= self.queue_rect.height as usize - 2
                                    && index <= self.queue.len()
                                {
                                    let index = index - 1 + self.queue_view;
                                    library.play_track(self.queue.get(index).cloned());
                                    self.queue_pos = index;
                                }
                            }
                            ZoneEventType::Panes { pane, row, column } => {
                                self.queue_sel = false;
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
                                    self.draw(library);
                                    return;
                                }
                                library.set_filters(self.rebuild_filters());
                                self.update_from_library(&library);
                            }
                            ZoneEventType::VolSub => library.volume_sub(0.05),
                            ZoneEventType::VolAdd => library.volume_add(0.05),
                            ZoneEventType::Prev => library.previous(),
                            ZoneEventType::Stop => library.stop(),
                            ZoneEventType::PlayPause => library.play_pause(),
                            ZoneEventType::Next => library.next(),
                            ZoneEventType::Search => self.search(library),
                            ZoneEventType::Insert(before) => {
                                if self.queue_sel {
                                    self.queue_sel = false
                                } else {
                                    self.insert_filter(library, before)
                                }
                            }
                            ZoneEventType::Delete => {
                                if self.queue_sel {
                                    self.queue_sel = false
                                } else {
                                    self.delete_filter(library)
                                }
                            }
                            ZoneEventType::Help => self.message("Help", HELP),
                            ZoneEventType::None => return,
                        },
                        MouseButton::Right => match event {
                            ZoneEventType::Panes {
                                pane,
                                row,
                                column: _column,
                            } => {
                                self.queue_sel = false;
                                self.panes_index = pane;
                                if row > 0 && row <= self.panes[pane].rect.height as usize - 2 {
                                    let sel = row - 1 + self.panes[pane].view;
                                    if let Some(pos) =
                                        self.panes[pane].selected.iter().position(|x| *x == sel)
                                    {
                                        self.panes[pane].selected.remove(pos);
                                    } else {
                                        self.panes[pane].selected.push(sel);
                                    }
                                    library.set_filters(self.rebuild_filters());
                                    self.update_from_library(&library);
                                } else {
                                    self.draw(library)
                                }
                            }
                            ZoneEventType::Queue(_) => self.queue_sel = true,
                            _ => return,
                        },
                        MouseButton::Middle => return,
                    },

                    MouseEventKind::ScrollDown => match event {
                        ZoneEventType::Queue(_index) => {
                            self.scroll_view_down(Pane::Queue);
                            self.queue_sel = true;
                        }
                        ZoneEventType::Panes { pane, .. } => {
                            self.scroll_view_down(Pane::Panes(pane));
                            self.queue_sel = false;
                            self.panes_index = pane;
                        }
                        _ => return,
                    },
                    MouseEventKind::ScrollUp => match event {
                        ZoneEventType::Queue(_index) => {
                            self.scroll_view_up(Pane::Queue);
                            self.queue_sel = true;
                        }
                        ZoneEventType::Panes { pane, .. } => {
                            self.scroll_view_up(Pane::Panes(pane));
                            self.queue_sel = false;
                            self.panes_index = pane;
                        }
                        _ => return,
                    },

                    _ => return,
                },
                _ => return,
            },
            // # Mouse Events # }}}
            Event::Resize(..) => (),
            _ => return,
        }
        self.draw(library)
    }
    // ## process_event ## }}}
}

// ### UI ### }}}

// ### tui ### {{{
pub fn tui(library: Arc<Library>, cli_recv: Receiver<Action>) {
    let library_weak = Arc::downgrade(&library);
    l2!("Entering interactive terminal...");
    let log_level = LOG_LEVEL.swap(0, LOG_ORD); // TODO: better solution?

    terminal::enable_raw_mode().unwrap();
    let mut stdo = std::io::stdout();

    queue!(
        stdo,
        terminal::EnterAlternateScreen,
        terminal::Clear(terminal::ClearType::All),
        event::EnableMouseCapture,
        cursor::Hide
    )
    .unwrap();

    let ui = Arc::new(std::sync::Mutex::new(UI::from_library(
        &library,
        Terminal::new(CrosstermBackend::new(std::io::stdout())).unwrap(),
        Theme::new(Color::Yellow),
    )));
    drop(library);

    let uiw_cli = Arc::downgrade(&ui);
    let lw_cli = library_weak.clone();

    let (tui_s, tui_r) = std::sync::mpsc::channel::<()>();
    let cli_s = tui_s.clone();

    let _event_jh = std::thread::spawn(move || {
        loop {
            if let Some(ev) = get_event(None) {
                if ev == km_c!('c') {
                    break;
                }
                // process_event will draw for us
                ui.lock().unwrap().process_event(
                    ev,
                    &match library_weak.upgrade() {
                        Some(l) => l,
                        None => break,
                    },
                );
            }
        }
        tui_s.send(())
    });

    let _cli_jh = std::thread::spawn(move || {
        loop {
            match cli_recv.recv() {
                Ok(action) => match uiw_cli.upgrade() {
                    Some(ui) => match lw_cli.upgrade() {
                        Some(library) => {
                            match action {
                                Action::Volume { .. } => (),
                                Action::Filter { .. } => {
                                    ui.lock().unwrap().update_from_library(&library)
                                }
                                Action::Next => (),
                                Action::Previous => (),
                                Action::Exit => break,
                                _ => continue,
                            }
                            ui.lock().unwrap().draw(&library);
                        }
                        None => break,
                    },
                    None => break,
                },
                Err(_) => break,
            }
        }
        cli_s.send(())
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
