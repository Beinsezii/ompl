use std::io::Write;
use std::time::Duration;

use crate::library::{Command, Response};
use crate::{l2, log, LOG_LEVEL, LOG_ORD};

use crossbeam::channel;
use crossbeam::channel::{Receiver, Sender};

use crossterm::{
    cursor, event,
    event::{poll, read, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent},
    execute, queue, style, terminal, ExecutableCommand, QueueableCommand,
};

type Row = Vec<char>;
type Buff = Vec<Row>;

/// build main UI
fn build(columns: u16, rows: u16) -> Buff {
    let row = vec!['R'; columns.into()].into_iter().collect::<Row>();
    let row_alt = vec!['r'; columns.into()].into_iter().collect::<Row>();
    let mut buff = Buff::new();
    for x in 0..rows {
        if x % 2 == 0 {
            buff.push(row.clone());
        } else {
            buff.push(row_alt.clone())
        }
    }
    buff
}

/// draw buff to source
fn draw<T: Write>(source: &mut T, buff: &Buff, cols: u16, rows: u16) {
    queue!(
        source,
        terminal::Clear(terminal::ClearType::All),
        cursor::SavePosition
    )
    .unwrap();

    for (rnum, row) in buff.iter().enumerate() {
        if rnum >= rows.into() {
            break;
        };
        let row: String = row.into_iter().collect();
        let row_view = &row[0..std::cmp::min(cols.into(), row.len())];
        queue!(
            source,
            cursor::MoveTo(0, rnum as u16),
            style::Print(row_view)
        )
        .unwrap();
    }

    source
        .queue(cursor::RestorePosition)
        .unwrap()
        .flush()
        .unwrap();
}

/// draw &str to source by converting it to a Buff
fn draw_str<T: Write>(source: &mut T, text: &str, cols: u16, rows: u16) {
    draw(
        source,
        &text
            .split('\n')
            .collect::<Vec<&str>>()
            .iter()
            .map(|row| row.chars().collect::<Row>())
            .collect::<Buff>(),
        cols,
        rows,
    )
}

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
            if poll(delay).unwrap() {
                Some(read().unwrap())
            } else {
                None
            }
        }
        None => Some(read().unwrap()),
    }
}

pub const HELP: &str = &"\
Ctrl+c - Exit
a - Play/Pause
x - Stop
n - Next
V/v - Volume Increase/Decrease
";

pub fn tui(lib_send: Sender<Command>, lib_recv: Receiver<Response>, cli_recv: Receiver<()>) {
    let snd = |com: Command| lib_send.send(com).unwrap();
    let sndrec = |com: Command| {
        lib_send.send(com).unwrap();
        lib_recv.recv().unwrap()
    };
    l2!("Entering interactive terminal...");
    let log_level = LOG_LEVEL.swap(0, LOG_ORD); // TODO: better solution?
    terminal::enable_raw_mode().unwrap();
    let (mut cols, mut rows) = terminal::size().unwrap();
    let mut stdo = std::io::stdout();

    queue!(stdo, terminal::EnterAlternateScreen, cursor::Hide).unwrap();

    let result = std::panic::catch_unwind(move || 'main: loop {
        draw_str(
            &mut stdo,
            &format!("{}\nVol: {}", HELP, sndrec(Command::VolumeGet)),
            cols,
            rows,
        );

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
                    km!('V') => snd(Command::VolumeAdd(0.05)),
                    km!('v') => snd(Command::VolumeSub(0.05)),

                    Event::Resize(c, r) => {
                        cols = c;
                        rows = r;
                    }
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
