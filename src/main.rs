extern crate bincode;
#[macro_use]
extern crate crossbeam;
extern crate env_logger;
extern crate protobuf;
extern crate uuid;

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
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
mod authentication;

fn handle_connection(mut s: TcpStream, qs: queue_server::QueueServer<Vec<u8>>, auth: authentication::Authentication) {
    thread::spawn(move || {
        let mut c = client::Client::new(qs, auth);
        c.handle_connection(s);
    });
}

fn main() {
    let mut qs = queue_server::QueueServer::new().expect("Failed to create underlying queue");
    let mut auth = authentication::Authentication::new(PathBuf::from("storage/auth")).expect("Failed to initialize authentication");

    auth.add_default_user("guest".to_string(), "guest".to_string()).expect("Failed to add default user");

    let listener = TcpListener::bind("0.0.0.0:6431").expect("Failed to bind to socket");

    println!("Listening on localhost:6431");

    for stream_result in listener.incoming() {
        let q = qs.clone();
        let a = auth.clone();
        match stream_result {
            Ok(mut stream) => handle_connection(stream, q, a),
            Err(e) => eprintln!("Stream failed: {}", e),
        }
    }
}
