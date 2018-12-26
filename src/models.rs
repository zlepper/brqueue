use std::collections::HashSet;
use std::iter::FromIterator;

use serde_derive::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Priority {
    Low,
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueItem<T: Send + Clone> {
    pub data: T,
    pub required_tags: Tags,
    pub id: uuid::Uuid,
    pub priority: Priority,
}

impl<T: Send + Clone> QueueItem<T> {
    pub fn new(data: T, tags: Tags, priority: Priority) -> QueueItem<T> {
        let id = uuid::Uuid::new_v4();

        QueueItem {
            data,
            required_tags: tags,
            priority,
            id,
        }
    }

    pub fn can_be_handled_by(&self, tags: &Tags) -> bool {
        tags.is_superset(&self.required_tags)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Tags {
    inner: HashSet<String>,
}

impl Tags {
    pub fn from(v: Vec<String>) -> Tags {
        Tags {
            inner: HashSet::from_iter(v),
        }
    }

    pub fn new() -> Tags {
        Tags {
            inner: HashSet::new(),
        }
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
