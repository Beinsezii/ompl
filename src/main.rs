use std::net::{Ipv4Addr, SocketAddrV4, TcpStream, TcpListener};
use std::io::{Write, Read};
mod player;
use player::{Player, Track};

#[derive(Debug)]
enum Instance {
    Main(TcpListener),
    Sub(TcpStream),
}

fn instance_main(listener: TcpListener) {
    let mut player = Player::new();
    player.queue.push(Track::new("/home/beinsezii/Music/NieR Replicant/Song of the Ancients - Fate.mp3".to_owned()));
    player.queue.push(Track::new("/home/beinsezii/Music/NieR Replicant/Snow in Summer.mp3".to_owned()));
    println!("{:?}", player.queue);

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                let mut result = String::new();
                s.read_to_string(&mut result).unwrap();
                match result.as_str().trim() {
                    "exit" => break,
                    "next" => player.next(),
                    "pause" => player.pause(),
                    "play" => player.play(),
                    "stop" => player.stop(),
                    _ => println!("{}", result),
                };
            }
            Err(e) => std::panic::panic_any(e),
        }
    }
}

fn instance_sub(mut stream: TcpStream) {
    let mut message = String::new();
    std::io::stdin().read_line(&mut message).unwrap();
    stream.write_all(message.as_bytes()).unwrap();
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
