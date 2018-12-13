use std::collections::HashSet;
use std::collections::VecDeque;
use std::iter::{FromIterator, Iterator};
use std::sync::Arc;
use std::sync::Mutex;

// Tags for messages
#[derive(Clone)]
pub struct Tags {
    inner: HashSet<String>,
}

impl Tags {
    pub fn from(v: Vec<String>) -> Tags {
        Tags { inner: HashSet::from_iter(v) }
    }

    pub fn new() -> Tags {
        Tags { inner: HashSet::new() }
    }

    pub fn add_tag(&mut self, s: String) {
        self.inner.insert(s);
    }

    pub fn is_subset(&self, other: &Tags) -> bool {
        self.inner.is_subset(&other.inner)
    }

    pub fn is_superset(&self, other: &Tags) -> bool {
        return self.inner.is_superset(&other.inner);
    }
}

#[derive(Clone)]
struct QueueItem<T: Send> {
    data: T,
    required_tags: Tags,
}

impl<T: Send> QueueItem<T> {
    pub fn can_be_handled_by(&self, tags: &Tags) -> bool {
        tags.is_superset(&self.required_tags)
    }
}

#[derive(Clone)]
pub struct Queue<T: Send + Clone> {
    data: Arc<Mutex<VecDeque<QueueItem<T>>>>
}

#[derive(Debug)]
pub enum Error {
    QueueCorrupted
}

impl<T: Send + Clone> Queue<T> {
    pub fn new() -> Queue<T> {
        Queue { data: Arc::new(Mutex::new(VecDeque::new())) }
    }

    pub fn enqueue(&mut self, data: T, required_tags: Tags) -> Result<(), Error> {
        if let Ok(mut queue) = self.data.lock() {
            queue.push_back(QueueItem { data, required_tags });
            Ok(())
        } else {
            Err(Error::QueueCorrupted)
        }
    }

    pub fn pop(&mut self, capabilities: &Tags) -> Result<Option<T>, Error> {
        if let Ok(mut queue) = self.data.lock() {
            let mut bad_matches = VecDeque::new();

            while let Some(q) = queue.pop_front() {
                if q.can_be_handled_by(capabilities) {
                    for m in bad_matches {
                        queue.push_front(m)
                    }
                    return Ok(Some(q.data));
                } else {
                    bad_matches.push_front(q)
                }
            }
            Ok(None)
        } else {
            Err(Error::QueueCorrupted)
        }
    }

    pub fn get_content(&self) -> Result<Vec<T>, Error> {
        if let Ok(queue) = self.data.lock() {
            let r = queue.iter().map(|q| q.to_owned().data).collect();
            Ok(r)
        } else {
            Err(Error::QueueCorrupted)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod queue_item_tests {
        use super::*;

        #[test]
        fn multiple_tags() {
            let item = QueueItem { data: "foo", required_tags: Tags::from(vec!["bar".to_string(), "foo".to_string()]) };

            assert!(item.can_be_handled_by(&Tags::from(vec!["bar".to_string(), "foo".to_string()])));
            assert!(item.can_be_handled_by(&Tags::from(vec!["foo".to_string(), "bar".to_string()])));
        }

        #[test]
        fn single_bar() {
            let item = QueueItem { data: "foo", required_tags: Tags::from(vec!["bar".to_string()]) };
            assert!(item.can_be_handled_by(&Tags::from(vec!["bar".to_string(), "foo".to_string()])));
        }

        #[test]
        fn single_foo() {
            let item = QueueItem { data: "foo", required_tags: Tags::from(vec!["foo".to_string()]) };
            assert!(item.can_be_handled_by(&Tags::from(vec!["foo".to_string(), "bar".to_string()])));
        }

        #[test]
        fn no_tags_on_item() {
            let item = QueueItem { data: "foo", required_tags: Tags::new() };
            assert!(item.can_be_handled_by(&Tags::from(vec!["foo".to_string()])));
        }

        #[test]
        fn no_tags_in_request() {
            let item = QueueItem { data: "foo", required_tags: Tags::from(vec!["foo".to_string()]) };
            assert!(!item.can_be_handled_by(&Tags::new()));
        }

        #[test]
        fn more_tags_required_than_available() {
            let item = QueueItem { data: "foo", required_tags: Tags::new() };
            assert!(item.can_be_handled_by(&Tags::new()));
        }

        #[test]
        fn tag_mismatch() {
            let item = QueueItem { data: "foo", required_tags: Tags::from(vec!["bar".to_string()]) };
            assert!(!item.can_be_handled_by(&Tags::from(vec!["foo".to_string()])));
        }
    }

    #[test]
    fn can_add_and_remove() {
        let mut q = Queue::new();

        q.enqueue("foo".to_string(), Tags::new());
        q.enqueue("bar".to_string(), Tags::new());
        q.enqueue("baz".to_string(), Tags::new());

        assert_eq!(q.pop(&Tags::new()).unwrap(), Some("foo".to_string()));
        assert_eq!(q.pop(&Tags::new()).unwrap(), Some("bar".to_string()));
        assert_eq!(q.pop(&Tags::new()).unwrap(), Some("baz".to_string()));
    }

    #[test]
    fn preserves_insertion_order_even_when_capabilities_steal_from_middle() {
        let mut q = Queue::new();

        q.enqueue("foo1".to_string(), Tags::from(vec!["a".to_string()]));
        q.enqueue("foo2".to_string(), Tags::from(vec!["a".to_string()]));
        q.enqueue("foo3".to_string(), Tags::from(vec!["a".to_string()]));
        q.enqueue("bar".to_string(), Tags::from(vec!["b".to_string()]));
        q.enqueue("baz1".to_string(), Tags::from(vec!["a".to_string()]));
        q.enqueue("baz2".to_string(), Tags::from(vec!["a".to_string()]));

        assert_eq!(q.pop(&Tags::from(vec!["b".to_string()])).unwrap(), Some("bar".to_string()));
        assert_eq!(q.pop(&Tags::from(vec!["a".to_string()])).unwrap(), Some("foo1".to_string()));
        assert_eq!(q.pop(&Tags::from(vec!["a".to_string()])).unwrap(), Some("foo2".to_string()));
        assert_eq!(q.pop(&Tags::from(vec!["a".to_string()])).unwrap(), Some("foo3".to_string()));
        assert_eq!(q.pop(&Tags::from(vec!["a".to_string()])).unwrap(), Some("baz1".to_string()));
        assert_eq!(q.pop(&Tags::from(vec!["a".to_string()])).unwrap(), Some("baz2".to_string()));
    }

    pub fn can_iterate_in_order() {
        let mut q = Queue::new();

        q.enqueue("foo1".to_string(), Tags::new());
        q.enqueue("foo2".to_string(), Tags::new());
        q.enqueue("foo3".to_string(), Tags::new());

        let mut content = q.get_content().unwrap();

        assert_eq!(content.len(), 3);
        assert_eq!(content.get(0), Some(&"foo1".to_string()));
        assert_eq!(content.get(1), Some(&"foo2".to_string()));
        assert_eq!(content.get(2), Some(&"foo3".to_string()));
    }
}