use core::borrow::BorrowMut;
use std::collections::HashSet;
use std::io::Cursor;
use std::io::Error as StdError;
use std::io::Read;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
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
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            Error::ConnectionError(e) => write!(f, "ConnectionError: {}", e),
            Error::ReadError(e) => write!(f, "ReadError: {}", e),
            Error::ParseError(e) => write!(f, "ParseError: {}", e),
            Error::ResponseError(e) => write!(f, "ResponseError: {}", e),
            Error::ConnectionReset => write!(f, "Connection reset"),
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

    match s.write(&size) {
        Err(e) => Err(Error::ResponseError(e)),
        Ok(_) => Ok(()),
    }
}

fn reply_error(s: &mut TcpStream, message: String, ref_id: i32) {
    let mut response = rpc::ErrorResponse::new();
    response.set_message(message);
    response.set_refId(ref_id);
    let mut wrapper = rpc::ResponseWrapper::new();
    wrapper.set_error(response);

    match send_reply(s, wrapper) {
        Ok(_) => {}
        Err(e) => eprintln!("Failed to write error: {}", e),
    }
}

// One client corresponds to exactly one connection
// to the server
pub struct Client {
    queue_server: queue_server::QueueServer<Vec<u8>>,
    outstanding_tasks: HashSet<Uuid>,
}

impl Client {
    pub fn new(queue_server: queue_server::QueueServer<Vec<u8>>) -> Client {
        Client { queue_server, outstanding_tasks: HashSet::new() }
    }

    fn pop(&mut self, request: &rpc::PopRequest, s: &mut TcpStream) {
        let capabilities = request.get_availableCapabilities();
        let wait_for_messages = request.get_waitForMessage();
        let ref_id = request.get_refId();

        let mut qs = &mut self.queue_server.to_owned();

        match qs.pop(capabilities.to_vec(), wait_for_messages) {
            Ok(Some(item)) => {
                self.outstanding_tasks.insert(item.id.clone());

                let mut response = rpc::PopResponse::new();
                response.set_id(item.id.to_string());
                response.set_message(item.data);
                response.set_hadResult(true);
                response.set_refId(ref_id);
                let mut wrapper = rpc::ResponseWrapper::new();
                wrapper.set_pop(response);
                send_reply(s, wrapper);
            }
            Ok(None) => {
                let mut response = rpc::PopResponse::new();
                response.set_hadResult(false);
                response.set_refId(ref_id);
                let mut wrapper = rpc::ResponseWrapper::new();
                wrapper.set_pop(response);
                send_reply(s, wrapper);
            }
            Err(e) => {
                eprintln!("Failed to pop message: {}", e);
                reply_error(s, format!("Failed to pop message: {}", e), ref_id);
            }
        }
    }

    fn acknowledge(&mut self, request: &rpc::AcknowledgeRequest, s: &mut TcpStream) {
        let id = request.get_id();
        let ref_id = request.get_refId();

        match Uuid::parse_str(id) {
            Ok(uuid) => {
                let mut qs = &mut self.queue_server.to_owned();
                match qs.acknowledge(uuid) {
                    Ok(()) => {
                        self.outstanding_tasks.remove(&uuid);

                        let mut response = rpc::AcknowledgeResponse::new();
                        response.set_refId(ref_id);
                        let mut wrapper = rpc::ResponseWrapper::new();
                        wrapper.set_acknowledge(response);
                        match send_reply(s, wrapper) {
                            Err(e) => {
                                eprintln!("Failed to send acknowledge reply: {}", e);
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to acknowledge message: {}", e);
                        reply_error(s, format!("Failed to acknowledge message: {}", e), ref_id);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to parse id to UUID: {}", e);
                reply_error(s, format!("Failed to parse id to UUID: {}", e), ref_id);
            }
        }
    }

    fn enqueue(&mut self, request: &rpc::EnqueueRequest, s: &mut TcpStream) {
        let priority = request.get_priority();
        let message = request.get_message();
        let required_capabilities = request.get_requiredCapabilities();
        let ref_id = request.get_refId();

        let prio = match priority {
            rpc::Priority::LOW => models::Priority::Low,
            rpc::Priority::HIGH => models::Priority::High,
        };

        let mut qs = &mut self.queue_server.to_owned();

        match qs.enqueue(message.to_vec(), prio, required_capabilities.to_vec()) {
            Ok(created) => {
                let mut response = rpc::ResponseWrapper::new();
                let mut enqueue_response = rpc::EnqueueResponse::new();
                enqueue_response.set_id(created.id.to_string());
                enqueue_response.set_refId(ref_id);
                response.set_enqueue(enqueue_response);
                match send_reply(s, response) {
                    Err(e) => eprintln!("Failed to respond: {}", e),
                    Ok(()) => debug!("Responded successfully"),
                };
            }
            Err(e) => {
                eprintln!("Failed to enqueue message: {}", e);
                reply_error(s, format!("Failed to enqueue message: {}", e), ref_id);
            }
        }
    }

    fn drop_connection(mut self) {
        for id in self.outstanding_tasks {
            match self.queue_server.fail(id) {
                Err(e) => { eprintln!("Failed to fail task: {}", e) },
                _ => {},
            };
        }
    }

    pub fn handle_connection(mut self, mut s: TcpStream) {
        loop {
            match read_message(&mut s) {
                Ok(data) => {
                    let message = match parse_request(data) {
                        Ok(message) => message,
                        Err(e) => {
                            eprintln!("Failed to parse message: {}", e);
                            continue;
                        }
                    };

                    if message.has_enqueue() {
                        let enqueue_request = message.get_enqueue();
                        self.enqueue(enqueue_request, &mut s);
                    } else if message.has_acknowledge() {
                        let acknowledge_request = message.get_acknowledge();
                        self.acknowledge(acknowledge_request, &mut s);
                    } else if message.has_pop() {
                        let pop_request = message.get_pop();
                        self.pop(pop_request, &mut s);
                    }
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
