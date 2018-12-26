use std::collections::HashSet;
use std::collections::VecDeque;
use std::fmt;
use std::iter::{FromIterator, Iterator};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::{JoinHandle, spawn};

use crossbeam::channel::{Receiver, Sender, unbounded};

use crate::models::{QueueItem, Tags};

// Tags for messages

#[derive(Clone)]
pub struct Queue<T: Send + Clone> {
    sender: Sender<QueueItem<T>>,
    receiver: Receiver<QueueItem<T>>,
}

#[derive(Debug)]
pub enum Error {
    QueueCorrupted,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::QueueCorrupted => write!(f, "Queue corrupted"),
        }
    }
}

impl<T: Send + Clone> Queue<T> {
    pub fn new() -> Queue<T> {
        let (sender, receiver) = unbounded();

        Queue { sender, receiver }
    }

    pub fn enqueue(&mut self, item: QueueItem<T>) -> Result<(), Error> {
        match self.sender.send(item) {
            Err(e) => Err(Error::QueueCorrupted),
            Ok(()) => Ok(()),
        }
    }

    pub fn pop(&mut self, capabilities: &Tags) -> Result<Option<QueueItem<T>>, Error> {
        let mut failures = Vec::new();

        while let Ok(q) = self.receiver.try_recv() {
            if q.can_be_handled_by(capabilities) {
                if !failures.is_empty() {
                    // If there were any failures, put them in the back of the queue for now
                    for failure in failures {
                        self.sender.send(failure);
                    }
                }
                return Ok(Some(q));
            } else {
                failures.push(q);
            }
        }

        Ok(None)
    }

    pub fn get_content(&mut self) -> Result<Vec<QueueItem<T>>, Error> {
        let items: Vec<QueueItem<T>> = self.receiver.try_iter().collect();
        // Add all the items back again
        for item in &items {
            self.sender.send(item.to_owned());
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use crate::models::Priority;

    use super::*;

    mod queue_item_tests {
        use crate::models::Priority;

        use super::*;

        #[test]
        fn multiple_tags() {
            let item = QueueItem::new(
                "foo",
                Tags::from(vec!["bar".to_string(), "foo".to_string()]),
                Priority::High,
            );
            assert!(item.can_be_handled_by(&Tags::from(vec!["bar".to_string(), "foo".to_string()])));
            assert!(item.can_be_handled_by(&Tags::from(vec!["foo".to_string(), "bar".to_string()])));
        }

        #[test]
        fn single_bar() {
            let item = QueueItem::new("foo", Tags::from(vec!["bar".to_string()]), Priority::High);
            assert!(item.can_be_handled_by(&Tags::from(vec!["bar".to_string(), "foo".to_string()])));
        }

        #[test]
        fn single_foo() {
            let item = QueueItem::new("foo", Tags::from(vec!["foo".to_string()]), Priority::High);
            assert!(item.can_be_handled_by(&Tags::from(vec!["foo".to_string(), "bar".to_string()])));
        }

        #[test]
        fn no_tags_on_item() {
            let item = QueueItem::new("foo", Tags::new(), Priority::High);
            assert!(item.can_be_handled_by(&Tags::from(vec!["foo".to_string()])));
        }

        #[test]
        fn no_tags_in_request() {
            let item = QueueItem::new("foo", Tags::from(vec!["foo".to_string()]), Priority::High);
            assert!(!item.can_be_handled_by(&Tags::new()));
        }

        #[test]
        fn more_tags_required_than_available() {
            let item = QueueItem::new("foo", Tags::new(), Priority::High);
            assert!(item.can_be_handled_by(&Tags::new()));
        }

        #[test]
        fn tag_mismatch() {
            let item = QueueItem::new("foo", Tags::from(vec!["bar".to_string()]), Priority::High);
            assert!(!item.can_be_handled_by(&Tags::from(vec!["foo".to_string()])));
        }
    }

    #[test]
    fn can_add_and_remove() {
        let mut q = Queue::new();

        q.enqueue(QueueItem::new("foo", Tags::new(), Priority::High));
        q.enqueue(QueueItem::new("bar", Tags::new(), Priority::High));
        q.enqueue(QueueItem::new("baz", Tags::new(), Priority::High));

        assert_eq!(q.pop(&Tags::new()).unwrap().unwrap().data, "foo");
        assert_eq!(q.pop(&Tags::new()).unwrap().unwrap().data, "bar");
        assert_eq!(q.pop(&Tags::new()).unwrap().unwrap().data, "baz");
    }

    pub fn can_iterate_in_order() {
        let mut q = Queue::new();

        q.enqueue(QueueItem::new("foo1", Tags::new(), Priority::High));
        q.enqueue(QueueItem::new("foo2", Tags::new(), Priority::High));
        q.enqueue(QueueItem::new("foo3", Tags::new(), Priority::High));

        let mut content = q.get_content().unwrap();

        assert_eq!(content.len(), 3);
        assert_eq!(content.get(0).unwrap().data, "foo1");
        assert_eq!(content.get(1).unwrap().data, "foo2");
        assert_eq!(content.get(2).unwrap().data, "foo3");
    }
}
