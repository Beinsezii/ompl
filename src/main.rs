use clap::{ArgEnum, Clap};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};

mod library;
use library::Library;

#[derive(Debug)]
enum Instance {
    Main(TcpListener),
    Sub(TcpStream),
}

#[derive(ArgEnum, Debug, Clone, Serialize, Deserialize)]
enum Action {
    Play,
    Pause,
    Stop,
    Next,
    Exit,
}

#[derive(Debug, Clone, Clap, Serialize, Deserialize)]
#[clap(name = "ompl", version = "0.1.0", author = "Beinsezii")]
struct SubArgs {
    #[clap(arg_enum)]
    action: Action,
}

#[derive(Debug, Clone, Clap)]
#[clap(name = "ompl", version = "0.1.0", author = "Beinsezii")]
struct MainArgs {
    /// Path to music libary folder
    library: std::path::PathBuf,
}

const ID: &str = "OMPL SERVER 0.1.0";

fn instance_main(listener: TcpListener) {
    let main_args = MainArgs::parse();

    let library = Library::new(&main_args.library);

    assert!(!library.songs.is_empty());

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                // confirmation ID
                if s.write_all(ID.as_bytes()).is_err() {
                    continue;
                };

                let mut response = String::from("fail");

                // exchange size
                let mut data = [0u8; std::mem::size_of::<usize>()];
                if let Err(e) = s.read_exact(&mut data) {
                    println!("{}", e)
                };
                let size: usize = usize::from_be_bytes(data);

                // exchange args
                let mut data = vec![0u8; size];
                if let Err(e) = s.read_exact(&mut data) {
                    println!("{}", e)
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
                            Action::Stop => library.stop(),
                        };
                        response = "success".to_string()
                    }
                    Err(e) => response = format!("Could not deserialize args, {}", e),
                };
                // finalize response
                if let Err(e) = s.write_all(response.as_bytes()) {
                    println!("{}", e)
                };
            }
            Err(e) => std::panic::panic_any(e),
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
    println!("{}", response);
}

fn main() {
    let socket = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 18346);

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
