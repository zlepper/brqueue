use std::convert;
use std::fmt;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::Error as IOError;
use std::io::Write;
use std::path;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::Mutex;

use bincode::{deserialize, Error as BinCodeError, serialize};
use log::{debug, error};
use serde_derive::{Deserialize, Serialize};
use uuid::Uuid;

use super::queue;
use std::collections::HashMap;
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkEntry {
    message: Vec<u8>,
    id: String,
}

impl WorkEntry {
    fn new(message: Vec<u8>) -> WorkEntry {
        WorkEntry {
            id: Uuid::new_v4().to_string(),
            message,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct WorkItem {
    priority: Priority,
    required_capabilities: Vec<String>,
    entry: WorkEntry,
}

impl WorkItem {
    pub fn new(message: Vec<u8>, priority: Priority, required_capabilities: Vec<String>) -> WorkItem {
        WorkItem {
            priority,
            required_capabilities,
            entry: WorkEntry::new(message),
        }
    }
}

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
            Error::FailedToOpenPersistenceFiles(e) => write!(f, "Failed to open persistence files: {}", e),
            Error::FileMutexCorrupt => write!(f, "File mutex corrupted"),
            Error::FailedToSerializeWorkItem(e) => write!(f, "Failed to serialize work item: {}", e),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Priority {
    Low,
    High,
}


#[derive(Clone)]
struct InternalQueueManager {
    high_priority_queue: queue::Queue<WorkEntry>,
    low_priority_queue: queue::Queue<WorkEntry>,
}

impl InternalQueueManager {
    fn new() -> InternalQueueManager {
        InternalQueueManager {
            high_priority_queue: queue::Queue::new(),
            low_priority_queue: queue::Queue::new(),
        }
    }

    fn enqueue(&mut self, entry: WorkEntry, priority: Priority, required_capabilities: Vec<String>) -> Result<(), Error> {
        let tags = queue::Tags::from(required_capabilities);
        let result = match priority {
            Priority::Low => self.low_priority_queue.enqueue(entry, tags),
            Priority::High => self.high_priority_queue.enqueue(entry, tags),
        };

        if result.is_err() {
            error!("Error when inserting into queue {}", result.unwrap_err());
            Err(Error::QueueCorrupted)
        } else {
            Ok(())
        }
    }

    fn pop(&mut self, capabilities: Vec<String>) -> Result<Option<WorkEntry>, Error> {
        let tags = queue::Tags::from(capabilities);

        // Try the queues in order
        match self.high_priority_queue.pop(&tags) {
            Err(e) => Err(Error::QueueCorrupted),
            Ok(Some(entry)) => Ok(Some(entry)),
            Ok(None) => match self.low_priority_queue.pop(&tags) {
                Err(e) => Err(Error::QueueCorrupted),
                Ok(Some(entry)) => Ok(Some(entry)),
                Ok(None) => Ok(None),
            }
        }
    }

    fn get_content(&self) -> Result<Vec<WorkEntry>, Error> {
        match self.high_priority_queue.get_content() {
            Err(e) => Err(Error::QueueCorrupted),
            Ok(mut high_priority_content) => match self.low_priority_queue.get_content() {
                Err(e) => Err(Error::QueueCorrupted),
                Ok(mut low_priority_content) => {
                    high_priority_content.append(&mut low_priority_content);
                    Ok(high_priority_content)
                }
            }
        }
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

    fn save_item(&self, item: &WorkItem) -> Result<(), Error> {
        let file_ref = match item.priority {
            Priority::Low => &self.low_priority_file,
            Priority::High => &self.high_priority_file,
        };

        if let Ok(mut file) = file_ref.lock() {
            let mut encoded = match serialize(item) {
                Err(e) => return Err(Error::FailedToSerializeWorkItem(e)),
                Ok(encoded) => encoded
            };

            let size = encoded.len() as u64;

            let mut encoded_len = match serialize(&size) {
                Err(e) => return Err(Error::FailedToSerializeWorkItem(e)),
                Ok(encoded) => encoded
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
pub struct QueueServer {
    queue: InternalQueueManager,
    file_manager: InternalQueueFileManager,
    waiting: Arc<Mutex<VecDeque<Sender>>>,
    processing: Arc<Mutex<HashMap<String, WorkItem>>>
}

pub struct CreatedMessage {
    pub id: String,
}

impl QueueServer {
    pub fn new() -> Result<QueueServer, Error> {
        let file_manager = InternalQueueFileManager::new("./storage/tasks".to_string())?;

        return Ok(QueueServer {
            queue: InternalQueueManager::new(),
            file_manager,
            waiting: Arc::new(Mutex::new(VecDeque::new()))
        });
    }

    fn add_item_to_queue(&mut self, item: &WorkItem) -> Result<(), Error> {
        self.queue.enqueue(item.entry.to_owned(), item.priority.to_owned(), item.required_capabilities.to_owned())
    }

    // Enqueues another item in the queue.
    // The generated id of the enqueued item is returned
    pub fn enqueue(&mut self, message: Vec<u8>, priority: Priority, required_capabilities: Vec<String>) -> Result<CreatedMessage, Error> {
        let item = WorkItem::new(message, priority, required_capabilities.clone());

        match self.file_manager.save_item(&item) {
            Err(e) => return Err(e),
            _ => debug!("Item saved to disk without issues"),
        }


        let id = item.entry.id.clone();
        let result = self.add_item_to_queue(&item);
        match result {
            Err(e) => return Err(e),
            _ => debug!("Item added to queue without issues. ")
        }

        Ok(CreatedMessage { id })
    }

    pub fn pop(&mut self, capabilities: Vec<String>, wait_for_message: bool) -> Result<Option<WorkEntry>, Error> {
        match self.queue.pop(capabilities) {
            Err(e) => Err(e),
            Ok(Some(entry)) => Ok(Some(entry)),
            Ok(None) => if wait_for_message {
                let (tx, rc) = mpsc::channel();

            } else {
                Ok(None)
            }
        }
    }
}

