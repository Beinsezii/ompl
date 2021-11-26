use std::cmp::{max, min};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use crate::library::{Filter, Library};
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

// ### FNs ### }}}

pub const HELP: &str = &"\
QUEUE GO HERE
Ctrl+c - Exit
a - Play/Pause
x - Stop
n - Next
v/V - Volume Increase/Decrease
h/j/k/l - left/down/up/right
s - [De]select active item
Tab - [De]select queue
";

// ### UI ### {{{

struct FilterPane {
    tag: String,
    items: Vec<String>,
    index: usize,
    selected: Vec<usize>,
}

struct UI {
    panes: Vec<FilterPane>,
    panes_index: usize,
    queue_sel: bool,
    queue_pos: usize,
}

impl UI {
    fn from_library(library: &Arc<Library>) -> Self {
        let mut result = Self {
            panes_index: 0,
            queue_sel: false,
            queue_pos: 0,
            panes: Vec::new(),
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
                let items = crate::library::tags_from_tracks(&f.filter.tag, &tracks);
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

    let accent = Color::Yellow;
    let style_base = Style::default();
    let style_base_hi = Style::default().fg(Color::Black).bg(Color::White);
    let style_active = Style::default().fg(accent);
    let style_active_hi = Style::default().fg(Color::Black).bg(accent);

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
                    Paragraph::new(HELP).style(if ui.queue_sel {
                        style_active
                    } else {
                        style_base
                    }),
                    queue,
                );
                let status = zones[0];
                f.render_widget(
                    Paragraph::new(format!("Vol: {:.2}", library.volume_get())),
                    status,
                );
                for (num, fp) in ui.panes.iter().enumerate() {
                    f.render_widget(
                        List::new(
                            fp.items
                                .iter()
                                .enumerate()
                                .map(|(n, i)| {
                                    let mut style = if num == ui.panes_index && !ui.queue_sel {
                                        if fp.selected.contains(&n) {
                                            style_active_hi
                                        } else {
                                            style_active
                                        }
                                    } else {
                                        if fp.selected.contains(&n) {
                                            style_base_hi
                                        } else {
                                            style_base
                                        }
                                    };
                                    if n == fp.index {
                                        style = style.add_modifier(Modifier::UNDERLINED);
                                        if num == ui.panes_index && !ui.queue_sel {
                                            style = style.add_modifier(Modifier::BOLD);
                                        }
                                    }
                                    ListItem::new(text::Span {
                                        content: i.into(),
                                        style,
                                    })
                                })
                                .collect::<Vec<ListItem>>(),
                        )
                        .block(
                            Block::default()
                                .border_type(widgets::BorderType::Plain)
                                .borders(Borders::ALL)
                                .title(text::Span {
                                    content: fp.tag.as_str().into(),
                                    style: if fp.selected.is_empty() {
                                        style_base_hi
                                    } else {
                                        style_base
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
                            ui.queue_pos = max(ui.queue_pos + 1, 5)
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

                    km!('s') => {
                        let pane = ui.active_pane_mut();
                        match pane.selected.iter().position(|i| i == &pane.index) {
                            Some(p) => drop(pane.selected.remove(p)),
                            None => pane.selected.push(pane.index),
                        }
                        library.set_filters(ui.rebuild_filters());
                        ui.update_from_library(&library);
                        break 'poller;
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
