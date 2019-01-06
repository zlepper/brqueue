use std::collections::HashMap;
use std::collections::VecDeque;
use std::convert;
use std::fmt;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::BufWriter;
use std::io::Error as IOError;
use std::io::Write;
use std::path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread;
use std::time::Duration;

use bincode::{deserialize, Error as BinCodeError, serialize};
use crossbeam::channel::{bounded, Receiver, Sender, TrySendError};
use log::{debug, error};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_derive::{Deserialize, Serialize};
use uuid::Uuid;

use crate::internal_queue_file_manager::{Error as InternalQueueFileManagerError, InternalQueueFileManager};
use crate::models::Priority;
use crate::models::QueueItem;
use crate::models::Tags;

use super::queue;

#[derive(Debug)]
pub enum Error {
    QueueCorrupted,
    IOError(IOError),
    MutexCorrupted,
    FailedToSerializeWorkItem(BinCodeError),
    GarbageCollectionFailed
}

impl convert::From<IOError> for Error {
    fn from(e: IOError) -> Self {
        Error::IOError(e)
    }
}

impl convert::From<InternalQueueFileManagerError> for Error {
    fn from(e: InternalQueueFileManagerError) -> Self {
        match e {
            InternalQueueFileManagerError::IOError(e) => Error::IOError(e),
            InternalQueueFileManagerError::FailedToSerializeWorkItem(e) => Error::FailedToSerializeWorkItem(e),
            InternalQueueFileManagerError::MutexCorrupted => Error::MutexCorrupted,
            InternalQueueFileManagerError::GarbageCollectionFailed => Error::GarbageCollectionFailed,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::QueueCorrupted => write!(f, "Queue corrupted"),
            Error::IOError(e) => {
                write!(f, "Failed to open persistence files: {}", e)
            }
            Error::MutexCorrupted => write!(f, "File mutex corrupted"),
            Error::FailedToSerializeWorkItem(e) => {
                write!(f, "Failed to serialize work item: {}", e)
            },
            Error::GarbageCollectionFailed => write!(f, "Garbage collection failed")
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
}

#[derive(Clone)]
pub struct QueueServer<T: Send + Clone + Serialize + DeserializeOwned> {
    queue: InternalQueueManager<T>,
    file_manager: Arc<RwLock<InternalQueueFileManager<T>>>,
    // Try writing to this to see if something can be send
    waiting: Sender<QueueItem<T>>,
    // Wait on this for push like queuing
    wait_receive: Receiver<QueueItem<T>>,
    processing: Arc<Mutex<HashMap<Uuid, QueueItem<T>>>>,
}

pub struct CreatedMessage {
    pub id: Uuid,
}

impl<T: Send + Clone + Serialize + DeserializeOwned> QueueServer<T> {
    pub fn new_with_filename(filename: String) -> Result<QueueServer<T>, Error> {
        let file_manager = InternalQueueFileManager::new(filename, true)?;
        let (sender, receiver) = bounded(0);

        return Ok(QueueServer {
            queue: InternalQueueManager::new(),
            file_manager: Arc::new(RwLock::new(file_manager)),
            waiting: sender,
            wait_receive: receiver,
            processing: Arc::new(Mutex::new(HashMap::new())),
        });
    }

    pub fn new() -> Result<QueueServer<T>, Error> {
        QueueServer::new_with_filename("./storage/tasks".to_string())
    }

    fn add_item_to_queue(&mut self, item: QueueItem<T>) -> Result<(), Error> {
        match self.waiting.try_send(item) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(item)) => self.queue.enqueue(item),
            Err(_) => Err(Error::QueueCorrupted),
        }
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

        if let Ok(mut manager) = self.file_manager.read() {
            match manager.save_item(&item) {
                Err(e) => return Err(e.into()),
                _ => debug!("Item saved to disk without issues"),
            }
        } else {
            return Err(Error::MutexCorrupted);
        }

        let id = item.id.clone();
        let result = self.add_item_to_queue(item);
        match result {
            Err(e) => return Err(e),
            _ => debug!("Item added to queue without issues. "),
        }

        Ok(CreatedMessage { id })
    }

    fn pop_item(
        &mut self,
        capabilities: Vec<String>,
        wait_for_message: bool,
    ) -> Result<Option<QueueItem<T>>, Error> {
        match self.queue.pop(capabilities.clone()) {
            Err(e) => Err(e),
            Ok(Some(entry)) => Ok(Some(entry)),
            Ok(None) => {
                if wait_for_message {
                    loop {
                        select! {
                            recv(self.wait_receive) -> msg => {
                                match msg {
                                    Ok(item) => return Ok(Some(item)),
                                    Err(_) => return Err(Error::QueueCorrupted),
                                }
                            },
                            default(Duration::from_secs(1)) => {
                                // Try to receive something from the queue again
                                match self.queue.pop(capabilities.clone()) {
                                    Err(e) => return Err(e),
                                    Ok(Some(item)) => return Ok(Some(item)),
                                    Ok(None) => {},
                                }
                            }
                        }
                    }
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub fn pop(
        &mut self,
        capabilities: Vec<String>,
        wait_for_message: bool,
    ) -> Result<Option<QueueItem<T>>, Error> {
        match self.pop_item(capabilities, wait_for_message) {
            Err(e) => Err(e),
            Ok(None) => Ok(None),
            Ok(Some(item)) => {
                if let Ok(mut waiting) = self.processing.lock() {
                    waiting.insert(item.id.clone(), item.clone());
                } else {
                    return Err(Error::QueueCorrupted);
                };
                Ok(Some(item))
            }
        }
    }

    // Marks a task as completed
    pub fn acknowledge(&mut self, id: Uuid) -> Result<(), Error> {
        if let Ok(mut waiting) = self.processing.lock() {
            waiting.remove(&id);
            Ok(())
        } else {
            Err(Error::QueueCorrupted)
        }
    }

    // Marks tasks as failed, and puts them back in the queue
    pub fn fail(&mut self, id: Uuid) -> Result<(), Error> {
        let item = match self.processing.lock() {
            Ok(mut waiting) => waiting.remove(&id),
            _ => return Err(Error::QueueCorrupted),
        };

        match item {
            Some(item) => self.add_item_to_queue(item),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread::spawn;

    use crate::test_helpers::setup_test_storage;

    use super::*;

    fn setup() -> String {
        format!("{}test", setup_test_storage().unwrap())
    }

    mod enqueue_and_pop {
        use super::*;

        #[test]
        fn enqueue_and_pop_with_wait_for_message() {
            let storage_path = setup();
            let mut qs = QueueServer::new_with_filename(storage_path)
                .expect("Failed to create queue server");

            qs.enqueue("foo".to_string(), Priority::High, vec!["foo".to_string()]);
            qs.enqueue("bar".to_string(), Priority::High, vec!["bar".to_string()]);

            assert_eq!(
                qs.pop(vec!["foo".to_string(), "bar".to_string()], true)
                    .unwrap()
                    .unwrap()
                    .data,
                "foo"
            );
            assert_eq!(
                qs.pop(vec!["foo".to_string(), "bar".to_string()], true)
                    .unwrap()
                    .unwrap()
                    .data,
                "bar"
            );

            let mut q = qs.clone();

            let h1 = spawn(move || {
                thread::sleep_ms(50);
                q.enqueue("baz".to_string(), Priority::High, vec!["foo".to_string()]);
            });

            assert_eq!(
                qs.pop(vec!["foo".to_string(), "bar".to_string()], true)
                    .unwrap()
                    .unwrap()
                    .data,
                "baz"
            );

            h1.join().expect("Failed to join thread");
        }

        #[test]
        #[ignore]
        fn enqueue_and_pop_with_long_wait() {
            let storage_path = setup();
            let mut qs = QueueServer::new_with_filename(storage_path)
                .expect("Failed to create queue server");

            let mut q = qs.clone();

            let h1 = spawn(move || {
                thread::sleep_ms(3000);
                q.enqueue("baz".to_string(), Priority::High, vec!["foo".to_string()]);
            });

            assert_eq!(
                qs.pop(vec!["foo".to_string(), "bar".to_string()], true)
                    .unwrap()
                    .unwrap()
                    .data,
                "baz"
            );

            h1.join().expect("Failed to join thread");
        }

        #[test]
        fn enqueue_and_pop_without_wait_for_message() {
            let storage_path = setup();
            let mut qs = QueueServer::new_with_filename(storage_path)
                .expect("Failed to create queue server");

            qs.enqueue("foo".to_string(), Priority::High, vec!["foo".to_string()]);
            qs.enqueue("bar".to_string(), Priority::High, vec!["bar".to_string()]);

            assert_eq!(
                qs.pop(vec!["foo".to_string(), "bar".to_string()], false)
                    .unwrap()
                    .unwrap()
                    .data,
                "foo"
            );
            assert_eq!(
                qs.pop(vec!["foo".to_string(), "bar".to_string()], false)
                    .unwrap()
                    .unwrap()
                    .data,
                "bar"
            );

            assert!(qs
                .pop(vec!["foo".to_string(), "bar".to_string()], false)
                .unwrap()
                .is_none());
        }

        #[test]
        #[ignore]
        fn rough_benchmark() {
            let storage_path = setup();
            let mut qs = QueueServer::new_with_filename(storage_path)
                .expect("Failed to create queue server");
            let mut handles = Vec::new();
            for i in 0..100 {
                let mut q = qs.clone();
                let handle = spawn(move || {
                    for j in 0..10000 {
                        q.enqueue("foo".to_string(), Priority::High, vec!["foo".to_string()]);
                    }
                });
                handles.push(handle);
            }
            for h in handles {
                h.join();
            }
        }
    }

    mod acknowledge_and_fail {
        use super::*;

        #[test]
        fn acknowledge_will_remove() {
            let storage_path = setup();
            let mut qs = QueueServer::new_with_filename(storage_path)
                .expect("Failed to create queue server");

            let id = qs
                .enqueue("foo".to_string(), Priority::High, vec![])
                .expect("Failed to enqueue task");

            let item = qs
                .pop(vec![], false)
                .expect("Failed to pop item")
                .expect("Not item received");

            assert_eq!(item.id, id.id);

            qs.acknowledge(id.id).expect("Failed to acknowledge task");

            assert!(qs.pop(vec![], false).unwrap().is_none());
        }

        #[test]
        fn fail_will_re_enqueue() {
            let storage_path = setup();
            let mut qs = QueueServer::new_with_filename(storage_path)
                .expect("Failed to create queue server");

            let id = qs
                .enqueue("foo".to_string(), Priority::High, vec![])
                .expect("Failed to enqueue task");

            let item = qs
                .pop(vec![], false)
                .expect("Failed to pop item")
                .expect("Not item received");

            assert_eq!(item.id, id.id);

            qs.fail(id.id).expect("Failed to fail task");

            assert_eq!(qs.pop(vec![], false).unwrap().unwrap().id, item.id);
        }
    }

    mod priority {
        use super::*;

        #[test]
        fn opholds_priority() {
            let storage_path = setup();
            let mut qs = QueueServer::new_with_filename(storage_path)
                .expect("Failed to create queue server");

            qs.enqueue("foo".to_string(), Priority::High, vec![])
                .expect("Failed to enqueue");
            qs.enqueue("bar".to_string(), Priority::Low, vec![])
                .expect("Failed to enqueue");
            qs.enqueue("baz".to_string(), Priority::High, vec![])
                .expect("Failed to enqueue");

            assert_eq!(qs.pop(vec![], false).unwrap().unwrap().data, "foo");
            assert_eq!(qs.pop(vec![], false).unwrap().unwrap().data, "baz");
            assert_eq!(qs.pop(vec![], false).unwrap().unwrap().data, "bar");
        }
    }
}
