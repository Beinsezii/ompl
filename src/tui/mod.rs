use std::cmp::min;
use std::io;
use std::io::Write;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use crate::library::{Filter, LibEvt, Library};
use crate::{l2, log, LOG_LEVEL, LOG_ORD};

use crossterm::{
    cursor, event,
    event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    queue, terminal,
};

use tui::backend::{Backend, CrosstermBackend};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::Color;
use tui::widgets::{Block, Borders, Clear, Paragraph};
use tui::Terminal;

mod theme;
use theme::Theme;
mod widgets;
use widgets::{Clickable, ContainedWidget, Scrollable, Searchable};
use widgets::{FilterTreeView, QueueTable, StatusBar};

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
* ? | show this help
* Ctrl+c/q | exit
* a | play/pause
* x | stop
* n/p | next/previous
* -/+ | volume decrease/increase
* h/j/k/l | left/down/up/right
* g/G scroll to top/bottom
* f | select item
* F | select only item
* v/V | invert/clear selection
* Tab | change focus
* i/I | insert after/before
* D | delete
* / | search
";

// ### UI ### {{{

struct UI<T: Backend> {
    lib_weak: Weak<Library>,
    status_bar: StatusBar,
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
            status_bar: StatusBar::new(&library, "title"),
            panes: FilterTreeView::new(library.clone()),
            queuetable: QueueTable::new(library.clone()),
            theme,
            terminal: Some(terminal),
            debug,
            draw_count: 0,
        }
    }

    // ## UI Data FNs ## {{{

    fn insert(&mut self, before: bool) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };
        if self.queuetable.active && library.filter_count() != 0 {
            let tag = self.input("Tagstring").trim().to_string();
            if !tag.is_empty() {
                let pos = self.queuetable.index + if before { 0 } else { 1 };
                library.insert_sort_tagstring(tag, pos);
                self.queuetable.index =
                    min(pos, library.get_sort_tagstrings().len().saturating_sub(1));
            }
        } else {
            let tag = self.input("Filter").trim().to_string();
            if !tag.is_empty() {
                self.panes.insert(before);
                let pos = self.panes.index + if before { 0 } else { 1 };
                library.insert_filter(
                    Filter {
                        tag,
                        items: Vec::new(),
                    },
                    pos,
                );
                self.panes.index = min(pos, library.filter_count().saturating_sub(1));
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
        } else {
            self.panes.remove();
            library.remove_filter(self.panes.index);
            if library.filter_count() == 0 {
                self.queuetable.active = true;
                self.panes.active = false;
            }
        }
    }

    // ## UI Data FNs ## }}}

    // ## draw ## {{{
    fn draw(&mut self) {
        self.draw_inject(|_| {});
    }

    fn draw_inject<F: FnOnce(&mut tui::terminal::Frame<T>)>(&mut self, injection: F) {
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
                let time_headers = Instant::now();
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

                self.status_bar.area = zones[0];
                self.status_bar.draw(f, self.theme);

                f.render_widget(Paragraph::new("MenuBar Placeholder"), zones[1]);

                let time_headers2 = Instant::now();

                let time_panes = Instant::now();
                self.panes.area = zones[3];
                self.panes.draw(f, self.theme);
                let time_panes2 = Instant::now();

                let time_queue = Instant::now();
                self.queuetable.area = zones[4];
                self.queuetable.draw(f, self.theme);
                let time_queue2 = Instant::now();

                if self.debug {
                    f.render_widget(
                        Paragraph::new(format!(
                            "Draws: {} timeH: {:.2}ms timeP: {:.2}ms timeQ: {:.2}ms",
                            self.draw_count,
                            (time_headers2 - time_headers).as_secs_f64() * 1000.0,
                            (time_panes2 - time_panes).as_secs_f64() * 1000.0,
                            (time_queue2 - time_queue).as_secs_f64() * 1000.0,
                        ))
                        .style(self.theme.base),
                        zones[2],
                    );
                }

                injection(f);
            })
            .unwrap();
        self.terminal = terminal;
    }
    // ## draw ## }}}

    // ## Popops ## {{{

    pub fn message(&mut self, title: &str, message: &str) {
        let message = message.trim();
        loop {
            let style = self.theme.active;
            self.draw_inject(|f| {
                let size = f.size();
                let mut height: u16 = 0;
                let mut width: u16 = 0;
                for line in message.split('\n') {
                    height += 1;
                    width = width.max(line.len() as u16);
                }

                let area = Rect {
                    x: size.width.saturating_sub(width + 2) / 2,
                    y: size.height.saturating_sub(height + 2) / 2,
                    width: (width + 2).min(size.width),
                    height: (height + 2).min(size.height),
                };

                f.render_widget(Clear, area);
                f.render_widget(
                    Paragraph::new(message)
                        .style(style)
                        .block(Block::default().borders(Borders::ALL).title(title)),
                    area,
                )
            });
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
                Some(Event::Resize(..)) => (),
                _ => break,
            }
        }
        self.draw();
    }

    fn input(&mut self, query: &str) -> String {
        let mut result = String::new();

        let submit = 'outer: loop {
            let style = self.theme.active;
            let active = self.queuetable.active;
            self.draw_inject(|f| {
                let size = f.size();
                let area = Rect {
                    x: 0,
                    y: if active {
                        size.height.saturating_sub(3)
                    } else {
                        // -5 -> headers(2), height(3)
                        size.height.saturating_sub(5) / 2
                    },
                    height: 3,
                    // +5 -> borders(2), pad(1), ": "(2)
                    width: ((result.len() + query.len()) as u16 + 5).min(size.width),
                };
                f.render_widget(Clear, area);
                let text = format!("{}: {}", query, result);
                f.render_widget(
                    Paragraph::new(
                        &text[text
                            .len()
                            .saturating_sub(size.width.saturating_sub(2) as usize)..],
                    )
                    .style(style)
                    .block(Block::default().borders(Borders::ALL)),
                    area,
                );
            });

            loop {
                if let Some(event) = get_event(None) {
                    match event {
                        km_c!('c') => break 'outer false,
                        Event::Key(KeyEvent { code, .. }) => match code {
                            KeyCode::Esc => break 'outer false,
                            KeyCode::Enter => break 'outer true,
                            KeyCode::Backspace => drop(result.pop()),
                            KeyCode::Char(c) => result.push(c),
                            _ => continue,
                        },
                        Event::Mouse(MouseEvent {
                            kind: MouseEventKind::Down(_),
                            ..
                        }) => break 'outer false,
                        Event::Resize(..) => break,
                        _ => continue,
                    }
                }
                break;
            }
        };

        self.draw();

        if submit {
            result
        } else {
            String::new()
        }
    }

    fn search(&mut self) {
        match self.input("Search").trim().to_ascii_lowercase().as_str() {
            "" => (),
            input => {
                if self.queuetable.active {
                    self.queuetable.find(input);
                } else {
                    self.panes.find(input);
                };
                self.draw();
            }
        }
    }

    // ## Popops ## }}}

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

            km!('g') => {
                if self.queuetable.active {
                    self.queuetable.scroll_by_n_lock(i32::MIN)
                } else {
                    self.panes.scroll_by_n(i32::MIN)
                };
                self.draw();
            }
            km_s!('G') => {
                if self.queuetable.active {
                    // i32::max will overflow since it gets added to pos. easy avoidance lol.
                    self.queuetable.scroll_by_n_lock(i16::MAX.into())
                } else {
                    self.panes.scroll_by_n(i16::MAX.into())
                };
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
            Event::Mouse(event) => {
                let queue = self.queuetable.active;

                if [
                    self.status_bar.process_event(event),
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

    let libweak_evt = Arc::downgrade(&library);

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
                        LibEvt::Volume | LibEvt::Play | LibEvt::Pause | LibEvt::Stop => {
                            ui.lock().unwrap().draw()
                        }
                        LibEvt::Filter => {
                            let mut uiw = ui.lock().unwrap();
                            let i = uiw.panes.index;
                            for x in 0..libweak_evt.upgrade().unwrap().filter_count() {
                                uiw.panes.index = x;
                                uiw.panes.scroll_by_n(0);
                            }
                            uiw.panes.index = i;
                            uiw.draw();
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
    if log_level > 1 {
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
