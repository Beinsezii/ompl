use std::io::Write;
use std::time::Duration;

use crate::library::{Command, Response};
use crate::{l2, log, LOG_LEVEL, LOG_ORD};

use crossbeam::channel::{Receiver, Sender};

use crossterm::{
    cursor, event,
    event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent},
    queue, terminal, ExecutableCommand, QueueableCommand,
};

use tui::backend::CrosstermBackend;
use tui::layout;
use tui::layout::{Layout, Rect};
use tui::style;
use tui::style::{Color, Modifier, Style};
use tui::text;
use tui::widgets;
use tui::widgets::{
    BarChart, Block, Borders, Chart, Clear, Gauge, List, ListItem, Paragraph, Sparkline, Table,
    Tabs,
};
use tui::Terminal;

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

pub const HELP: &str = &"\
QUEUE GO HERE
Ctrl+c - Exit
a - Play/Pause
x - Stop
n - Next
v/V - Volume Increase/Decrease
";

struct FilterPane {
    tag: String,
    items: Vec<String>,
    selected: Vec<usize>,
}

pub fn tui(lib_send: Sender<Command>, lib_recv: Receiver<Response>, cli_recv: Receiver<()>) {
    l2!("Entering interactive terminal...");
    let log_level = LOG_LEVEL.swap(0, LOG_ORD); // TODO: better solution?

    let snd = |com: Command| lib_send.send(com).unwrap();
    let sndrec = |com: Command| {
        lib_send.send(com).unwrap();
        lib_recv.recv().unwrap()
    };

    terminal::enable_raw_mode().unwrap();
    let mut stdo = std::io::stdout();

    queue!(stdo, terminal::EnterAlternateScreen, cursor::Hide).unwrap();

    let backend = CrosstermBackend::new(stdo);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut filter_panes = Vec::<FilterPane>::new();
    filter_panes.push(FilterPane {
        tag: "album".to_string(),
        items: vec![
            "Illusions".to_string(),
            "Sun".to_string(),
            "Skyworld".to_string(),
        ],
        selected: vec![1, 2],
    });

    filter_panes.push(FilterPane {
        tag: "artist".to_string(),
        items: vec![
            "Thomas Bergersen".to_string(),
            "Two Steps From Hell".to_string(),
        ],
        selected: vec![0],
    });

    let result = std::panic::catch_unwind(move || 'main: loop {
        terminal
            .draw(|f| {
                let size = f.size();
                let zones = Layout::default()
                    .direction(layout::Direction::Vertical)
                    .constraints(vec![
                        layout::Constraint::Length(1),
                        layout::Constraint::Percentage(50),
                        layout::Constraint::Percentage(50),
                    ])
                    .split(size);
                let panes = Layout::default()
                    .direction(layout::Direction::Horizontal)
                    .constraints(
                        (0..filter_panes.len())
                            .map(|_| layout::Constraint::Ratio(1, filter_panes.len() as u32))
                            .collect::<Vec<layout::Constraint>>(),
                    )
                    .split(zones[1]);
                let queue = zones[2];
                f.render_widget(Paragraph::new(HELP), queue);
                let status = zones[0];
                f.render_widget(
                    Paragraph::new(format!("Vol: {}", sndrec(Command::VolumeGet))),
                    status,
                );
                for (num, fp) in filter_panes.iter().enumerate() {
                    f.render_widget(
                        List::new(
                            fp.items
                                .iter()
                                .enumerate()
                                .map(|(n, i)| {
                                    ListItem::new(tui::text::Span {
                                        content: i.into(),
                                        style: if fp.selected.contains(&n) {
                                            Style::default().fg(Color::Black).bg(Color::White)
                                        } else {
                                            Style::default()
                                        },
                                    })
                                })
                                .collect::<Vec<ListItem>>(),
                        )
                        .block(
                            Block::default()
                                .border_type(widgets::BorderType::Plain)
                                .borders(Borders::ALL)
                                .title(format!("{}", &fp.tag)),
                        ),
                        panes[num],
                    );
                }
            })
            .unwrap();

        // you *could* implement a proper event-driven system where you have separate threads for
        // key events, cli events, and updating the UI, but that'd mean redoing damn near
        // everything here to avoid deadlocks
        'poller: loop {
            if let Some(ev) = get_event(Some(Duration::from_millis(50))) {
                match ev {
                    km_c!('c') => {
                        snd(Command::Exit);
                        break 'main;
                    }

                    km!('a') => snd(Command::PlayPause),
                    km!('x') => snd(Command::Stop),
                    km!('n') => snd(Command::Next),
                    // KM!('p') => snd(Command::Previous),
                    km!('v') => snd(Command::VolumeAdd(0.05)),
                    km!('V') => snd(Command::VolumeSub(0.05)),
                    _ => (),
                }
                break 'poller;
            }
            if let Ok(_) = cli_recv.try_recv() {
                break 'poller;
            }
        }
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
