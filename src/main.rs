extern crate bincode;
#[macro_use]
extern crate crossbeam;
extern crate env_logger;
extern crate protobuf;
extern crate uuid;

use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

mod binary;
mod client;
mod file_item_reader;
mod internal_queue_file_manager;
mod models;
mod queue;
mod queue_server;
mod rpc;
mod test_helpers;

fn handle_connection(mut s: TcpStream, qs: queue_server::QueueServer<Vec<u8>>) {
    thread::spawn(move || {
        let mut c = client::Client::new(qs);
        c.handle_connection(s);
    });
}

fn main() {
    let mut qs = match queue_server::QueueServer::new() {
        Err(e) => {
            eprintln!("Failed to create underlying queue: {}", e);
            std::process::exit(1);
        }
        Ok(qs) => qs,
    };

    let listener = TcpListener::bind("0.0.0.0:6431").expect("Failed to bind to socket");

    println!("Listening on localhost:6431");

    for stream_result in listener.incoming() {
        let q = qs.clone();
        match stream_result {
            Ok(mut stream) => handle_connection(stream, q),
            Err(e) => eprintln!("Stream failed: {}", e),
        }
    }
}
