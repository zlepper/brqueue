use core::borrow::BorrowMut;
use std::collections::HashSet;
use std::io::Cursor;
use std::io::Error as StdError;
use std::io::Read;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use log::debug;
use protobuf::{Message, ProtobufError};
use uuid::Uuid;

use crate::models;

use super::queue_server;
use super::rpc;

fn get_size(data: &[u8]) -> Result<i32, StdError> {
    let mut reader = Cursor::new(data);

    reader.read_i32::<LittleEndian>()
}

fn get_size_array(size: i32) -> Result<Vec<u8>, StdError> {
    let mut writer = vec![];
    writer.write_i32::<LittleEndian>(size)?;
    Ok(writer)
}

enum Error {
    ConnectionError(StdError),
    ReadError(StdError),
    ParseError(ProtobufError),
    ResponseError(StdError),
    ConnectionReset,
    RequestError(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            Error::ConnectionError(e) => write!(f, "ConnectionError: {}", e),
            Error::ReadError(e) => write!(f, "ReadError: {}", e),
            Error::ParseError(e) => write!(f, "ParseError: {}", e),
            Error::ResponseError(e) => write!(f, "ResponseError: {}", e),
            Error::ConnectionReset => write!(f, "Connection reset"),
            Error::RequestError(s) => write!(f, "{}", s),
        }
    }
}

fn read_message(s: &mut TcpStream) -> Result<Vec<u8>, Error> {
    let mut size = [0, 0, 0, 0];

    match s.read(&mut size) {
        Ok(0) => {
            println!("Read nothing");
            Err(Error::ConnectionReset)
        }
        Ok(read) => match get_size(&size) {
            Ok(message_size) => {
                let mut data = vec![0u8; message_size as usize];

                match s.read(&mut data) {
                    Ok(read_size) => Ok(data),
                    Err(e) => {
                        eprintln!("Failed to read message: {}", e);
                        Err(Error::ReadError(e))
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to read message size {}", e);
                Err(Error::ReadError(e))
            }
        },
        Err(e) => {
            eprintln!("Failed to read size of next message: {}", e);
            Err(Error::ConnectionError(e))
        }
    }
}

fn parse_request(data: Vec<u8>) -> Result<rpc::RequestWrapper, Error> {
    match protobuf::parse_from_bytes(&data) {
        Ok(message) => Ok(message),
        Err(e) => Err(Error::ParseError(e)),
    }
}

fn to_binary(message: rpc::ResponseWrapper) -> Result<Vec<u8>, Error> {
    let mut data = Vec::new();
    match message.write_to_vec(&mut data) {
        Err(e) => Err(Error::ParseError(e)),
        Ok(_) => Ok(data),
    }
}

fn send_reply(s: &mut TcpStream, message: rpc::ResponseWrapper) -> Result<(), Error> {
    let mut data = to_binary(message)?;

    let mut size = match get_size_array(data.len() as i32) {
        Ok(size) => size,
        Err(e) => return Err(Error::ResponseError(e)),
    };

    size.append(&mut data);

    match s.write_all(&size) {
        Err(e) => Err(Error::ResponseError(e)),
        Ok(_) => Ok(()),
    }
}

fn reply_error(s: &mut TcpStream, message: String, ref_id: i32) {
    let mut response = rpc::ErrorResponse::new();
    response.set_message(message);
    let mut wrapper = rpc::ResponseWrapper::new();
    wrapper.set_error(response);
    wrapper.set_refId(ref_id);

    match send_reply(s, wrapper) {
        Ok(_) => {}
        Err(e) => eprintln!("Failed to write error: {}", e),
    }
}

// One client corresponds to exactly one connection
// to the server
#[derive(Clone)]
pub struct Client {
    queue_server: queue_server::QueueServer<Vec<u8>>,
    outstanding_tasks: Arc<Mutex<HashSet<Uuid>>>,
}

impl Client {
    pub fn new(queue_server: queue_server::QueueServer<Vec<u8>>) -> Client {
        Client {
            queue_server,
            outstanding_tasks: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn pop(&mut self, request: &rpc::PopRequest) -> Result<rpc::ResponseWrapper, Error> {
        let capabilities = request.get_availableCapabilities();
        let wait_for_messages = request.get_waitForMessage();

        let mut qs = &mut self.queue_server.to_owned();

        match qs.pop(capabilities.to_vec(), wait_for_messages) {
            Ok(Some(item)) => {
                if let Ok(mut tasks) = self.outstanding_tasks.lock() {
                    tasks.insert(item.id.clone());
                }

                let mut response = rpc::PopResponse::new();
                response.set_id(item.id.to_string());
                response.set_message(item.data);
                response.set_hadResult(true);
                let mut wrapper = rpc::ResponseWrapper::new();
                wrapper.set_pop(response);
                Ok(wrapper)
            }
            Ok(None) => {
                let mut response = rpc::PopResponse::new();
                response.set_hadResult(false);
                let mut wrapper = rpc::ResponseWrapper::new();
                wrapper.set_pop(response);
                Ok(wrapper)
            }
            Err(e) => {
                eprintln!("Failed to pop message: {}", e);
                Err(Error::RequestError(format!("Failed to pop message: {}", e)))
            }
        }
    }

    fn acknowledge(&mut self, request: &rpc::AcknowledgeRequest) -> Result<rpc::ResponseWrapper, Error> {
        let id = request.get_id();

        match Uuid::parse_str(id) {
            Ok(uuid) => {
                let mut qs = &mut self.queue_server.to_owned();
                match qs.acknowledge(uuid) {
                    Ok(()) => {
                        if let Ok(mut tasks) = self.outstanding_tasks.lock() {
                            tasks.remove(&uuid);
                        }

                        let mut response = rpc::AcknowledgeResponse::new();
                        let mut wrapper = rpc::ResponseWrapper::new();
                        wrapper.set_acknowledge(response);
                        Ok(wrapper)
                    }
                    Err(e) => {
                        eprintln!("Failed to acknowledge message: {}", e);
                        Err(Error::RequestError(format!("Failed to acknowledge message: {}", e)))
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to parse id to UUID: {}", e);
                Err(Error::RequestError(format!("Failed to parse id to UUID: {}", e)))
            }
        }
    }

    fn enqueue(&mut self, request: &rpc::EnqueueRequest) -> Result<rpc::ResponseWrapper, Error> {
        let priority = request.get_priority();
        let message = request.get_message();
        let required_capabilities = request.get_requiredCapabilities();

        let prio = match priority {
            rpc::Priority::LOW => models::Priority::Low,
            rpc::Priority::HIGH => models::Priority::High,
        };

        let mut qs = &mut self.queue_server.to_owned();

        match qs.enqueue(message.to_vec(), prio, required_capabilities.to_vec()) {
            Ok(created) => {
                let mut response = rpc::EnqueueResponse::new();
                response.set_id(created.id.to_string());
                let mut wrapper = rpc::ResponseWrapper::new();
                wrapper.set_enqueue(response);
                Ok(wrapper)
            }
            Err(e) => {
                eprintln!("Failed to enqueue message: {}", e);
                Err(Error::RequestError(format!("Failed to enqueue message: {}", e)))
            }
        }
    }

    fn drop_connection(mut self) {
        if let Ok(mut tasks) = self.outstanding_tasks.lock() {
            for id in tasks.iter() {
                match self.queue_server.fail(*id) {
                    Err(e) => { eprintln!("Failed to fail task: {}", e) }
                    _ => {}
                };
            }
        }
    }

    pub fn handle_connection(mut self, mut s: TcpStream) {
        loop {
            match read_message(&mut s) {
                Ok(data) => {
                    let mut s = match s.try_clone() {
                        Ok(socket) => socket,
                        Err(e) => {
                            eprintln!("Failed to clone socket, assuming this means it's dead..");
                            return;
                        }
                    };
                    let mut se = self.clone();
                    thread::spawn(move || {
                        let message = match parse_request(data) {
                            Ok(message) => message,
                            Err(e) => {
                                eprintln!("Failed to parse message: {}", e);
                                return;
                            }
                        };

                        let ref_id = message.get_refId();

                        let result = if message.has_enqueue() {
                            let enqueue_request = message.get_enqueue();
                            se.enqueue(enqueue_request)
                        } else if message.has_acknowledge() {
                            let acknowledge_request = message.get_acknowledge();
                            se.acknowledge(acknowledge_request)
                        } else if message.has_pop() {
                            let pop_request = message.get_pop();
                            se.pop(pop_request)
                        } else {
                            Err(Error::RequestError("Unknown request".to_string()))
                        };

                        match result {
                            Ok(mut wrapper) => {
                                wrapper.set_refId(ref_id);
                                match send_reply(&mut s, wrapper) {
                                    Err(e) => { eprintln!("Failed to send reply: {}", e) }
                                    _ => debug!("Response send without issue for ref_id '{}'", ref_id)
                                };
                            }
                            Err(Error::RequestError(error_message)) => {
                                reply_error(&mut s, error_message, ref_id);
                            }
                            Err(e) => {
                                eprintln!("Unexpected error {}", e);
                            }
                        }
                    });
                }
                Err(e) => {
                    println!("Failed to read new message from client: {}", e);
                    drop(s);
                    self.drop_connection();
                    return;
                }
            }
        }
    }
}
