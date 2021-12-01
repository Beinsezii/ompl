use std::cmp::min;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use crate::library::{Filter, Library, Track};
use crate::{l2, log, Action, LOG_LEVEL, LOG_ORD};

use crossbeam::channel::Receiver;

use crossterm::{
    cursor, event,
    event::{Event, KeyCode, KeyEvent, KeyModifiers /* MouseButton, MouseEvent */},
    queue, terminal,
};

use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
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
            modifiers: _,
        })
    };
}

/// Can't assign modifiers with $mod for some reason
macro_rules! km_c {
    ($ch:expr) => {
        Event::Key(KeyEvent {
            code: KeyCode::Char($ch),
            modifiers: KeyModifiers::CONTROL,
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

fn build_list<'a>(
    items: &'a Vec<String>,
    index: usize,
    selected: &'a Vec<usize>,
    active: bool,
    pane_height: usize,
    theme: Theme,
) -> List<'a> {
    List::new(
        items
            .iter()
            .enumerate()
            .skip(min(
                index.saturating_sub(pane_height / 2),
                items.len().saturating_sub(pane_height),
            ))
            .map(|(n, i)| {
                let mut style = if active {
                    if selected.contains(&n) {
                        theme.active_hi
                    } else {
                        theme.active
                    }
                } else {
                    if selected.contains(&n) {
                        theme.base_hi
                    } else {
                        theme.base
                    }
                };
                if n == index {
                    style = style.patch(theme.mod_select);
                    if active {
                        style = style.patch(theme.mod_select_active)
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

// ### FNs ### }}}

pub const HELP: &str = &"\
TUI Controls:
* Ctrl+c - Exit
* a - Play/Pause
* x - Stop
* n - Next
* v/V - Volume Increase/Decrease
* h/j/k/l - left/down/up/right
* f - [De]select active item
* Ctrl+f - [De]select all
* Tab - [De]select queue
";

// ### UI ### {{{

#[derive(Clone, Debug, PartialEq)]
struct FilterPane {
    tag: String,
    items: Vec<String>,
    index: usize,
    selected: Vec<usize>,
}

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

#[derive(Clone, Debug, PartialEq)]
struct UI {
    panes: Vec<FilterPane>,
    panes_index: usize,
    queue: Vec<Arc<Track>>,
    queue_sel: bool,
    queue_pos: usize,
}

impl UI {
    fn from_library(library: &Arc<Library>) -> Self {
        let mut result = Self {
            panes: Vec::new(),
            panes_index: 0,
            queue: Vec::new(),
            queue_sel: false,
            queue_pos: 0,
        };
        result.update_from_library(library);
        result
    }

    fn update_from_library(&mut self, library: &Arc<Library>) {
        let filter_tree = library.get_filter_tree();
        let old_indicies = self.panes.iter().map(|p| p.index).collect::<Vec<usize>>();
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
                }
            })
            .collect();
        if let Some(_ft) = filter_tree.iter().last() {
            self.queue = library.get_queue();
            crate::library::sort_by_tag("title", &mut self.queue)
        }
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
}

// ### UI ### }}}

pub fn tui(library: Arc<crate::library::Library>, cli_recv: Receiver<Action>) {
    let mut ui = UI::from_library(&library);
    let library_weak = Arc::downgrade(&library);
    drop(library);
    l2!("Entering interactive terminal...");
    let log_level = LOG_LEVEL.swap(0, LOG_ORD); // TODO: better solution?

    terminal::enable_raw_mode().unwrap();
    let mut stdo = std::io::stdout();

    queue!(stdo, terminal::EnterAlternateScreen, cursor::Hide).unwrap();

    let backend = CrosstermBackend::new(stdo);
    let mut terminal = Terminal::new(backend).unwrap();

    let theme = Theme::new(Color::Yellow);

    let result = std::panic::catch_unwind(move || 'main: loop {
        // ## Layout ## {{{

        let library = match library_weak.upgrade() {
            Some(l) => l,
            None => break 'main,
        };

        terminal
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
                let panes = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(
                        (0..ui.panes.len())
                            .map(|_| Constraint::Ratio(1, ui.panes.len() as u32))
                            .collect::<Vec<Constraint>>(),
                    )
                    .split(zones[1]);
                let queue = zones[2];
                f.render_widget(
                    build_list(
                        &crate::library::get_all_tag("title", &ui.queue),
                        ui.queue_pos,
                        &Vec::new(),
                        ui.queue_sel,
                        queue.height.saturating_sub(2) as usize,
                        theme,
                    )
                    .block(
                        Block::default()
                            .border_type(widgets::BorderType::Plain)
                            .borders(Borders::ALL)
                            .title("Queue"),
                    ),
                    queue,
                );
                let status = zones[0];
                f.render_widget(
                    Paragraph::new(format!("Vol: {:.2}", library.volume_get())),
                    status,
                );
                let pane_height = panes
                    .get(0)
                    .map_or(0, |p| p.height.saturating_sub(2) as usize);
                for (num, fp) in ui.panes.iter().enumerate() {
                    f.render_widget(
                        build_list(
                            &fp.items,
                            fp.index,
                            &fp.selected,
                            num == ui.panes_index && !ui.queue_sel,
                            pane_height,
                            theme,
                        )
                        .block(
                            Block::default()
                                .border_type(widgets::BorderType::Plain)
                                .borders(Borders::ALL)
                                .title(text::Span {
                                    content: fp.tag.as_str().into(),
                                    style: if fp.selected.is_empty() {
                                        theme.base_hi
                                    } else {
                                        theme.base
                                    },
                                }),
                        ),
                        panes[num],
                    );
                }
            })
            .unwrap();
        drop(library);

        // ## Layout ## }}}

        // you *could* implement a proper event-driven system where you have separate threads for
        // key events, cli events, and updating the UI, but that'd mean redoing damn near
        // everything here to avoid deadlocks

        // ## Event Loop ## {{{
        'poller: loop {
            let library = match library_weak.upgrade() {
                Some(l) => l,
                None => break 'main,
            };
            if let Some(ev) = get_event(Some(Duration::from_millis(50))) {
                match ev {
                    km_c!('c') => {
                        break 'main;
                    }

                    Event::Key(KeyEvent {
                        code: KeyCode::Tab, ..
                    }) => ui.queue_sel = !ui.queue_sel,
                    km!('h') => {
                        if !ui.queue_sel {
                            ui.panes_index = ui.panes_index.saturating_sub(1)
                        }
                    }
                    km!('l') => {
                        if !ui.queue_sel {
                            ui.panes_index =
                                min(ui.panes_index + 1, ui.panes.len().saturating_sub(1))
                        }
                    }
                    km!('j') => {
                        if ui.queue_sel {
                            ui.queue_pos = min(ui.queue_pos + 1, ui.queue.len().saturating_sub(1))
                        } else {
                            ui.active_pane_mut().index = min(
                                ui.active_pane().index + 1,
                                ui.active_pane().items.len().saturating_sub(1),
                            )
                        }
                    }
                    km!('k') => {
                        if ui.queue_sel {
                            ui.queue_pos = ui.queue_pos.saturating_sub(1)
                        } else {
                            ui.active_pane_mut().index = ui.active_pane().index.saturating_sub(1)
                        }
                    }

                    km_c!('f') => {
                        if !ui.queue_sel {
                            match ui.active_pane().selected.is_empty() {
                                true => {
                                    ui.active_pane_mut().selected =
                                        (0..ui.active_pane().items.len()).collect()
                                }
                                false => ui.active_pane_mut().selected = Vec::new(),
                            }
                        }
                    }

                    km!('f') => {
                        if ui.queue_sel {
                            library.play_track(ui.queue.get(ui.queue_pos).cloned())
                        } else {
                            let pane = ui.active_pane_mut();
                            match pane.selected.iter().position(|i| i == &pane.index) {
                                Some(p) => drop(pane.selected.remove(p)),
                                None => pane.selected.push(pane.index),
                            }
                            library.set_filters(ui.rebuild_filters());
                            ui.update_from_library(&library);
                            break 'poller;
                        }
                    }

                    km!('a') => library.play_pause(),
                    km!('x') => library.stop(),
                    km!('n') => library.next(),
                    // KM!('p') => library.previous(),
                    km!('v') => library.volume_add(0.05),
                    km!('V') => library.volume_sub(0.05),
                    _ => (),
                }
                break 'poller;
            }
            if let Ok(action) = cli_recv.try_recv() {
                match action {
                    Action::Volume { .. } => (),
                    Action::Filter { .. } => ui = UI::from_library(&library),
                    _ => continue,
                }
                break 'poller;
            }
        }
        // ## Event Loop ## }}}
    });

    let mut stdo = std::io::stdout();
    queue!(stdo, terminal::LeaveAlternateScreen, cursor::Show).unwrap();
    stdo.flush().unwrap();
    terminal::disable_raw_mode().unwrap();

    LOG_LEVEL.store(log_level, LOG_ORD);

    if let Err(e) = result {
        std::panic::resume_unwind(e)
    };
}
