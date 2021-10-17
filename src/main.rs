use clap::Clap;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};

mod library;
use library::Library;

const ID: &str = "OMPL SERVER 0.1.0";
const PORT: u16 = 18346;

#[derive(Debug)]
enum Instance {
    Main(TcpListener),
    Sub(TcpStream),
}

#[derive(Clap, Debug, Clone, Serialize, Deserialize)]
enum VolumeCmd {
    Get,
    Add { amount: f32 },
    Sub { amount: f32 },
    Set { amount: f32 },
}

#[derive(Clap, Debug, Clone, Serialize, Deserialize)]
enum PrintCmd {
    Status,
    Playing { format_string: String },
}

#[derive(Clap, Debug, Clone, Serialize, Deserialize)]
enum Action {
    Play,
    Pause,
    Stop,
    PlayPause,
    Next,
    Exit,
    #[clap(subcommand)]
    Volume(VolumeCmd),
    #[clap(subcommand)]
    Print(PrintCmd),
}

#[derive(Debug, Clone, Clap, Serialize, Deserialize)]
#[clap(name = "ompl", version = "0.1.0", author = "Beinsezii")]
struct SubArgs {
    #[clap(subcommand)]
    action: Action,
}

#[derive(Debug, Clone, Clap)]
#[clap(name = "ompl", version = "0.1.0", author = "Beinsezii")]
struct MainArgs {
    /// Path to music libary folder
    library: std::path::PathBuf,
    #[clap(long)]
    /// Play immediately
    now: bool,
}

fn instance_main(listener: TcpListener) {
    let main_args = MainArgs::parse();

    let now = std::time::Instant::now();
    let library = Library::new(&main_args.library);
    println!("Library loading in {:?}", std::time::Instant::now() - now);

    if main_args.now {
        library.play()
    }

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                // confirmation ID
                if s.write_all(ID.as_bytes()).is_err() {
                    continue;
                };

                let mut response = String::new();

                // exchange size
                let mut data = [0u8; std::mem::size_of::<usize>()];
                if s.read_exact(&mut data).is_err() {
                    continue
                };
                let size: usize = usize::from_be_bytes(data);

                // exchange args
                let mut data = vec![0u8; size];
                if s.read_exact(&mut data).is_err() {
                    continue
                };
                match bincode::deserialize::<SubArgs>(&data) {
                    Ok(sub_args) => {
                        match sub_args.action {
                            Action::Exit => {
                                // finalize response 2
                                if let Err(e) = s.write_all(response.as_bytes()) {
                                    println!("{}", e)
                                };
                                break;
                            }
                            Action::Next => library.next(),
                            Action::Pause => library.pause(),
                            Action::Play => library.play(),
                            Action::PlayPause => library.play_pause(),
                            Action::Stop => library.stop(),
                            Action::Volume(vol_cmd) => match vol_cmd {
                                VolumeCmd::Get => {
                                    response = format!("{:.3}", library.volume_get());
                                }
                                VolumeCmd::Add { amount } => library.volume_add(amount),
                                VolumeCmd::Sub { amount } => library.volume_sub(amount),
                                VolumeCmd::Set { amount } => library.volume_set(amount),
                            },
                            Action::Print(print_cmd) => match print_cmd {
                                PrintCmd::Status => response = format!("{}", library.get_status().read().unwrap()),
                                PrintCmd::Playing { format_string } => {
                                    response = format!("Unimplemented!\n{}", format_string)
                                }
                            },
                        };
                    }
                    Err(e) => response = format!("Could not deserialize args\n{}\nOMPL version mismatch?", e),
                };
                // finalize response
                if s.write_all(response.as_bytes()).is_err() {
                    continue
                };
            }
            Err(e) => panic!("Listener panic: {}", e),
        }
    }
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

fn main() {
    // I want the port to be change-able but don't know a good way to without buggering the
    // sub & main args
    let socket = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), PORT);

    let instance = match TcpListener::bind(socket) {
        Ok(v) => Instance::Main(v),
        Err(_) => match TcpStream::connect(socket) {
            Ok(v) => Instance::Sub(v),
            Err(_) => panic!("Couldn't bind socket or connect to existing. Try another port?"),
        },
    };

    match instance {
        Instance::Main(m) => instance_main(m),
        Instance::Sub(s) => instance_sub(s),
    }
}
