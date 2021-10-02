use clap::{ArgEnum, Clap};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};

mod player;
use player::{Player, Track};

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
struct Args {
    #[clap(arg_enum)]
    action: Option<Action>,
}

const ID: &str = "OMPL SERVER 0.1.0";

fn instance_main(listener: TcpListener, _args: Args) {
    let mut player = Player::new();
    player.queue.push(Track::new(
        "/home/beinsezii/Music/NieR Replicant/Song of the Ancients - Fate.mp3".to_owned(),
    ));
    player.queue.push(Track::new(
        "/home/beinsezii/Music/NieR Replicant/Snow in Summer.mp3".to_owned(),
    ));
    println!("{:?}", player.queue);

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                // confirmation ID
                s.write_all(ID.as_bytes()).unwrap();

                #[allow(unused_assignments)]
                let mut response = String::from("fail");

                // exchange size
                let mut data = [0u8; std::mem::size_of::<usize>()];
                s.read_exact(&mut data).unwrap();
                let size: usize = usize::from_be_bytes(data);
                println!("{}", size);

                // exchange args
                let mut data = vec![0u8; size];
                s.read_exact(&mut data).unwrap();
                match bincode::deserialize::<Args>(&data) {
                    Ok(a) => {
                        match a.action {
                            Some(ref action) => match action {
                                Action::Exit => {
                                    // finalize response 2
                                    s.write_all("success".as_bytes()).unwrap();
                                    break;
                                }
                                Action::Next => player.next(),
                                Action::Pause => player.pause(),
                                Action::Play => player.play(),
                                Action::Stop => player.stop(),
                            },
                            None => (),
                        };
                        response = "success".to_string()
                    }
                    Err(e) => response = format!("Could not deserialize args, {}", e),
                };
                // finalize response
                s.write_all(response.as_bytes()).unwrap();
            }
            Err(e) => std::panic::panic_any(e),
        }
    }
}

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
    println!("{}", response);
}

fn main() {
    let args: Args = Args::parse();
    println!("{:?}", args);
    let socket = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 18346);

    let instance = match TcpListener::bind(socket) {
        Ok(v) => Instance::Main(v),
        Err(_) => match TcpStream::connect(socket) {
            Ok(v) => Instance::Sub(v),
            Err(_) => panic!("Couldn't bind socket or connect to existing. Try another port?"),
        },
    };

    match instance {
        Instance::Main(m) => instance_main(m, args),
        Instance::Sub(s) => instance_sub(s, args),
    }
}
