#![warn(missing_docs)]

use std::cmp::min;
use std::io::{stdout, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use crate::library::{Color, Filter, LibEvt, Library, Theme};
use crate::{logging::*, parse_art_size, parse_time};

#[cfg(feature = "clipboard")]
use arboard::Clipboard;

use ratatui::crossterm::{
    cursor, event,
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind},
    queue, terminal,
};

use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Terminal;

mod stylesheet;
use stylesheet::StyleSheet;
mod widgets;
use widgets::{Art, Clickable, ContainedWidget, FilterPanes, MTree, MenuBar, Scrollable, Searchable, Seeker, SortPanes, StatusBar};

// ### FNs ### {{{

/// easy matching key events
macro_rules! km {
    ($ch:expr) => {
        Event::Key(KeyEvent {
            code: KeyCode::Char($ch),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            ..
        })
    };
}

// Can't assign modifiers with $mod for some reason
macro_rules! km_c {
    ($ch:expr) => {
        Event::Key(KeyEvent {
            code: KeyCode::Char($ch),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            ..
        })
    };
}

macro_rules! km_s {
    ($ch:expr) => {
        Event::Key(KeyEvent {
            code: KeyCode::Char($ch),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            ..
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
* 0-9 | navigate top menu
* Ctrl+c/q | exit program
* Ctrl+z | exit only TUI
* a | play/pause
* x | stop
* n/p | next/previous
* -/+ | volume decrease/increase
* .,>< | seek 5/30 seconds
* e | toggle shuffle
* r | toggle repeat
* h/j/k/l | left/down/up/right
* H/L | move panes
* g/G | scroll to top/bottom
* z | focus playing
* f | select item
* F | select only item
* v/V | invert/clear selection
* Tab | change focus
* i/I | insert after/before
* D | delete
* / | search
* ' | edit
* c | toggle art view

* input
  * Ctrl-y/p | copy/paste
  * Ctrl-x | delete word
";

// ### UI ### {{{

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    // general
    Debug,
    Draw,
    Help,
    None,

    // Library
    SeekTo,
    Statusline,
    ACC,
    FG,
    BG,
    ArtSize,
    ArtView,
    Append,
    Purge,

    // Active pane
    Delete,
    Edit,
    InsertAfter,
    InsertBefore,
    MoveLeft,
    MoveRight,
}

struct UI<T: Backend> {
    lib_weak: Weak<Library>,
    menubar: MenuBar<Action>,
    status_bar: StatusBar,
    seeker: Seeker,
    art: Art,
    filterpanes: FilterPanes,
    sortpanes: SortPanes,
    stylesheet: StyleSheet,
    terminal: Option<Terminal<T>>,
    art_inspect: bool,
    debug: bool,
    draw_count: u128,
    #[cfg(feature = "clipboard")]
    clipboard: Option<Clipboard>,
}

impl<T: Backend> UI<T> {
    fn from_library(library: Arc<Library>, terminal: Terminal<T>, stylesheet: StyleSheet) -> Self {
        let tree = MTree::Tree(vec![
            (String::from("Help"), MTree::Action(Action::Help)),
            (
                String::from("Pane"),
                MTree::Tree(vec![
                    (String::from("Insert <-"), MTree::Action(Action::InsertBefore)),
                    (String::from("Insert ->"), MTree::Action(Action::InsertAfter)),
                    (String::from("Move <-"), MTree::Action(Action::MoveLeft)),
                    (String::from("Move ->"), MTree::Action(Action::MoveRight)),
                    (String::from("Edit"), MTree::Action(Action::Edit)),
                    (String::from("Delete"), MTree::Action(Action::Delete)),
                ]),
            ),
            (
                String::from("Library"),
                MTree::Tree(vec![
                    (String::from("Seek To"), MTree::Action(Action::SeekTo)),
                    (String::from("Statusline"), MTree::Action(Action::Statusline)),
                    (String::from("Append"), MTree::Action(Action::Append)),
                    (String::from("Purge"), MTree::Action(Action::Purge)),
                ]),
            ),
            (
                String::from("Theme"),
                MTree::Tree(vec![
                    (String::from("Foreground"), MTree::Action(Action::FG)),
                    (String::from("Background"), MTree::Action(Action::BG)),
                    (String::from("Accent"), MTree::Action(Action::ACC)),
                    (String::from("Art Size"), MTree::Action(Action::ArtSize)),
                ]),
            ),
            (String::from("Debug"), MTree::Action(Action::Debug)),
        ]);

        let debug = if LOG_LEVEL.load(Ordering::Relaxed) >= 3 { true } else { false };

        Self {
            lib_weak: Arc::downgrade(&library),
            menubar: MenuBar::new(tree),
            status_bar: StatusBar::new(&library),
            seeker: Seeker::new(&library),
            art: Art::new(&library),
            filterpanes: FilterPanes::new(library.clone()),
            sortpanes: SortPanes::new(library.clone()),
            stylesheet,
            terminal: Some(terminal),
            art_inspect: false,
            debug,
            draw_count: 0,
            #[cfg(feature = "clipboard")]
            clipboard: Clipboard::new().ok(),
        }
    }

    // ## Action FNs ## {{{

    // # insert # {{{
    fn insert(&mut self, before: bool) {
        let Some(library) = self.lib_weak.upgrade() else { return };
        if self.sortpanes.active() && library.filter_count() != 0 {
            let tag = self.input("Tagstring", "", false).trim().to_string();
            if !tag.is_empty() {
                let pos = self.sortpanes.index() + if before { 0 } else { 1 };
                library.insert_sorter(tag, pos);
                *self.sortpanes.index_mut() = min(pos, library.get_sorters().len().saturating_sub(1));
            }
        } else {
            let tag = self.input("Filter", "", false).trim().to_string();
            if !tag.is_empty() {
                let pos = self.filterpanes.index() + if before { 0 } else { 1 };
                library.insert_filter(Filter { tag, items: Vec::new() }, pos);
                *self.filterpanes.index_mut() = min(pos, library.filter_count().saturating_sub(1));
            }
            *self.sortpanes.active_mut() = false;
            *self.filterpanes.active_mut() = true;
        }
    }
    // # insert # }}}

    // # delete # {{{
    fn delete(&mut self) {
        let Some(library) = self.lib_weak.upgrade() else { return };
        if self.sortpanes.active() {
            library.remove_sorter(self.sortpanes.index());
        } else {
            library.remove_filter(self.filterpanes.index());
            if library.filter_count() == 0 {
                *self.sortpanes.active_mut() = true;
                *self.filterpanes.active_mut() = false;
            }
        }
    }
    // # delete # }}}

    // # move_pane # {{{
    fn move_pane(&mut self, left: bool) {
        let Some(library) = self.lib_weak.upgrade() else { return };

        let s = self.sortpanes.active();

        let (count, index_mut) = if s {
            (library.sort_count(), self.sortpanes.index_mut())
        } else {
            (library.filter_count(), self.filterpanes.index_mut())
        };

        if (left && *index_mut > 0) || (!left && *index_mut < count.saturating_sub(1)) {
            let from = *index_mut;
            if left {
                *index_mut -= 1
            } else {
                *index_mut += 1
            }
            if s {
                let mut items = library.get_sorters();
                items.swap(from, *index_mut);
                library.set_sorters(items);
            } else {
                let mut items = library.get_filters();
                items.swap(from, *index_mut);
                library.set_filters(items);
            }
        }
    }
    // # move_pane # }}}

    // # edit {{{
    fn edit(&mut self) {
        let Some(library) = self.lib_weak.upgrade() else { return };
        let s = self.sortpanes.active();

        let tagstring = if s {
            library.get_sorter(self.sortpanes.index())
        } else {
            library.get_filter(self.filterpanes.index()).map(|f| f.tag)
        };

        let Some(tagstring) = tagstring else { return };

        match self.input("Edit", &tagstring, false).trim().to_ascii_lowercase().as_str() {
            "" => (),
            input => {
                if input != &tagstring {
                    if s {
                        library.set_sorter(self.sortpanes.index(), input.to_string())
                    } else {
                        library.set_filter(
                            self.filterpanes.index(),
                            Filter {
                                tag: input.to_string(),
                                items: vec![],
                            },
                        )
                    }
                }
            }
        }
    }
    // # edit # }}}

    // # search {{{
    fn search(&mut self) {
        match self.input("Search", "", false).trim().to_ascii_lowercase().as_str() {
            "" => (),
            input => {
                if self.sortpanes.active() {
                    self.sortpanes.find(input);
                } else {
                    self.filterpanes.find(input);
                };
                self.draw();
            }
        }
    }
    // # search # }}}

    // ## Action FNs ## }}}

    // ## draw ## {{{
    fn draw(&mut self) {
        self.draw_inject(|_| {});
    }

    fn draw_inject<F: FnOnce(&mut ratatui::Frame)>(&mut self, injection: F) {
        let Some(library) = self.lib_weak.upgrade() else { return };
        let theme = library.theme_get();
        self.draw_count += 1;
        let mut terminal = self.terminal.take();
        terminal
            .as_mut()
            .unwrap()
            .draw(|f| {
                let time_begin = Instant::now();
                let size = f.area();
                let [header, body] = *Layout::vertical([
                    Constraint::Length(if library.seekable().is_some() { 4 } else { 2 }.max(if self.art_inspect {
                        0
                    } else {
                        theme.art_size.into()
                    })),
                    Constraint::Min(1),
                ])
                .split(size) else {
                    return;
                };
                let [action_area, art_area] = *Layout::horizontal([
                    Constraint::Min(1),
                    if self.art_inspect {
                        Constraint::Length(0)
                    } else {
                        Constraint::Length((theme.art_size * 2).into())
                    },
                ])
                .split(header) else {
                    return;
                };
                let [status_bar_area, menubar_area, debug_area, seeker_area] = *Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(if self.debug { 1 } else { 0 }),
                    Constraint::Min(0),
                ])
                .split(action_area) else {
                    return;
                };
                let [art_area2, filterpanes_area, sortpanes_area] = *Layout::vertical([
                    if self.art_inspect {
                        Constraint::Length(body.height.min(body.width / 2))
                    } else {
                        Constraint::Length(0)
                    },
                    if library.filter_count() == 0 {
                        Constraint::Max(0)
                    } else {
                        Constraint::Min(0)
                    },
                    Constraint::Min(0),
                ])
                .split(body) else {
                    return;
                };

                self.status_bar.render(f.buffer_mut(), status_bar_area, self.stylesheet);
                self.menubar.render(f.buffer_mut(), menubar_area, self.stylesheet);

                let time_bars = Instant::now();

                if let Some(_) = library.seekable() {
                    self.seeker.render(f.buffer_mut(), seeker_area, self.stylesheet)
                }

                let time_seekbar = Instant::now();

                self.art
                    .render(f.buffer_mut(), if self.art_inspect { art_area2 } else { art_area }, self.stylesheet);

                let time_art = Instant::now();

                self.filterpanes.render(f.buffer_mut(), filterpanes_area, self.stylesheet);

                let time_panes = Instant::now();

                self.sortpanes.render(f.buffer_mut(), sortpanes_area, self.stylesheet);

                let time_queue = Instant::now();

                if self.debug {
                    f.render_widget(
                        Paragraph::new(format!(
                            "Draws: {: <4} time: {: <4} timeB: {: <4} timeS: {: <4} timeA: {: <4} timeP: {: <4} timeQ: {: <4}",
                            self.draw_count,
                            time_begin.elapsed().as_micros(),
                            (time_bars - time_begin).as_micros(),
                            (time_seekbar - time_bars).as_micros(),
                            (time_art - time_seekbar).as_micros(),
                            (time_panes - time_art).as_micros(),
                            (time_queue - time_panes).as_micros(),
                        ))
                        .style(self.stylesheet.base),
                        debug_area,
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
            let style = self.stylesheet.active;
            self.draw_inject(|f| {
                let size = f.area();
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
                    kind: MouseEventKind::Moved, ..
                })) => (),
                Some(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(_), ..
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

    fn input(&mut self, query: &str, prefill: &str, header: bool) -> String {
        let mut result = String::from(prefill);

        let Some(library) = self.lib_weak.upgrade() else { return result };
        let art_size = library.theme_get().art_size;
        let header_height: u16 = if library.seekable().is_some() { 4 } else { 2 }.max(if self.art_inspect { 0 } else { art_size.into() });
        const BOX_HEIGHT: u16 = 3;
        // +5 -> borders(2), pad(1), ": "(2)
        const BOX_PAD: u16 = 5;

        let submit = 'outer: loop {
            let style = self.stylesheet.active;
            let sortpanes_active = self.sortpanes.active();
            self.draw_inject(|f| {
                let size = f.area();
                let filterpanes_height = size.height.saturating_sub(header_height).div_ceil(2);
                let area = Rect {
                    x: 0,
                    y: if header {
                        header_height.saturating_sub(BOX_HEIGHT).min(size.height.saturating_sub(BOX_HEIGHT))
                    } else if sortpanes_active {
                        size.height.saturating_sub(BOX_HEIGHT)
                    } else {
                        (filterpanes_height + header_height).min(size.height).saturating_sub(BOX_HEIGHT)
                    },
                    height: BOX_HEIGHT.min(size.height),
                    width: if header {
                        (size.width / 4).max((result.len() + query.len()) as u16 + BOX_PAD).min(size.width)
                    } else {
                        ((result.len() + query.len()) as u16 + BOX_PAD).min(size.width)
                    },
                };
                f.render_widget(Clear, area);
                let text = format!("{}: {}", query, result);
                f.render_widget(
                    Paragraph::new(&text[text.len().saturating_sub(size.width.saturating_sub(2) as usize)..])
                        .style(style)
                        .block(Block::default().borders(Borders::ALL)),
                    area,
                );
            });

            loop {
                if let Some(event) = get_event(None) {
                    match event {
                        km_c!('c') => break 'outer false,
                        #[cfg(feature = "clipboard")]
                        km_c!('p') => {
                            if let Some(clip) = self.clipboard.as_mut() {
                                if let Ok(contents) = clip.get_text() {
                                    result = contents
                                }
                            }
                        }
                        #[cfg(feature = "clipboard")]
                        km_c!('y') => {
                            if let Some(clip) = self.clipboard.as_mut() {
                                drop(clip.set_text(result.clone()));
                            }
                        }
                        // // seriously why the fuck does this print 'h'
                        // Event::Key(KeyEvent {
                        //     code: KeyCode::Backspace,
                        //     modifiers: KeyModifiers::CONTROL,
                        // }) => loop {
                        km_c!('x') => loop {
                            if let Some(c) = result.pop() {
                                if !c.is_alphanumeric() {
                                    break;
                                }
                            } else {
                                break;
                            }
                        },
                        Event::Key(KeyEvent {
                            code,
                            kind: KeyEventKind::Press,
                            ..
                        }) => match code {
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
            String::from(prefill)
        }
    }

    // ## Popops ## }}}

    // ## action ## {{{
    fn action(&mut self, action: Action) {
        match action {
            // General
            Action::Debug => {
                self.debug = !self.debug;
                self.draw()
            }
            Action::ArtView => {
                self.art_inspect = !self.art_inspect;
                self.draw()
            }
            Action::Draw => self.draw(),
            Action::Help => self.message("Help", HELP),
            Action::None => (),

            // Library
            Action::SeekTo => {
                if let Some(library) = self.lib_weak.upgrade() {
                    if library.seekable() == Some(true) {
                        let text = self.input("hh:mm:ss.dd", "", true);
                        if text.len() > 0 {
                            match parse_time(&text) {
                                Ok(time) => library.seek(time),
                                Err(s) => self.message("Could not parse time", &s.to_string()),
                            }
                        }
                    }
                }
            }
            Action::Statusline => {
                if let Some(library) = self.lib_weak.upgrade() {
                    library.statusline_set(self.input("Statusline", &library.statusline_get(), true))
                }
            }
            Action::ACC | Action::FG | Action::BG => {
                if let Some(library) = self.lib_weak.upgrade() {
                    let text = self.input("Hex, terminal/ansi #, space function, or none", "", true);
                    if !text.is_empty() {
                        match Color::try_from(text) {
                            Ok(color) => {
                                let mut theme = library.theme_get();
                                match action {
                                    Action::ACC => theme.acc = color,
                                    Action::FG => theme.fg = color,
                                    Action::BG => theme.bg = color,
                                    _ => unreachable!(),
                                }
                                library.theme_set(theme)
                            }
                            Err(e) => self.message("Error reading text as color: ", &e),
                        }
                    }
                }
            }
            Action::ArtSize => {
                if let Some(library) = self.lib_weak.upgrade() {
                    let theme = library.theme_get();
                    let result = self.input("Set art size 0, 4..5..16", theme.art_size.to_string().as_str(), true);
                    if !result.is_empty() {
                        match parse_art_size(&result) {
                            Ok(uint8) => library.theme_set(Theme { art_size: uint8, ..theme }),
                            Err(e) => self.message("Error setting image size: ", &e.to_string()),
                        }
                    }
                }
            }
            Action::Append => {
                if let Some(library) = self.lib_weak.upgrade() {
                    library.append_library(PathBuf::from(self.input("Path", "", true)));
                }
            }
            Action::Purge => {
                if let Some(library) = self.lib_weak.upgrade() {
                    library.purge();
                }
            }

            // Active Pane
            Action::Delete => self.delete(),
            Action::Edit => self.edit(),
            Action::InsertAfter => self.insert(false),
            Action::InsertBefore => self.insert(true),
            Action::MoveLeft => self.move_pane(true),
            Action::MoveRight => self.move_pane(false),
        }
    }
    // ## action ## }}}

    // ## process_event ## {{{
    fn process_event(&mut self, event: Event) {
        let Some(library) = self.lib_weak.upgrade() else { return };
        match event {
            // # Key Events # {{{
            Event::Key(KeyEvent {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if library.filter_count() != 0 {
                    *self.filterpanes.active_mut() = !self.filterpanes.active();
                    *self.sortpanes.active_mut() = !self.sortpanes.active();
                }
                self.draw();
            }
            km!('h')
            | Event::Key(KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if self.sortpanes.active() {
                    *self.sortpanes.index_mut() = self.sortpanes.index().saturating_sub(1);
                    self.draw()
                }
                {
                    *self.filterpanes.index_mut() = self.filterpanes.index().saturating_sub(1);
                    self.draw();
                }
            }
            km_s!('H')
            | Event::Key(KeyEvent {
                code: KeyCode::Left,
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                ..
            }) => self.move_pane(true),
            km!('l')
            | Event::Key(KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if self.sortpanes.active() {
                    *self.sortpanes.index_mut() = (self.sortpanes.index() + 1).min(library.get_sorters().len().saturating_sub(1));
                    self.draw();
                } else {
                    *self.filterpanes.index_mut() = (self.filterpanes.index() + 1).min(library.filter_count().saturating_sub(1));
                    self.draw();
                }
            }
            km_s!('L')
            | Event::Key(KeyEvent {
                code: KeyCode::Right,
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                ..
            }) => self.move_pane(false),
            km!('j')
            | Event::Key(KeyEvent {
                code: KeyCode::Down,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if self.sortpanes.active() {
                    self.sortpanes.scroll_by_n_lock(1)
                } else {
                    self.filterpanes.scroll_by_n_lock(1)
                }
                self.draw();
            }
            km_s!('J')
            | Event::Key(KeyEvent {
                code: KeyCode::Down,
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if self.sortpanes.active() {
                    self.sortpanes.scroll_down();
                    self.sortpanes.scroll_by_n_lock(0);
                } else {
                    self.filterpanes.scroll_down();
                    self.filterpanes.scroll_by_n_lock(0);
                }
                self.draw();
            }
            km!('k')
            | Event::Key(KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if self.sortpanes.active() {
                    self.sortpanes.scroll_by_n_lock(-1)
                } else {
                    self.filterpanes.scroll_by_n_lock(-1)
                }
                self.draw();
            }
            km_s!('K')
            | Event::Key(KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if self.sortpanes.active() {
                    self.sortpanes.scroll_up();
                    self.sortpanes.scroll_by_n_lock(0)
                } else {
                    self.filterpanes.scroll_up();
                    self.filterpanes.scroll_by_n_lock(0);
                }
                self.draw();
            }

            km!('g') => {
                if self.sortpanes.active() {
                    self.sortpanes.scroll_by_n_lock(i32::MIN)
                } else {
                    self.filterpanes.scroll_by_n(i32::MIN)
                };
                self.draw();
            }
            km_s!('G') => {
                if self.sortpanes.active() {
                    // i32::max will overflow since it gets added to pos. easy avoidance lol.
                    self.sortpanes.scroll_by_n_lock(i16::MAX.into())
                } else {
                    self.filterpanes.scroll_by_n(i16::MAX.into())
                };
                self.draw();
            }

            km!('z') => {
                if let Some(track) = library.track_get() {
                    if self.sortpanes.active() {
                        if let Some(index) = library.get_queue().iter().position(|t| *t == track) {
                            self.sortpanes.scroll_by_n(i32::MIN);
                            self.sortpanes.scroll_by_n_lock(index as i32);
                            self.draw();
                        }
                    } else {
                    }
                }
            }

            km!('f')
            | Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if self.sortpanes.active() {
                    library.play_track(library.get_queue().get(self.sortpanes.position()).cloned())
                } else {
                    self.filterpanes.toggle_current()
                }
            }

            // shift enter no worky???
            km_s!('F')
            | Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if !self.sortpanes.active() {
                    self.filterpanes.select_current()
                }
            }

            km!('v') => {
                if !self.sortpanes.active() {
                    self.filterpanes.invert_selection()
                }
            }
            km_s!('V') => {
                if !self.sortpanes.active() {
                    self.filterpanes.deselect_all()
                }
            }

            km_s!('D') => self.delete(),
            km!('i') => self.insert(false),
            km_s!('I') => self.insert(true),
            km!('/') => self.search(),
            km!('\'') => self.edit(),

            km!('a') => library.play_pause(),
            km!('x') => library.stop(),
            km!('n') => library.next(),
            km!('p') => library.previous(),
            km!('=') => library.volume_add(0.05),
            km!('-') => library.volume_add(-0.05),
            km!('e') => library.shuffle_toggle(),
            km!('r') => library.repeat_toggle(),

            km!('.') => library.seek_by(5.0),
            km!(',') => library.seek_by(-5.0),
            km!('>') => library.seek_by(30.0),
            km!('<') => library.seek_by(-30.0),

            // c for cover I guess..?
            km!('c') => {
                self.art_inspect = !self.art_inspect;
                self.draw()
            }

            // yay vim macros
            km!('0')
            | Event::Key(KeyEvent {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                ..
            }) => {
                self.menubar.up();
                self.draw();
            }
            km!('1') => {
                self.menubar.down(0);
                self.draw();
            }
            km!('2') => {
                self.menubar.down(1);
                self.draw();
            }
            km!('3') => {
                self.menubar.down(2);
                self.draw();
            }
            km!('4') => {
                self.menubar.down(3);
                self.draw();
            }
            km!('5') => {
                self.menubar.down(4);
                self.draw();
            }
            km!('6') => {
                self.menubar.down(5);
                self.draw();
            }
            km!('7') => {
                self.menubar.down(6);
                self.draw();
            }
            km!('8') => {
                self.menubar.down(7);
                self.draw();
            }
            km!('9') => {
                self.menubar.down(8);
                self.draw();
            }

            // # Key Events # }}}

            // # Mouse Events # {{{
            Event::Mouse(event) => {
                let (q, qi, p, pi) = (
                    self.sortpanes.active(),
                    self.sortpanes.index(),
                    self.filterpanes.active(),
                    self.filterpanes.index(),
                );

                let actions = &[
                    self.status_bar.process_event(event),
                    self.menubar.process_event(event),
                    self.seeker.process_event(event),
                    self.art.process_event(event),
                    self.filterpanes.process_event(event),
                    self.sortpanes.process_event(event),
                ];

                let draws = self.draw_count;

                // handle draws later
                for action in actions {
                    if action != &Action::Draw {
                        self.action(*action)
                    }
                }

                // Ensure one pane is always active
                if (!self.sortpanes.active() && !self.filterpanes.active()) || (self.sortpanes.active() && self.filterpanes.active()) {
                    match q {
                        true => (*self.filterpanes.active_mut(), *self.sortpanes.active_mut()) = (false, true),
                        false => (*self.filterpanes.active_mut(), *self.sortpanes.active_mut()) = (true, false),
                    }
                }

                // Hopefully avoid unecessary draws.
                if (actions.iter().any(|r| *r == Action::Draw)
                    || (q, qi, p, pi)
                        != (
                            self.sortpanes.active(),
                            self.sortpanes.index(),
                            self.filterpanes.active(),
                            self.filterpanes.index(),
                        ))
                    && self.draw_count == draws
                {
                    self.draw()
                }
            }
            // # Mouse Events # }}}
            Event::Resize(..) => self.draw(),
            _ => (),
        }

        // Menubar received separately since its type-agnostic
        if let Some(action) = self.menubar.receive() {
            self.action(action)
        }
    }
    // ## process_event ## }}}
}

// ### UI ### }}}

// ### tui ### {{{
pub fn tui(library: Arc<Library>) -> bool {
    let mut libevt_r = library.get_receiver().unwrap();
    debug!("Entering interactive terminal...");
    log_pause!();

    terminal::enable_raw_mode().unwrap();
    let mut stdo = stdout();

    queue!(
        stdo,
        terminal::EnterAlternateScreen,
        terminal::Clear(terminal::ClearType::All),
        event::EnableMouseCapture,
        cursor::Hide
    )
    .unwrap();

    let join = Arc::new(AtomicBool::new(false));
    let libweak_evt = Arc::downgrade(&library);

    let theme = library.theme_get();
    let ui = Arc::new(Mutex::new(UI::from_library(
        library,
        Terminal::new(CrosstermBackend::new(stdout())).unwrap(),
        StyleSheet::from(theme),
    )));
    ui.lock().unwrap().draw();

    let uiw_libevt = Arc::downgrade(&ui);

    let egg = Arc::new(true);
    let egg_tui = egg.clone();
    let egg_evt = egg.clone();

    let join_tui = join.clone();
    thread::Builder::new()
        .name(String::from("TUI Event Poller"))
        .spawn(move || {
            let _egg_tui = egg_tui;
            loop {
                if let Some(ev) = get_event(None) {
                    match ev {
                        km_c!('c') | km_c!('q') => break,
                        km_c!('z') => {
                            join_tui.store(true, Ordering::Relaxed);
                            break;
                        }
                        _ => (),
                    }
                    // process_event will draw for us
                    ui.lock().unwrap().process_event(ev);
                }
            }
        })
        .unwrap();

    thread::Builder::new()
        .name(String::from("TUI LibEvt Receiver"))
        .spawn(move || {
            let _egg_evt = egg_evt;
            loop {
                match libevt_r.recv() {
                    Ok(action) => match uiw_libevt.upgrade() {
                        Some(ui) => match action {
                            LibEvt::Playback | LibEvt::Update => ui.lock().unwrap().draw(),
                            LibEvt::Theme => {
                                if let Ok(mut uiw) = ui.lock() {
                                    if let Some(libw) = libweak_evt.upgrade() {
                                        uiw.stylesheet = StyleSheet::from(libw.theme_get());
                                        uiw.draw()
                                    }
                                }
                            }
                            LibEvt::Error(message) => {
                                let mut uiw = ui.lock().unwrap();
                                uiw.message("Library Error", &message)
                            }
                        },
                        None => break,
                    },
                    Err(_) => break,
                }
            }
        })
        .unwrap();

    // waits for any thread to drop the egg and die.
    while Arc::strong_count(&egg) == 3 {
        std::thread::sleep(std::time::Duration::from_millis(50))
    }

    // lets you read panic messages
    // yes this is the dumbest solution
    if LOG_LEVEL.load(Ordering::Relaxed) > 3 {
        std::thread::sleep(std::time::Duration::from_secs(10));
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

    log_resume!();
    join.load(Ordering::Relaxed)
}
// ### tui ### }}}
