//! OMPL Opinionated Music Player/Library
//! Everything licensed under GNU General Public License V3
//! Alternatively, OMPL can be licensed under GOGAC which means if you can
//! officially 1v1 me and win you get it licensed under what's effectively MIT

#![warn(missing_docs)]

use clap::{ArgAction, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use regex::Regex;

#[cfg(feature = "media-controls")]
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};

mod library;
use library::{Backend, Color, LibEvt, Library, Theme};

#[cfg(feature = "tui")]
mod tui;

const ID: &str = "OMPL SERVER 0.9.0";
const PORT: &str = "18346";

// ### LOGGING ### {{{

/// Easy logging across modules
static LOG: (AtomicU8, AtomicBool, Mutex<Vec<(u8, String)>>) = (AtomicU8::new(0), AtomicBool::new(true), Mutex::new(Vec::new()));

/// If $v <= LOG_LEVEL print values
#[macro_export]
macro_rules! log {
    ($v:expr, $($fmt_args:tt)*) => {
        if LOG.1.load(std::sync::atomic::Ordering::Relaxed) {
            if LOG.0.load(std::sync::atomic::Ordering::Relaxed) >= $v {
                println!($($fmt_args)*)
            }
        // Store if paused
        } else {
            LOG.2.lock().unwrap().push(($v, format!($($fmt_args)*)))
        }
    };
}

/// Pause log and queue further entries
#[macro_export]
macro_rules! log_pause {
    () => {
        LOG.1.store(false, std::sync::atomic::Ordering::Relaxed)
    };
}

/// Resume log and print queued entries
#[macro_export]
macro_rules! log_resume {
    () => {
        LOG.1.store(true, std::sync::atomic::Ordering::Relaxed);
        let mut queue = LOG.2.lock().unwrap();
        for (n, s) in queue.drain(..) {
            log!(n, "{}", s)
        }
        queue.shrink_to_fit();
    };
}

/// Level 1
#[macro_export]
macro_rules! info {
    ($($fmt_args:tt)*) => {log!(1, $($fmt_args)*)}
}
/// Level 2
#[macro_export]
macro_rules! bench {
    ($($fmt_args:tt)*) => {log!(2, $($fmt_args)*)}
}
/// Level 3
#[macro_export]
macro_rules! debug {
    ($($fmt_args:tt)*) => {log!(3, $($fmt_args)*)}
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

// Weird macro lifetime bullshit. Could either spend a few hours researching or just do this
fn parse_color(s: &str) -> Result<Color, String> {
    Color::try_from(s)
}

fn parse_time(s: &str) -> Result<Duration, String> {
    let string = s.trim().to_ascii_lowercase();
    // hh:mm:ss.ddd format. hh:mm optional.
    if let Some(captures) = Regex::new(r"^(?:(\d+):)?(?:(\d+):)?(\d+(?:\.\d+)?)$").unwrap().captures(&string) {
        // secs
        let mut result = Duration::from_secs_f32(captures[3].parse::<f32>().unwrap());
        let mut hm = 1;
        // mins
        if let Some(m) = captures.get(2) {
            result += Duration::from_secs(m.as_str().parse::<u64>().unwrap() * 60);
            hm = 60
        }
        // hours
        if let Some(m) = captures.get(1) {
            result += Duration::from_secs(m.as_str().parse::<u64>().unwrap() * 60 * hm);
        }
        return Ok(result);
    }
    Err(format!("Could not parse {} as time signature", string))
}

// ### PARSERS ### }}}

// ### ARGS {{{
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
/// see Action
pub enum StatuslineCmd {
    /// Retreive statusline tagstring
    Get,
    /// Set statusline tagstring
    Set {
        ///
        tagstring: String,
    },
}

/// see Action
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum VolumeCmd {
    /// Current volume in range 0.0 -> 1.0
    Get,
    /// Add value onto current volume
    Add {
        ///
        amount: f32,
    },
    /// Subtract value from current volume
    Sub {
        ///
        amount: f32,
    },
    /// Set volume directly from range 0.0 -> 1.0
    Set {
        ///
        amount: f32,
    },
}

/// see Action
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum FilterCmd {
    /// Retrieves either INDEX Filter layer or all
    Get {
        ///
        index: Option<usize>,
    },
    /// Set INDEX filter layer to FILTER
    Set {
        ///
        index: usize,
        ///
        #[arg(value_parser=parse_filter)]
        filter: library::Filter,
    },
    /// Replace all Filter layers with provided list of filters
    SetAll {
        ///
        #[arg(num_args(1..), value_parser=parse_filter)]
        filters: Vec<library::Filter>,
    },
    /// Remove INDEX Filter layer
    Remove {
        ///
        index: usize,
    },
}

/// see Action
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum SorterCmd {
    /// Retrieves either INDEX Sorter layer or all
    Get {
        ///
        index: Option<usize>,
    },
    /// Set INDEX Sorter layer to TAGSTRING
    Set {
        ///
        index: usize,
        ///
        tagstring: String,
    },

    /// Replace all Sorter layers with provided list of tagstrings
    SetAll {
        ///
        #[arg(num_args(1..))]
        tagstrings: Vec<String>,
    },
    /// Remove INDEX Sorter layer
    Remove {
        ///
        index: usize,
    },
}

/// see Action
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum ShuffleCmd {
    /// true/false
    Get,
    /// Set true, next track picked randomly
    True,
    /// Set false, next track picked based on Sorter
    False,
    /// Toggle beteween true/false
    Toggle,
}

/// see Action
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum RepeatCmd {
    /// true, false, or track
    Get,
    /// Do not repeat.
    /// Only works if Shuffle is false
    False,
    /// Repeat only current track
    Track,
    /// Repeat forever
    True,
    /// Advance between False -> Track -> True
    Toggle,
}

/// see Action
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum SeekCmd {
    /// Get time in hh:mm:ss.dd / hh:mm::ss.dd format
    Get,
    /// Get time in float / float format
    GetSecs,
    /// Get time in floating point format normalized 0.0 -> 1.0
    GetFloat,
    /// true/false, whether or not seeking is possible
    Seekable,
    /// Seek to exact time in hh:mm:ss.dd format
    To {
        #[arg(value_parser=parse_time)]
        /// hh:mm:ss.dd
        time: Duration,
    },
    /// Advance time by seconds, positive or negative
    By {
        ///
        secs: f32,
    },
}

/// see Action
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum ThemeCmd {
    /// Foreground
    FG {
        #[arg(value_parser=parse_color)]
        ///
        foreground: Color,
    },
    /// Background
    BG {
        #[arg(value_parser=parse_color)]
        ///
        background: Color,
    },
    /// Accent
    ACC {
        #[arg(value_parser=parse_color)]
        ///
        accent: Color,
    },
}

/// see Action
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum PrintCmd {
    /// Dump all information about current track
    Track,
    /// Retrieve information from playing track using a tagstring
    Tagstring {
        ///
        tagstring: String,
    },
    /// Path to currently playing track
    File,
    /// 'playing'/'stopped'/'paused'
    Status,
    /// true/false
    Playing,
    /// true/false
    Stopped,
    /// true/false
    Paused,
    /// Print formatted statusline according to the set tagstring
    Statusline,
    /// Print current theme in either hex or terminal ID
    Theme,
}

/// see Args
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    /// Create new server on PORT
    Main {
        /// Paths to scan for music
        library: Vec<PathBuf>,

        #[arg(short = 'H', long)]
        /// Include hidden items ( '.' prefix )
        hidden: bool,

        #[arg(short = 'S', long)]
        /// Disable shuffle on startup
        noshuffle: bool,

        #[arg(short = 'r', long)]
        /// Loop single track
        repeat_track: bool,

        #[arg(short = 'R', long, conflicts_with = "repeat_track")]
        /// Disable looping
        norepeat: bool,

        #[arg(long, short)]
        /// Daemon / no-gui mode. Does nothing if `tui` is disabled at compile-time
        daemon: bool,

        #[arg(long, short)]
        /// Disable media interface.
        ///
        /// Useful if you want to only use the CLI as opposed to MPRIS.
        ///
        /// Does nothing if `media-controls` disabled at compile time
        no_media: bool,

        #[arg(long, short, num_args(1..), value_parser=parse_filter)]
        /// Starting filters
        filters: Vec<library::Filter>,

        #[arg(long = "sorters", short, num_args(1..))]
        /// Starting sorters
        sorters: Vec<String>,

        #[arg(long, short, default_value = "0.5")]
        /// Starting volume
        volume: f32,

        /// Tagstring to display on statusline
        #[arg(long, default_value = "title")]
        statusline: String,

        /// UI Foreground color
        #[arg(long, default_value = "none", value_parser=parse_color)]
        fg: Color,

        /// UI Background color
        #[arg(long, default_value = "none", value_parser=parse_color)]
        bg: Color,

        /// UI Accent color
        #[arg(long, default_value = "yellow", value_parser=parse_color)]
        acc: Color,

        /// Select audio streaming backend
        #[arg(long, default_value = "default")]
        backend: Backend,

        /// Verbosity level. Pass multiple times to get more verbose (spammy).
        #[arg(long, short = 'V', action(ArgAction::Count))]
        verbosity: u8,
    },
    /// Set audio player to 'playing'
    Play,
    /// Set audio player to 'paused'
    Pause,
    /// Set audio player to 'stopped'
    Stop,
    /// Toggle between Play/Pause
    PlayPause,
    /// Send next track in queue to the player
    Next,
    /// Send last previously played track to player, removing it from history
    Previous,
    /// Calls on the server to exit
    Exit,
    /// Work with volume in a range of 0.0 -> 1.0
    #[command(subcommand)]
    Volume(VolumeCmd),
    /// Control behavior after track ends
    #[command(subcommand)]
    Repeat(RepeatCmd),
    /// Control selection of next tracks
    #[command(subcommand)]
    Shuffle(ShuffleCmd),
    /// Scrub and seek current playback time. Sympal backend only
    #[command(subcommand)]
    Seek(SeekCmd),
    /// Update theme colors.
    ///
    /// Examples:
    ///
    /// "0.2, 0.5, 0.6"
    ///
    /// "lch:50;20;120"
    ///
    /// "oklab(0.2, 0.6, -0.5)"
    ///
    /// "srgb 100% 50% 25%"
    ///
    /// "#BB9944"
    #[command(subcommand)]
    Theme(ThemeCmd),
    /// Set/retrieve UI statusline
    #[command(subcommand)]
    Statusline(StatuslineCmd),
    /// Retrieve various server information not in other commands
    #[command(subcommand)]
    Print(PrintCmd),
    /// Send a file directly to the audio player
    PlayFile {
        /// A single audio track
        file: PathBuf,
    },
    /// Control how tracks are filtered for final play queue using layers of Filters.
    #[command(subcommand)]
    Filter(FilterCmd),
    /// Control how tracks are sorted internally using layers of tagstrings
    #[command(subcommand)]
    Sorter(SorterCmd),
    /// Append tracks to library from path
    Append {
        /// Path to scan for audio files
        path: PathBuf,
    },
    /// Remove all currently loaded tracks
    Purge,
}

#[derive(Parser, Debug, Clone, Serialize, Deserialize)]
#[command(author, about, version)]
struct Args {
    /// Action to perform. Required
    #[command(subcommand)]
    action: Action,

    /// Port with which to communicate with other OMPL instances
    #[arg(long, default_value = PORT)]
    port: u16,

    /// Address to listen on for client signals
    #[arg(long, default_value = "127.0.0.1")]
    host: Ipv4Addr,
}

// ### ARGS }}}

// ### SERVER ### {{{

fn server(listener: TcpListener, library: Arc<Library>) {
    for stream in listener.incoming() {
        debug!("Found client");
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
                debug!("Processing command...");
                match bincode::deserialize::<Args>(&data) {
                    Ok(args) => {
                        match args.action.clone() {
                            Action::Main { .. } => (),
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
                            Action::Seek(seek_cmd) => match seek_cmd {
                                SeekCmd::Get => {
                                    if let Some((current, total)) = library.times() {
                                        response = format!(
                                            "{:02}:{:02}:{:05.2} / {:02}:{:02}:{:05.2}",
                                            current.as_secs() / 360,
                                            current.as_secs() / 60 % 60,
                                            current.as_secs_f32() % 60.0,
                                            total.as_secs() / 360,
                                            total.as_secs() / 60 % 60,
                                            total.as_secs_f32() % 60.0,
                                        )
                                    }
                                }
                                SeekCmd::GetSecs => {
                                    if let Some((current, total)) = library.times() {
                                        response = format!("{:.2} / {:.2}", current.as_secs_f32(), total.as_secs_f32())
                                    }
                                }
                                SeekCmd::GetFloat => {
                                    if let Some((current, total)) = library.times() {
                                        response = format!("{:.8}", current.as_secs_f32() / total.as_secs_f32())
                                    }
                                }
                                SeekCmd::Seekable => response = (library.seekable() == Some(true)).to_string(),
                                SeekCmd::To { time } => library.seek(time),
                                SeekCmd::By { secs } => library.seek_by(secs),
                            },
                            Action::Volume(vol_cmd) => match vol_cmd {
                                VolumeCmd::Get => {
                                    response = format!("{:.2}", library.volume_get());
                                }
                                VolumeCmd::Add { amount } => library.volume_add(amount),
                                VolumeCmd::Sub { amount } => library.volume_add(-amount),
                                VolumeCmd::Set { amount } => library.volume_set(amount),
                            },
                            Action::Shuffle(shuffle_cmd) => match shuffle_cmd {
                                ShuffleCmd::Get => response = library.shuffle_get().to_string(),
                                ShuffleCmd::True => library.shuffle_set(true),
                                ShuffleCmd::False => library.shuffle_set(false),
                                ShuffleCmd::Toggle => library.shuffle_toggle(),
                            },
                            Action::Repeat(repeat_cmd) => match repeat_cmd {
                                RepeatCmd::Get => {
                                    response = match library.repeat_get() {
                                        Some(true) => true.to_string(),
                                        Some(false) => "track".to_string(),
                                        None => false.to_string(),
                                    }
                                }
                                RepeatCmd::True => library.repeat_set(Some(true)),
                                RepeatCmd::Track => library.repeat_set(Some(false)),
                                RepeatCmd::False => library.repeat_set(None),
                                RepeatCmd::Toggle => library.repeat_toggle(),
                            },
                            Action::Statusline(statusline_cmd) => match statusline_cmd {
                                StatuslineCmd::Set { tagstring } => library.statusline_set(tagstring),
                                StatuslineCmd::Get => response = library.statusline_get(),
                            },
                            Action::Theme(theme_cmd) => {
                                let mut theme = library.theme_get();
                                match theme_cmd {
                                    ThemeCmd::FG { foreground } => theme.fg = foreground,
                                    ThemeCmd::BG { background } => theme.bg = background,
                                    ThemeCmd::ACC { accent } => theme.acc = accent,
                                };
                                library.theme_set(theme)
                            }
                            Action::PlayFile { file } => {
                                if file.is_file() {
                                    library.play_track(library::find_tracks(file, &library.types(), true).into_iter().last().map(|mut t| {
                                        t.load_meta();
                                        Arc::new(t)
                                    }))
                                }
                            }

                            Action::Filter(cmd) => match cmd {
                                FilterCmd::Get { index } => {
                                    response = if let Some(i) = index {
                                        library
                                            .get_filter(i)
                                            .map(|f| {
                                                if f.items.is_empty() {
                                                    f.tag
                                                } else {
                                                    format!("{}={}", f.tag, f.items.join(","))
                                                }
                                            })
                                            .unwrap_or(String::new())
                                    } else {
                                        library
                                            .get_filters()
                                            .into_iter()
                                            .map(|f| {
                                                if f.items.is_empty() {
                                                    f.tag
                                                } else {
                                                    format!("{}={}", f.tag, f.items.join(","))
                                                }
                                            })
                                            .collect::<Vec<String>>()
                                            .join("\n")
                                    }
                                }
                                FilterCmd::Set { index, filter } => library.set_filter(index, filter),
                                FilterCmd::SetAll { filters } => library.set_filters(filters),
                                FilterCmd::Remove { index } => library.remove_filter(index),
                            },

                            Action::Sorter(cmd) => match cmd {
                                SorterCmd::Get { index } => {
                                    response = if let Some(i) = index {
                                        library.get_sorter(i).unwrap_or(String::new())
                                    } else {
                                        library.get_sorters().join("\n")
                                    }
                                }
                                SorterCmd::Set { index, tagstring } => library.set_sorter(index, tagstring),
                                SorterCmd::SetAll { tagstrings } => library.set_sorters(tagstrings),
                                SorterCmd::Remove { index } => library.remove_sorter(index),
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
                                PrintCmd::Track => response = library.track_get().map(|t| format!("{}", t)).unwrap_or("???".to_string()),
                                PrintCmd::File => {
                                    response = library
                                        .track_get()
                                        .map(|t| t.path().to_str().unwrap_or("???").to_string())
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
                                PrintCmd::Statusline => response = library.statusline_get_format(),
                                PrintCmd::Theme => response = library.theme_get().to_string(),
                            },
                            Action::Append { path } => library.append_library(path),
                            Action::Purge => library.purge(),
                        };
                    }
                    Err(e) => response = format!("Could not deserialize args\n{}\nOMPL version mismatch?", e),
                };
                // # Process # }}}

                // finalize response
                if s.write_all(response.as_bytes()).is_err() {
                    continue;
                };
            }
            Err(e) => panic!("Listener panic: {}", e),
        }
        debug!("End client connection");
    }
    debug!("Server exiting");
}

fn instance_main(listener: TcpListener, args: Args) {
    // is there a way to extract the structured data?
    // so I can just yoink the main struct and do
    // main.hidden main.volume etc?
    match args.action {
        Action::Main {
            library: library_paths,
            hidden,
            noshuffle,
            norepeat,
            repeat_track,
            daemon,
            no_media,
            filters,
            sorters,
            volume,
            verbosity,
            statusline,
            fg,
            bg,
            acc,
            backend,
        } => {
            LOG.0.store(verbosity, std::sync::atomic::Ordering::Relaxed);

            debug!("Starting main...");
            let library = Library::new(backend);
            library.hidden_set(hidden);
            library.volume_set(volume);
            library.shuffle_set(!noshuffle);
            library.repeat_set(if norepeat {
                None
            } else if repeat_track {
                Some(false)
            } else {
                Some(true)
            });
            library.statusline_set(statusline);
            library.theme_set(Theme { fg, bg, acc });
            library.set_filters(filters);
            library.set_sorters(sorters);
            for path in library_paths {
                library.append_library(path)
            }

            let server_library = library.clone();
            let jh = thread::spawn(move || server(listener, server_library));
            debug!("Listening on port {}", args.port);

            // ## souvlaki ## {{{
            #[cfg(feature = "media-controls")]
            if !no_media {
                debug!("Initializing media controls...");
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
                        .build(&winit::event_loop::EventLoop::new().unwrap())
                        .unwrap()
                        .raw_window_handle()
                    {
                        RawWindowHandle::Win32(han) => Some(han.hwnd),
                        _ => panic!("Unknown window handle type!"),
                    }
                };

                match MediaControls::new(PlatformConfig {
                    // <https://dbus.freedesktop.org/doc/dbus-specification.html#message-protocol-names-bus>
                    dbus_name: &format!("ompl.port{}", args.port),
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
                                        MediaControlEvent::SetVolume(n) => library.volume_set(n as f32),
                                        MediaControlEvent::SetPosition(p) => library.seek(p.0),
                                        MediaControlEvent::Seek(d) => match d {
                                            souvlaki::SeekDirection::Forward => library.seek_by(5.0),
                                            souvlaki::SeekDirection::Backward => library.seek_by(-5.0),
                                        },
                                        MediaControlEvent::SeekBy(d, n) => match d {
                                            souvlaki::SeekDirection::Forward => library.seek_by(n.as_secs_f32()),
                                            souvlaki::SeekDirection::Backward => library.seek_by(-n.as_secs_f32()),
                                        },
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
                                        let (pos, tot) = if let Some((cur, tot)) = library.times() {
                                            (Some(souvlaki::MediaPosition(cur)), Some(tot))
                                        } else {
                                            (None, None)
                                        };
                                        controls
                                            .set_metadata(MediaMetadata {
                                                title: library.track_get().map(|t| t.tags().get("title").cloned()).flatten().as_deref(),
                                                artist: library.track_get().map(|t| t.tags().get("artist").cloned()).flatten().as_deref(),
                                                album: library.track_get().map(|t| t.tags().get("album").cloned()).flatten().as_deref(),
                                                duration: tot,
                                                cover_url: None,
                                            })
                                            .unwrap();
                                        controls
                                            .set_playback(if library.playing() {
                                                MediaPlayback::Playing { progress: pos }
                                            } else if library.paused() {
                                                MediaPlayback::Paused { progress: pos }
                                            } else {
                                                MediaPlayback::Stopped
                                            })
                                            .unwrap();
                                        #[cfg(target_os = "linux")]
                                        controls.set_volume(library.volume_get() as f64).unwrap();
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

            debug!("Main server started");
            if daemon || cfg!(not(feature = "tui")) {
                let mut recv = library.get_receiver();
                drop(library);
                loop {
                    match recv.recv() {
                        Ok(LibEvt::Error(e)) => eprintln!("{}", e),
                        Ok(_) => (),
                        Err(_e) => break,
                    }
                }
                jh.join().unwrap();
            } else {
                #[cfg(feature = "tui")]
                if tui::tui(library) {
                    jh.join().unwrap();
                }
            }
        }
        _ => unreachable!("Instance_Main called without Main Subcommand!\n{:?}", args),
    }
}

// ### SERVER ### }}}

// ### CLIENT ### {{{

fn instance_sub(mut stream: TcpStream, args: Args) {
    // confirmation ID
    let mut confirmation = vec![0u8; ID.bytes().count()];
    stream.read_exact(&mut confirmation).unwrap();
    assert!(String::from_utf8(confirmation).unwrap() == ID);

    let data = match bincode::serialize(&args) {
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
    let args = Args::parse();

    match args.action {
        Action::Main { .. } => match TcpListener::bind(SocketAddrV4::new(args.host, args.port)) {
            Ok(listener) => instance_main(listener, args),
            Err(_) => panic!(
                "\n\nCouldn't bind server socket to port {}.\n\
                    Try another port, or perhaps an instance is already running?\n\n",
                args.port
            ),
        },
        _ => match TcpStream::connect(SocketAddrV4::new(args.host, args.port)) {
            Ok(stream) => instance_sub(stream, args),
            Err(_) => panic!(
                "\n\nCouldn't connect client socket to port {}.\n\
                        Are you sure there's an OMPL server running here?\n\n",
                args.port
            ),
        },
    }
}
// ### MAIN ### }}}
