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
    execute, queue, terminal,
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
* I/D - Insert/Delete filter
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
    Panes { pane: usize, index: usize },

    VolAdd,
    VolSub,
    Next,
    Stop,
    PlayPause,
    Prev,

    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ZoneEvent {
    kind: MouseEventKind,
    mods: KeyModifiers,
    event: ZoneEventType,
}

// ## Events ## }}}

// ## Bar ## {{{

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Bar {
    parent: Rect,
    help: Rect,
    vol_div: Rect,
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

impl Bar {
    pub fn from_rect(rect: Rect) -> Self {
        let s = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(2), // help
                Constraint::Length(3), // vol_div
                Constraint::Length(9), // vol_stat
                Constraint::Length(1), // vol_sub
                Constraint::Length(1), // vol_add
                Constraint::Length(3), // control_div
                Constraint::Length(2), // prev
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
            help: s[0],
            vol_div: s[1],
            vol_stat: s[2],
            vol_sub: s[3],
            vol_add: s[4],
            control_div: s[5],
            prev: s[6],
            stop: s[8],
            play_pause: s[10],
            next: s[12],
            track_div: s[13],
            track: s[14],
        }
    }
    pub fn draw<T: Backend>(&self, frame: &mut tui::terminal::Frame<T>, library: &Arc<Library>) {
        frame.render_widget(Paragraph::new(" ?"), self.help);
        frame.render_widget(Paragraph::new(" | "), self.vol_div);
        frame.render_widget(
            Paragraph::new(format!("Vol {:.2} ", library.volume_get())),
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
    bar: Bar,
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
            bar: Bar::default(),
            panes: Vec::new(),
            panes_index: 0,
            queue: Vec::new(),
            queue_sel: false,
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
        if let Some(_ft) = filter_tree.iter().last() {
            self.queue = library.get_queue();
            crate::library::sort_by_tag("title", &mut self.queue)
        }
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

    fn active_pane(&self) -> &FilterPane {
        &self.panes[self.panes_index]
    }
    fn active_pane_mut(&mut self) -> &mut FilterPane {
        &mut self.panes[self.panes_index]
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
                        Constraint::Percentage(50),
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
                    .split(zones[1])
                    .into_iter()
                    .enumerate()
                {
                    self.panes[n].rect = r;
                }
                self.queue_rect = zones[2];
                f.render_widget(
                    self.build_list(Pane::Queue).block(
                        Block::default()
                            .border_type(widgets::BorderType::Plain)
                            .borders(Borders::ALL)
                            .title("Queue"),
                    ),
                    self.queue_rect,
                );
                self.bar = Bar::from_rect(zones[0]);
                self.bar.draw(f, library);
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

    pub fn get_input(&mut self, display: &str) -> String {
        let mut result = String::new();
        let mut terminal = self.terminal.take().unwrap();

        terminal
            .draw(|f| {
                f.render_widget(
                    Block::default()
                        .border_type(widgets::BorderType::Plain)
                        .borders(Borders::ALL)
                        .title(display),
                    f.size(),
                )
            })
            .unwrap();

        let mut stdo = std::io::stdout();
        execute!(
            stdo,
            cursor::MoveTo(1, 1),
            event::DisableMouseCapture,
            cursor::Show
        )
        .unwrap();
        terminal::disable_raw_mode().unwrap();

        std::io::stdin().read_line(&mut result).unwrap();

        terminal::enable_raw_mode().unwrap();
        execute!(stdo, event::EnableMouseCapture, cursor::Hide).unwrap();

        self.terminal = Some(terminal);
        result
    }

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
            } else if self.bar.parent.intersects(point) {
                if self.bar.vol_sub.intersects(point) {
                    ZoneEventType::VolSub
                } else if self.bar.vol_add.intersects(point) {
                    ZoneEventType::VolAdd
                } else if self.bar.prev.intersects(point) {
                    ZoneEventType::Prev
                } else if self.bar.stop.intersects(point) {
                    ZoneEventType::Stop
                } else if self.bar.play_pause.intersects(point) {
                    ZoneEventType::PlayPause
                } else if self.bar.next.intersects(point) {
                    ZoneEventType::Next
                } else {
                    ZoneEventType::None
                }
            } else {
                let mut result = ZoneEventType::None;
                for (num, pane) in self.panes.iter().enumerate() {
                    if pane.rect.intersects(point) {
                        result = ZoneEventType::Panes {
                            pane: num,
                            index: event.row.saturating_sub(pane.rect.y).into(),
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
                code: KeyCode::Tab, ..
            }) => self.queue_sel = !self.queue_sel,
            km!('h') => {
                if !self.queue_sel {
                    self.panes_index = self.panes_index.saturating_sub(1)
                }
            }
            km!('l') => {
                if !self.queue_sel {
                    self.panes_index = min(self.panes_index + 1, self.panes.len().saturating_sub(1))
                }
            }
            km!('j') => {
                if self.queue_sel {
                    self.queue_pos = min(self.queue_pos + 1, self.queue.len().saturating_sub(1));
                    self.lock_view(Pane::Queue);
                } else {
                    self.active_pane_mut().index = min(
                        self.active_pane().index + 1,
                        self.active_pane().items.len().saturating_sub(1),
                    );
                    self.lock_view(Pane::Panes(self.panes_index));
                }
            }
            km!('k') => {
                if self.queue_sel {
                    self.queue_pos = self.queue_pos.saturating_sub(1);
                    self.lock_view(Pane::Queue);
                } else {
                    self.active_pane_mut().index = self.active_pane().index.saturating_sub(1);
                    self.lock_view(Pane::Panes(self.panes_index));
                }
            }

            km!('f') => {
                if self.queue_sel {
                    library.play_track(self.queue.get(self.queue_pos).cloned())
                } else {
                    let pane = self.active_pane_mut();
                    match pane.selected.iter().position(|i| i == &pane.index) {
                        Some(p) => drop(pane.selected.remove(p)),
                        None => pane.selected.push(pane.index),
                    }
                    library.set_filters(self.rebuild_filters());
                    self.update_from_library(&library);
                }
            }

            km_s!('F') => {
                if !self.queue_sel {
                    match self.active_pane().selected.is_empty() {
                        true => {
                            self.active_pane_mut().selected =
                                (0..self.active_pane().items.len()).collect()
                        }
                        false => self.active_pane_mut().selected = Vec::new(),
                    }
                }
            }

            km_s!('D') => {
                if !self.queue_sel {
                    if self.panes.len() > 1 {
                        self.panes.remove(self.panes_index);
                        self.panes_index = self.panes_index.saturating_sub(1);
                        library.set_filters(self.rebuild_filters());
                        self.update_from_library(library);
                    }
                }
            }
            km_s!('I') => {
                if !self.queue_sel {
                    let mut filters = self.rebuild_filters();
                    filters.insert(
                        self.panes_index + 1,
                        Filter {
                            tag: self.get_input("Tag to sort by: ").trim().to_string(),
                            items: Vec::new(),
                        },
                    );
                    library.set_filters(filters);
                    self.update_from_library(library);
                    self.panes_index += 1;
                }
            }

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
                                    && index < self.queue.len()
                                {
                                    let index = index - 1 + self.queue_view;
                                    library.play_track(self.queue.get(index).cloned());
                                    self.queue_pos = index;
                                }
                            }
                            ZoneEventType::Panes { pane, index } => {
                                self.queue_sel = false;
                                self.panes_index = pane;
                                if index == 0 {
                                    self.panes[pane].selected = vec![];
                                } else if index <= self.panes[pane].rect.height as usize - 2
                                    && index <= self.panes[pane].items.len()
                                {
                                    self.panes[pane].selected =
                                        vec![index - 1 + self.panes[pane].view];
                                } else {
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
                            ZoneEventType::None => return,
                        },
                        MouseButton::Right => match event {
                            ZoneEventType::Panes { pane, index } => {
                                self.queue_sel = false;
                                self.panes_index = pane;
                                if index > 0 && index <= self.panes[pane].rect.height as usize - 2 {
                                    let sel = index - 1 + self.panes[pane].view;
                                    if let Some(pos) =
                                        self.panes[pane].selected.iter().position(|x| *x == sel)
                                    {
                                        self.panes[pane].selected.remove(pos);
                                    } else {
                                        self.panes[pane].selected.push(sel);
                                    }
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
pub fn tui(library: Arc<crate::library::Library>, cli_recv: Receiver<Action>) {
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
