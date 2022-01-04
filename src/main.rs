use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use std::thread;

mod library;
mod tui;
use library::Library;

const ID: &str = "OMPL SERVER 0.2.0";

// ### LOGGING ### {{{

/// Easy logging across modules
static LOG_LEVEL: AtomicU8 = AtomicU8::new(0);
const LOG_ORD: Ordering = Ordering::Relaxed;

#[macro_export]
macro_rules! log {
    ($v:expr, $($info:expr),*) => {
        // ???
        if LOG_LEVEL.load(std::sync::atomic::Ordering::Relaxed) >= $v {
            $(
                print!("{} ", $info);
                )*
            println!();
        }
    };
}

// Must build individually: https://github.com/rust-lang/rust/issues/35853
#[macro_export]
/// Level 1 is more intended for performance metrics
macro_rules! l1 {
    ($($info:expr),*) => {log!(1, $($info)*)}
}
#[macro_export]
/// Level 2 is the 'rubber duck' level that walks you through the program
macro_rules! l2 {
    ($($info:expr),*) => {log!(2, $($info)*)}
}
#[macro_export]
/// Level 3 is for dumping various bits of information
macro_rules! l3 {
    ($($info:expr),*) => {log!(3, $($info)*)}
}
#[macro_export]
/// Level 4 is for spamming the shit out of your terminal in a last-resort attempt at debugging
macro_rules! l4 {
    ($($info:expr),*) => {log!(4, $($info)*)}
}

// ### LOGGING ### }}}

// ### PARSERS ### {{{

#[rustfmt::skip] // it adds a whole lot of lines
fn parse_filter(s: &str) -> Result<library::Filter, String> {
    let mut i = s.chars();

    let mut tag = String::new();
    let mut items = Vec::new();

    let mut switch = false;
    let mut item_buff = String::new();

    let mut pos = 1;

    loop {
        match i.next() {
            Some('\\') => if let Some(c) = i.next() {
                    if switch { item_buff.push(c) }
                    else { tag.push(c) };
                    pos += 1
            },
            Some('=') => match switch {
                false => switch = true,
                true => return Err(format!(
                        "Inappropriate equals @ position {} of \"{}\"",
                        pos, s
                    ))
            },
            Some(',') => match item_buff.is_empty() && switch {
                false => {
                    items.push(item_buff);
                    item_buff = String::new();
                }
                true => return Err(format!(
                        "Inappropriate comma @ position {} of \"{}\"",
                        pos, s
                    ))
            },
            Some(c) =>
                if switch { item_buff.push(c) }
                else { tag.push(c) },
            None => break,
        }
        pos += 1;
    }

    if !item_buff.is_empty() { items.push(item_buff) }

    Ok(library::Filter { tag, items })
}

// ### PARSERS ### }}}

// ### SHARED {{{

#[derive(Debug)]
enum Instance {
    Main(TcpListener),
    Sub(TcpStream),
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum VolumeCmd {
    Get,
    Add { amount: f32 },
    Sub { amount: f32 },
    Set { amount: f32 },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum PrintCmd {
    Status,
    Track,
    Volume,
    Playing,
    Stopped,
    Paused,
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Play,
    Pause,
    Stop,
    PlayPause,
    Next,
    Previous,
    Exit,
    #[clap(subcommand)]
    Volume(VolumeCmd),
    #[clap(subcommand)]
    Print(PrintCmd),
    Filter {
        #[clap(long, short, multiple_occurrences(true), multiple_values(false), parse(try_from_str = parse_filter))]
        filters: Vec<library::Filter>,
        /// Play next track immediately
        #[clap(long)]
        now: bool,
    },
}

// ### SHARED }}}

// ### SERVER ### {{{

#[derive(Parser, Debug, Clone)]
#[clap(author, about, version, after_help(tui::HELP))]
struct MainArgs {
    #[clap(short, long)]
    /// Path to music libary folder
    library: PathBuf,

    #[clap(long)]
    /// Play immediately
    now: bool,

    #[clap(long, short)]
    /// [D]aemon / no-gui mode.
    daemon: bool,

    #[clap(long, short)]
    /// Disable media interface. Useful if you want to only use the CLI as opposed to MPRIS
    no_media: bool,

    /// Port with which to communicate with other OMPL instances
    #[clap(long, default_value = "18346")]
    port: u16,

    #[clap(long, short, multiple_occurrences(true), multiple_values(false), parse(try_from_str = parse_filter))]
    filters: Vec<library::Filter>,

    #[clap(long, short, default_value = "0.5")]
    /// Starting volume
    volume: f32,

    /// Verbosity level. Pass multiple times to get more verbose (spammy).
    #[clap(long, short = 'V', multiple_occurrences(true), parse(from_occurrences))]
    verbosity: u8,
}

fn server(listener: TcpListener, library: Arc<Library>) {
    for stream in listener.incoming() {
        l2!("Found client");
        match stream {
            Ok(mut s) => {
                // # Get Data # {{{
                // confirmation ID
                if s.write_all(ID.as_bytes()).is_err() {
                    continue;
                };

                let mut response = String::new();

                // exchange size
                let mut data = [0u8; std::mem::size_of::<usize>()];
                if s.read_exact(&mut data).is_err() {
                    continue;
                };
                let size: usize = usize::from_be_bytes(data);

                // exchange args
                let mut data = vec![0u8; size];
                if s.read_exact(&mut data).is_err() {
                    continue;
                };
                // # Get Data # }}}

                // # Process # {{{
                l2!("Processing command...");
                match bincode::deserialize::<SubArgs>(&data) {
                    Ok(sub_args) => {
                        match sub_args.action.clone() {
                            Action::Exit => {
                                // finalize response 2
                                if let Err(e) = s.write_all(response.as_bytes()) {
                                    println!("{}", e)
                                };
                                break;
                            }
                            Action::Next => library.next(),
                            Action::Previous => library.previous(),
                            Action::Pause => library.pause(),
                            Action::Play => library.play(),
                            Action::PlayPause => library.play_pause(),
                            Action::Stop => library.stop(),
                            Action::Volume(vol_cmd) => match vol_cmd {
                                VolumeCmd::Get => {
                                    response = format!("{:.2}", library.volume_get());
                                }
                                VolumeCmd::Add { amount } => library.volume_add(amount),
                                VolumeCmd::Sub { amount } => library.volume_sub(amount),
                                VolumeCmd::Set { amount } => library.volume_set(amount),
                            },
                            Action::Print(print_cmd) => match print_cmd {
                                PrintCmd::Status => {
                                    response = if library.playing() {
                                        "playing".to_string()
                                    } else if library.paused() {
                                        "paused".to_string()
                                    } else if library.stopped() {
                                        "stopped".to_string()
                                    } else {
                                        "invalid".to_string()
                                    }
                                }
                                PrintCmd::Track => {
                                    response = library
                                        .track_get()
                                        .map(|t| format!("{}", t))
                                        .unwrap_or("???".to_string())
                                }
                                PrintCmd::Playing => response = library.playing().to_string(),
                                PrintCmd::Paused => response = library.paused().to_string(),
                                PrintCmd::Stopped => response = library.stopped().to_string(),
                                PrintCmd::Volume => {
                                    response = format!("{:.2}", library.volume_get())
                                }
                            },
                            Action::Filter { now, filters } => {
                                library.set_filters(filters);
                                if now {
                                    library.next()
                                }
                            }
                        };
                    }
                    Err(e) => {
                        response =
                            format!("Could not deserialize args\n{}\nOMPL version mismatch?", e)
                    }
                };
                // # Process # }}}

                // finalize response
                if s.write_all(response.as_bytes()).is_err() {
                    continue;
                };
            }
            Err(e) => panic!("Listener panic: {}", e),
        }
        l2!("End client connection");
    }
    l2!("Server exiting");
}

fn instance_main(listener: TcpListener, port: u16) {
    let main_args = MainArgs::parse();
    LOG_LEVEL.store(main_args.verbosity, LOG_ORD);

    l2!("Starting main...");
    let library = Library::new(&main_args.library, Some(main_args.filters));
    library.volume_set(main_args.volume);
    if main_args.now {
        library.play()
    }

    let server_library = library.clone();
    let jh = thread::spawn(move || server(listener, server_library));
    l2!(format!("Listening on port {}", port));

    // ## souvlaki ## {{{
    if !main_args.no_media {
        use souvlaki::{
            MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig,
        };
        l2!("Initializing media controls...");
        let mut libevt_r = library.get_receiver();

        #[cfg(not(target_os = "windows"))]
        let hwnd = None;

        #[cfg(target_os = "windows")]
        let hwnd = {
            // You *could* use winapi::um::wincon::GetConsoleWindow()
            // but if you're running ompl from the CLI, conhost.exe will own the window process
            // so souvlaki can't hook into it. This just creates a hidden window instead.
            use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
            match winit::window::WindowBuilder::new()
                .with_decorations(false)
                .with_visible(false)
                .with_title("OMPL Media Control Window")
                .build(&winit::event_loop::EventLoop::new())
                .unwrap()
                .raw_window_handle()
            {
                RawWindowHandle::Win32(han) => Some(han.hwnd),
                _ => panic!("Unknown window handle type!"),
            }
        };

        match MediaControls::new(PlatformConfig {
            dbus_name: &format!("ompl.port{}", port),
            display_name: "OMPL",
            hwnd,
        }) {
            Ok(mut controls) => {
                let ctrl_libr_wk = Arc::downgrade(&library);
                controls
                    .attach(move |event: MediaControlEvent| {
                        if let Some(library) = ctrl_libr_wk.upgrade() {
                            match event {
                                MediaControlEvent::Play => library.play(),
                                MediaControlEvent::Stop => library.stop(),
                                MediaControlEvent::Pause => library.pause(),
                                MediaControlEvent::Toggle => library.play_pause(),
                                MediaControlEvent::Next => library.next(),
                                MediaControlEvent::Previous => library.previous(),
                                _ => (),
                            }
                        }
                    })
                    .unwrap();
                let meta_libr_wk = Arc::downgrade(&library);
                thread::spawn(move || loop {
                    match libevt_r.recv() {
                        Ok(_) => {
                            if let Some(library) = meta_libr_wk.upgrade() {
                                controls
                                    .set_metadata(MediaMetadata {
                                        title: library
                                            .track_get()
                                            .map(|t| t.tags().get("title").cloned())
                                            .flatten()
                                            .as_deref(),
                                        artist: library
                                            .track_get()
                                            .map(|t| t.tags().get("artist").cloned())
                                            .flatten()
                                            .as_deref(),
                                        album: library
                                            .track_get()
                                            .map(|t| t.tags().get("album").cloned())
                                            .flatten()
                                            .as_deref(),
                                        ..Default::default()
                                    })
                                    .unwrap();
                                controls
                                    .set_playback(if library.playing() {
                                        MediaPlayback::Playing { progress: None }
                                    } else if library.paused() {
                                        MediaPlayback::Paused { progress: None }
                                    } else {
                                        MediaPlayback::Stopped
                                    })
                                    .unwrap();
                            } else {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                });
            }
            Err(e) => println!("Media control failure: {:?}", e),
        }
    }
    // ## souvlaki ## }}}

    l2!("Main server started");
    if main_args.daemon {
        jh.join().unwrap();
    } else {
        tui::tui(library);
    }
}

// ### SERVER ### }}}

// ### CLIENT ### {{{

#[derive(Parser, Debug, Clone, Serialize, Deserialize)]
#[clap(author, about, version, after_help(tui::HELP))]
struct SubArgs {
    #[clap(subcommand)]
    action: Action,

    /// Port with which to communicate with other OMPL instances
    #[clap(long, default_value = "18346")]
    port: u16,
}

fn instance_sub(mut stream: TcpStream) {
    let sub_args = SubArgs::parse();
    // confirmation ID
    let mut confirmation = vec![0u8; ID.bytes().count()];
    stream.read_exact(&mut confirmation).unwrap();
    assert!(String::from_utf8(confirmation).unwrap() == ID);

    let data = match bincode::serialize(&sub_args) {
        Ok(d) => d,
        Err(e) => panic!("Could not serialize args\n{}", e),
    };

    // exchange size
    stream.write_all(&data.len().to_be_bytes()).unwrap();

    // exchange args
    stream.write_all(&data).unwrap();

    // finalize response
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    if !response.is_empty() {
        println!("{}", response);
    }
}

// ### CLIENT ### }}}

// ### MAIN ### {{{
fn main() {
    let mut port = 18346;
    let mut p = false;
    for arg in std::env::args() {
        if p {
            match arg.parse::<u16>() {
                Ok(a) => port = a,
                Err(e) => panic!("Invalid port: {:?}", e),
            }
            break;
        } else {
            if arg == "--port" {
                p = true
            }
        }
    }
    let socket = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port);

    let instance = match TcpListener::bind(socket) {
        Ok(v) => Instance::Main(v),
        Err(_) => match TcpStream::connect(socket) {
            Ok(v) => Instance::Sub(v),
            Err(_) => panic!("Couldn't bind socket or connect to existing. Try another port?"),
        },
    };

    match instance {
        Instance::Main(m) => instance_main(m, port),
        Instance::Sub(s) => instance_sub(s),
    }
}
// ### MAIN ### }}}
