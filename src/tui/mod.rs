use std::cmp::min;
use std::io;
use std::io::Write;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

use crate::library::{Filter, LibEvt, Library};
use crate::{l2, log, LOG_LEVEL, LOG_ORD};

use crossterm::{
    cursor, event,
    event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    queue, terminal,
};

use tui::backend::{Backend, CrosstermBackend};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Style};
use tui::widgets::{Block, BorderType, Borders, Paragraph};
use tui::Terminal;

mod theme;
use theme::Theme;
mod widgets;
use widgets::ClickableWidget;
use widgets::{tree2view, FilterTreeView, QueueTable};
use widgets::{Clickable, ContainedWidget, Scrollable};

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
                frame.render_widget(Paragraph::new("Insert"), self.insert);
                frame.render_widget(Paragraph::new("[Before]"), self.insert_before);
                frame.render_widget(Paragraph::new(" | "), self.insert_div);
                frame.render_widget(Paragraph::new("Delete"), self.delete);
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

struct UI<T: Backend> {
    lib_weak: Weak<Library>,
    status_bar_area: Rect,
    multi_bar: MultiBar,
    debug_bar: DebugBar,
    panes: FilterTreeView,
    queuetable: QueueTable,
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
        Self {
            lib_weak: Arc::downgrade(&library),
            status_bar_area: Rect::default(),
            multi_bar: MultiBar::default(),
            debug_bar: DebugBar::default(),
            panes: FilterTreeView::new(library.clone()),
            queuetable: QueueTable::new(library.clone()),
            theme,
            terminal: Some(terminal),
            debug,
            draw_count: 0,
        }
    }

    // ## UI Data FNs ## {{{

    #[deprecated]
    fn update_from_library(&mut self) {
        if let Some(library) = self.lib_weak.upgrade() {
            // probably not necessary to create a new one. Could work by having draw() take &mut
            // self and fix errors.
            let mut panes = FilterTreeView::new(library.clone());
            std::mem::swap(&mut panes, &mut self.panes);
            self.panes.index = panes
                .index
                .min(self.panes.positions.len().saturating_sub(1));
            if self.panes.positions.len() == 0 {
                self.queuetable.active = true;
            } else {
                self.panes.active = panes.active;
            }

            // ditto
            let mut queue = QueueTable::new(library.clone());
            std::mem::swap(&mut queue, &mut self.queuetable);
            self.queuetable.active = queue.active;
            self.queuetable.position = queue
                .position
                .min(library.get_queue().len().saturating_sub(1));

            self.draw();
        }
    }

    fn insert(&mut self, before: bool) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };
        if self.queuetable.active && library.filter_count() != 0 {
            let tag = self.multi_bar_input("Tagstring").trim().to_string();
            if !tag.is_empty() {
                library
                    .insert_sort_tagstring(tag, self.queuetable.index + if before { 0 } else { 1 });
                self.queuetable.index = min(
                    self.queuetable.index + if before { 0 } else { 1 },
                    library.get_sort_tagstrings().len().saturating_sub(1),
                );
            }
        } else {
            let tag = self.multi_bar_input("Filter").trim().to_string();
            if !tag.is_empty() {
                library.insert_filter(
                    Filter {
                        tag,
                        items: Vec::new(),
                    },
                    self.panes.index + if before { 0 } else { 1 },
                );
                self.update_from_library();
                self.panes.index = min(
                    self.panes.index + if before { 0 } else { 1 },
                    library.filter_count().saturating_sub(1),
                );
            }
            self.queuetable.active = false;
            self.panes.active = true;
        }
    }

    fn delete(&mut self) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };
        if self.queuetable.active {
            library.remove_sort_tagstring(self.queuetable.index);
            self.queuetable.index = self.queuetable.index.saturating_sub(1);
        } else {
            library.remove_filter(self.panes.index);
            self.update_from_library();
        }
    }

    // ## UI Data FNs ## }}}

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
                        Constraint::Length(if library.filter_count() == 0 {
                            0
                        } else {
                            size.height
                                .saturating_sub(2 + if self.debug { 1 } else { 1 })
                                / 2
                        }),
                        Constraint::Min(1),
                    ])
                    .split(size);
                self.status_bar_area = zones[0];
                f.render_widget(
                    widgets::StatusBar::new(&library, "title"),
                    self.status_bar_area,
                );
                self.panes.area = zones[3];
                self.panes.draw(f, self.theme);
                self.queuetable.area = zones[4];
                self.queuetable.draw(f, self.theme);
                let bar_mode = self.multi_bar.mode.clone();
                self.multi_bar = MultiBar::from_rect(zones[1]);
                self.multi_bar.mode = bar_mode;
                self.multi_bar.draw(f);
                self.debug_bar = DebugBar::from_rect(zones[2]);
                self.debug_bar.draw(f, self.draw_count);
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
                let (index, items) = if self.queuetable.active {
                    return;
                    // (
                    //     &mut self.queuetable.position,
                    //     self.lib_weak
                    //         .upgrade()
                    //         .unwrap()
                    //         .get_taglist_sort(&self.queue_state.tagstring),
                    // )
                } else {
                    if let Some(library) = self.lib_weak.upgrade() {
                        let (tags, data) =
                            tree2view(library.get_filter_tree(), library.get_tracks());
                        (
                            &mut self.panes.positions[self.panes.index],
                            crate::library::get_taglist_sort(
                                &tags[self.panes.index],
                                &data[self.panes.index],
                            ),
                        )
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
                            // self.lock_view(view);
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
                if let Some(library) = self.lib_weak.upgrade() {
                    let queue = self.queuetable.active;

                    if [
                        widgets::StatusBar::process_event(event, self.status_bar_area, &library),
                        self.panes.process_event(event),
                        self.queuetable.process_event(event),
                    ]
                    .iter()
                    .any(|r| *r)
                    // if you use || it can early return???
                    {
                        if !self.queuetable.active && !self.panes.active {
                            match queue {
                                true => self.queuetable.active = true,
                                false => self.panes.active = true,
                            }
                        }
                        self.draw()
                    }
                }
                ZoneEventType::None
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
                if library.filter_count() != 0 {
                    self.panes.active = !self.panes.active;
                    self.queuetable.active = !self.queuetable.active;
                }
                self.draw();
            }
            km!('h')
            | Event::Key(KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queuetable.active {
                    self.queuetable.index = self.queuetable.index.saturating_sub(1);
                    self.draw()
                }
                {
                    self.panes.index = self.panes.index.saturating_sub(1);
                    self.draw();
                }
            }
            km!('l')
            | Event::Key(KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queuetable.active {
                    self.queuetable.index = (self.queuetable.index + 1)
                        .min(library.get_sort_tagstrings().len().saturating_sub(1));
                    self.draw();
                } else {
                    self.panes.index =
                        (self.panes.index + 1).min(library.filter_count().saturating_sub(1));
                    self.draw();
                }
            }
            km!('j')
            | Event::Key(KeyEvent {
                code: KeyCode::Down,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queuetable.active {
                    self.queuetable.scroll_by_n_lock(1)
                } else {
                    self.panes.scroll_by_n_lock(1)
                }
                self.draw();
            }
            km_s!('J')
            | Event::Key(KeyEvent {
                code: KeyCode::Down,
                modifiers: KeyModifiers::SHIFT,
            }) => {
                if self.queuetable.active {
                    self.queuetable.scroll_down();
                    self.queuetable.scroll_by_n_lock(0);
                } else {
                    self.panes.scroll_down();
                    self.panes.scroll_by_n_lock(0);
                }
                self.draw();
            }
            km!('k')
            | Event::Key(KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queuetable.active {
                    self.queuetable.scroll_by_n_lock(-1)
                } else {
                    self.panes.scroll_by_n_lock(-1)
                }
                self.draw();
            }
            km_s!('K')
            | Event::Key(KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::SHIFT,
            }) => {
                if self.queuetable.active {
                    self.queuetable.scroll_up();
                    self.queuetable.scroll_by_n_lock(0)
                } else {
                    self.panes.scroll_up();
                    self.panes.scroll_by_n_lock(0);
                }
                self.draw();
            }
            km!('f')
            | Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
            }) => {
                if self.queuetable.active {
                    library.play_track(library.get_queue().get(self.queuetable.position).cloned())
                } else {
                    self.panes.toggle_current()
                }
            }

            // shift enter no worky???
            km_s!('F')
            | Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::SHIFT,
            }) => {
                if !self.queuetable.active {
                    self.panes.select_current()
                }
            }

            km!('v') => {
                if !self.queuetable.active {
                    self.panes.invert_selection()
                }
            }
            km_s!('V') => {
                if !self.queuetable.active {
                    self.panes.deselect_all()
                }
            }

            km_s!('D') => self.delete(),
            km!('i') => self.insert(false),
            km_s!('I') => self.insert(true),
            km!('?') => self.message("Help", HELP),
            km!('/') => self.search(),

            km!('a') => library.play_pause(),
            km!('x') => library.stop(),
            km!('n') => library.next(),
            km!('p') => library.previous(),
            km!('=') => library.volume_add(0.05),
            km!('-') => library.volume_sub(0.05),

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
                            ZoneEventType::Search => self.search(),
                            ZoneEventType::Insert(before) => self.insert(before),
                            ZoneEventType::Delete => self.delete(),
                            ZoneEventType::Help => self.message("Help", HELP),
                            ZoneEventType::None => (),
                        },
                        MouseButton::Right => (),
                        MouseButton::Middle => (),
                    },

                    MouseEventKind::ScrollDown => (),
                    MouseEventKind::ScrollUp => (),

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
    ui.lock().unwrap().draw();

    let uiw_libevt = Arc::downgrade(&ui);

    let egg = Arc::new(true);
    let egg_tui = egg.clone();
    let egg_evt = egg.clone();

    thread::spawn(move || {
        let _egg_tui = egg_tui;
        loop {
            if let Some(ev) = get_event(None) {
                if ev == km_c!('c') || ev == km_c!('q') {
                    break;
                }
                // process_event will draw for us
                ui.lock().unwrap().process_event(ev);
            }
        }
    });

    thread::spawn(move || {
        let _egg_evt = egg_evt;
        loop {
            match libevt_r.recv() {
                Ok(action) => match uiw_libevt.upgrade() {
                    Some(ui) => match action {
                        LibEvt::Volume => ui.lock().unwrap().draw(),
                        LibEvt::Play => ui.lock().unwrap().draw(),
                        LibEvt::Pause => ui.lock().unwrap().draw(),
                        LibEvt::Stop => ui.lock().unwrap().draw(),
                        LibEvt::Sort => ui.lock().unwrap().draw(),
                        LibEvt::Filter(resized) => {
                            if resized {
                                ui.lock().unwrap().update_from_library()
                            } else {
                                ui.lock().unwrap().draw()
                            }
                        }
                    },
                    None => break,
                },
                Err(_) => break,
            }
        }
    });

    // waits for any thread to drop the egg and die.
    while Arc::strong_count(&egg) == 3 {
        std::thread::sleep(std::time::Duration::from_millis(50))
    }

    // lets you read panic messages
    // yes this is the dumbest solution
    if log_level > 0 {
        std::thread::sleep(std::time::Duration::from_secs(5));
    }

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
