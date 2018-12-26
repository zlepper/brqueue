use std::collections::HashMap;
use std::collections::VecDeque;
use std::convert;
use std::fmt;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::Error as IOError;
use std::io::Write;
use std::path;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;

use bincode::{deserialize, Error as BinCodeError, serialize};
use log::{debug, error};
use serde::Deserialize;
use serde::Serialize;
use serde_derive::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::Priority;
use crate::models::QueueItem;
use crate::models::Tags;

use super::queue;

#[derive(Debug)]
pub enum Error {
    QueueCorrupted,
    FailedToOpenPersistenceFiles(IOError),
    FileMutexCorrupt,
    FailedToSerializeWorkItem(BinCodeError),
}

impl convert::From<IOError> for Error {
    fn from(e: IOError) -> Self {
        Error::FailedToOpenPersistenceFiles(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::QueueCorrupted => write!(f, "Queue corrupted"),
            Error::FailedToOpenPersistenceFiles(e) => {
                write!(f, "Failed to open persistence files: {}", e)
            }
            Error::FileMutexCorrupt => write!(f, "File mutex corrupted"),
            Error::FailedToSerializeWorkItem(e) => {
                write!(f, "Failed to serialize work item: {}", e)
            }
        }
    }
}

#[derive(Clone)]
struct InternalQueueManager<T: Send + Clone> {
    high_priority_queue: queue::Queue<T>,
    low_priority_queue: queue::Queue<T>,
}

impl<T: Send + Clone> InternalQueueManager<T> {
    fn new() -> InternalQueueManager<T> {
        InternalQueueManager {
            high_priority_queue: queue::Queue::new(),
            low_priority_queue: queue::Queue::new(),
        }
    }

    fn enqueue(&mut self, item: QueueItem<T>) -> Result<(), Error> {
        let result = match item.priority {
            Priority::Low => self.low_priority_queue.enqueue(item),
            Priority::High => self.high_priority_queue.enqueue(item),
        };

        if result.is_err() {
            error!("Error when inserting into queue {}", result.unwrap_err());
            Err(Error::QueueCorrupted)
        } else {
            Ok(())
        }
    }

    fn pop(&mut self, capabilities: Vec<String>) -> Result<Option<QueueItem<T>>, Error> {
        let tags = Tags::from(capabilities);

        // Try the queues in order
        match self.high_priority_queue.pop(&tags) {
            Err(e) => Err(Error::QueueCorrupted),
            Ok(Some(entry)) => Ok(Some(entry)),
            Ok(None) => match self.low_priority_queue.pop(&tags) {
                Err(e) => Err(Error::QueueCorrupted),
                Ok(Some(entry)) => Ok(Some(entry)),
                Ok(None) => Ok(None),
            },
        }
    }

    fn get_content(&self) -> Result<Vec<QueueItem<Vec<u8>>>, Error> {
        //        match self.high_priority_queue.get_content() {
        //            Err(e) => Err(Error::QueueCorrupted),
        //            Ok(mut high_priority_content) => match self.low_priority_queue.get_content() {
        //                Err(e) => Err(Error::QueueCorrupted),
        //                Ok(mut low_priority_content) => {
        //                    high_priority_content.append(&mut low_priority_content);
        //                    Ok(high_priority_content)
        //                }
        //            },
        //        }
        Ok(Vec::new())
    }
}

#[derive(Clone)]
struct InternalQueueFileManager {
    // All the high priority tasks received
    high_priority_file: Arc<Mutex<File>>,
    // All the low priority tasks received
    low_priority_file: Arc<Mutex<File>>,
    // Contains a complete list of all the tasks that has been finished
    completed_file_index_file: Arc<Mutex<File>>,
}

impl InternalQueueFileManager {
    fn new(filename: String) -> Result<InternalQueueFileManager, Error> {
        let p = path::Path::new(&filename);
        let parent_folder = p.parent().expect("No parent for path");
        create_dir_all(parent_folder)?;

        let options = OpenOptions::new().append(true).create(true).clone();
        let high_prio_file = options.open(format!("{}_high_priority.dat", filename))?;
        let low_prio_file = options.open(format!("{}_low_priority.dat", filename))?;
        let completed_file = options.open(format!("{}_completed.dat", filename))?;

        Ok(InternalQueueFileManager {
            high_priority_file: Arc::new(Mutex::new(high_prio_file)),
            low_priority_file: Arc::new(Mutex::new(low_prio_file)),
            completed_file_index_file: Arc::new(Mutex::new(completed_file)),
        })
    }

    fn save_item<T: Serialize + Send + Clone>(&self, item: &QueueItem<T>) -> Result<(), Error> {
        let file_ref = match item.priority {
            Priority::Low => &self.low_priority_file,
            Priority::High => &self.high_priority_file,
        };

        if let Ok(mut file) = file_ref.lock() {
            let mut encoded = match serialize(item) {
                Err(e) => return Err(Error::FailedToSerializeWorkItem(e)),
                Ok(encoded) => encoded,
            };

            let size = encoded.len() as u64;

            let mut encoded_len = match serialize(&size) {
                Err(e) => return Err(Error::FailedToSerializeWorkItem(e)),
                Ok(encoded) => encoded,
            };

            encoded_len.append(&mut encoded);

            // Write the data to the disk, and ensure the
            // content has been flushed to disk.
            file.write(&encoded_len)?;
            file.flush()?;

            Ok(())
        } else {
            Err(Error::FileMutexCorrupt)
        }
    }
}

#[derive(Clone)]
pub struct QueueServer<T: Send + Clone> {
    queue: InternalQueueManager<T>,
    file_manager: InternalQueueFileManager,
    waiting: Arc<Mutex<VecDeque<Sender<QueueItem<T>>>>>,
    processing: Arc<Mutex<HashMap<String, QueueItem<T>>>>,
}

pub struct CreatedMessage {
    pub id: Uuid,
}

impl<'de, T: Send + Clone + Serialize + Deserialize<'de>> QueueServer<T> {
    pub fn new() -> Result<QueueServer<T>, Error> {
        let file_manager = InternalQueueFileManager::new("./storage/tasks".to_string())?;

        return Ok(QueueServer {
            queue: InternalQueueManager::new(),
            file_manager,
            waiting: Arc::new(Mutex::new(VecDeque::new())),
            processing: Arc::new(Mutex::new(HashMap::new())),
        });
    }

    fn add_item_to_queue(&mut self, item: QueueItem<T>) -> Result<(), Error> {
        self.queue.enqueue(item)
    }

    // Enqueues another item in the queue.
    // The generated id of the enqueued item is returned
    pub fn enqueue(
        &mut self,
        message: T,
        priority: Priority,
        required_capabilities: Vec<String>,
    ) -> Result<CreatedMessage, Error> {
        let item = QueueItem::new(message, Tags::from(required_capabilities), priority);

        match self.file_manager.save_item(&item) {
            Err(e) => return Err(e),
            _ => debug!("Item saved to disk without issues"),
        }

        let id = item.id.clone();
        let result = self.add_item_to_queue(item);
        match result {
            Err(e) => return Err(e),
            _ => debug!("Item added to queue without issues. "),
        }

        Ok(CreatedMessage { id })
    }

    pub fn pop(
        &mut self,
        capabilities: Vec<String>,
        wait_for_message: bool,
    ) -> Result<Option<QueueItem<T>>, Error> {
        match self.queue.pop(capabilities) {
            Err(e) => Err(e),
            Ok(Some(entry)) => Ok(Some(entry)),
            Ok(None) => {
                if wait_for_message {
                    //                let (tx, rc) = mpsc::channel();
                    Ok(None)
                } else {
                    Ok(None)
                }
            }
        }
    }
}

