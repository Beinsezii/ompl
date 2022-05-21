use clap::{ErrorKind, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use std::thread;

#[cfg(feature = "media-controls")]
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};

mod library;
use library::Library;

#[cfg(feature = "tui")]
mod tui;

const ID: &str = "OMPL SERVER 0.4.0";
const PORT: &str = "18346";

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

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum VolumeCmd {
    Get,
    Add { amount: f32 },
    Sub { amount: f32 },
    Set { amount: f32 },
}

#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum PrintCmd {
    Volume,
    Track,
    Tagstring { tagstring: String },
    Status,
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
    Sort {
        #[clap(multiple_occurrences(false), multiple_values(true), required(true))]
        sort_tagstrings: Vec<String>,
    },
    Append {
        path: PathBuf,
    },
}

// ### SHARED }}}

// ### SERVER ### {{{

#[derive(Parser, Debug, Clone)]
#[clap(
    author,
    about,
    version,
    before_help("OMPL Server Help"),
    long_about(
        "Server-exclusive actions are all regular args\n\
        Server requires the --library or -l flags set\n\
        Use `ompl --help` to see both client and server helps together"
    )
)]
struct MainArgs {
    #[clap(short, long)]
    /// Path to music libary folder
    library: PathBuf,

    #[clap(long)]
    /// Play immediately
    now: bool,

    #[clap(long, short)]
    /// [D]aemon / no-gui mode. Does nothing if `tui` is disabled at compile-time
    daemon: bool,

    #[clap(long, short)]
    /// Disable media interface. Useful if you want to only use the CLI as opposed to MPRIS. Does nothing if `media-controls` disabled at compile time
    no_media: bool,

    /// Port with which to communicate with other OMPL instances
    #[clap(long, default_value = PORT)]
    port: u16,

    #[clap(long, short, multiple_occurrences(true), multiple_values(false), parse(try_from_str = parse_filter))]
    filters: Vec<library::Filter>,

    #[clap(
        long = "sort",
        short,
        multiple_occurrences(false),
        multiple_values(true)
    )]
    sort_tagstrings: Vec<String>,

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
                                PrintCmd::Tagstring { tagstring } => {
                                    response = if let Some(track) = library.track_get() {
                                        library::tagstring::parse(tagstring, track.tags())
                                    } else {
                                        String::new()
                                    }
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
                            Action::Sort { sort_tagstrings } => {
                                library.set_sort_tagstrings(sort_tagstrings)
                            }
                            Action::Append { path } => library.append_library(path),
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

fn instance_main(listener: TcpListener, main_args: MainArgs) {
    LOG_LEVEL.store(main_args.verbosity, LOG_ORD);

    l2!("Starting main...");
    let library = Library::new(&main_args.library, Some(main_args.filters));
    library.volume_set(main_args.volume);
    library.set_sort_tagstrings(main_args.sort_tagstrings);
    if main_args.now {
        library.play()
    }

    let server_library = library.clone();
    let jh = thread::spawn(move || server(listener, server_library));
    l2!(format!("Listening on port {}", main_args.port));

    // ## souvlaki ## {{{
    #[cfg(feature = "media-controls")]
    if !main_args.no_media {
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
            dbus_name: &format!("ompl.port{}", main_args.port),
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
    #[cfg(feature = "tui")]
    if main_args.daemon {
        jh.join().unwrap();
    } else {
        if tui::tui(library) {
            jh.join().unwrap();
        }
    }

    #[cfg(not(feature = "tui"))]
    jh.join().unwrap();
}

// ### SERVER ### }}}

// ### CLIENT ### {{{

#[derive(Parser, Debug, Clone, Serialize, Deserialize)]
#[clap(
    author,
    about,
    version,
    long_about(
        "Client-exclusive actions are all subcommands\n\
        Client requires at least one subcommand passed\n\
        Use `ompl help COMMAND` for more detailed subcommand information\n\
        Use `ompl --help` to see both client and server helps together"
    ),
    before_help("OMPL Client Help")
)]
struct SubArgs {
    #[clap(subcommand)]
    action: Action,

    /// Port with which to communicate with other OMPL instances
    #[clap(long, default_value = PORT)]
    port: u16,
}

fn instance_sub(mut stream: TcpStream, sub_args: SubArgs) {
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
    let main_args = MainArgs::try_parse();
    let sub_args = SubArgs::try_parse();

    match main_args {
        Ok(margs) => {
            match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), margs.port)) {
                Ok(listener) => instance_main(listener, margs),
                Err(_) => panic!(
                    "\n\nCouldn't bind server socket to port {}.\n\
                    Try another port, or perhaps an instance is already running?\n\n",
                    margs.port
                ),
            }
        }
        Err(m_e) => match sub_args {
            Ok(sargs) => {
                match TcpStream::connect(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), sargs.port))
                {
                    Ok(stream) => instance_sub(stream, sargs),
                    Err(_) => panic!(
                        "\n\nCouldn't connect client socket to port {}.\n\
                        Are you sure there's an OMPL server running here?\n\n",
                        sargs.port
                    ),
                }
            }
            Err(s_e) => {
                if s_e.kind() == ErrorKind::DisplayHelp && m_e.kind() != ErrorKind::DisplayHelp {
                    // if using help subcommand don't also print main error
                    s_e.print().unwrap();
                } else if s_e.kind() == ErrorKind::DisplayHelp
                    && m_e.kind() == ErrorKind::DisplayHelp
                {
                    // --help or -h
                    m_e.print().unwrap();
                    println!();
                    s_e.print().unwrap();
                } else {
                    // dual errors
                    println!("OMPL Main Error\n");
                    m_e.print().unwrap();
                    println!("\nOMPL Client Error\n");
                    s_e.print().unwrap();
                }
            }
        },
    }
}
// ### MAIN ### }}}
