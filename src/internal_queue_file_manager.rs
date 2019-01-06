use std::collections::HashSet;
use std::convert;
use std::fmt;
use std::fs::{create_dir_all, File, OpenOptions, remove_file, rename};
use std::io::{BufReader, BufWriter};
use std::io::{Read, Write};
use std::io::Error as IOError;
use std::marker::PhantomData;
use std::mem;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::RwLock;

use bincode::{deserialize, deserialize_from, Error as BinCodeError, serialize, serialize_into};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_derive::{Deserialize, Serialize};
use uuid::Uuid;

use crate::binary::get_size_array;
use crate::file_item_reader::FileItemReader;
use crate::models::{Priority, QueueItem, Tags};

#[derive(Debug)]
pub enum Error {
    IOError(IOError),
    FailedToSerializeWorkItem(BinCodeError),
    MutexCorrupted,
    GarbageCollectionFailed
}

impl convert::From<IOError> for Error {
    fn from(e: IOError) -> Self {
        Error::IOError(e)
    }
}

impl convert::From<BinCodeError> for Error {
    fn from(e: BinCodeError) -> Self {
        Error::FailedToSerializeWorkItem(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
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

struct FileReferences {
    // All the high priority tasks received
    high_priority_file: Arc<Mutex<BufWriter<File>>>,
    // All the low priority tasks received
    low_priority_file: Arc<Mutex<BufWriter<File>>>,
    // Contains a complete list of all the tasks that has been finished
    completed_file_index_file: Arc<Mutex<BufWriter<File>>>,
}

#[derive(Clone)]
pub struct InternalQueueFileManager<T> where T: Send + Clone + Serialize + DeserializeOwned {
    file_prefix: PathBuf,
    open_files: Arc<RwLock<FileReferences>>,
    _pd: PhantomData<T>,
    gc_lock: Arc<Mutex<()>>,
    require_flush: bool,
}

pub struct StoredItems<T: Send + Clone> {
    pub high_priority: Vec<QueueItem<T>>,
    pub low_priority: Vec<QueueItem<T>>,
}

const COMPLETED_EXTENSION: &'static str = "_completed.dat";
const HIGH_PRIORITY_EXTENSION: &'static str = "_low_priority.dat";
const LOW_PRIORITY_EXTENSION: &'static str = "_high_priority.dat";

fn get_file_path(base: &Path, extension: &str) -> PathBuf {
    Path::new(&format!("{}{}", base.to_string_lossy(), extension)).to_path_buf()
}

fn open_for_append(filename: &PathBuf) -> Result<FileReferences, Error> {
    let options = OpenOptions::new().append(true).create(true).clone();
    let high_prio_file = options.open(get_file_path(filename, HIGH_PRIORITY_EXTENSION))?;
    let low_prio_file = options.open(get_file_path(filename, LOW_PRIORITY_EXTENSION))?;
    let completed_file = options.open(get_file_path(filename, COMPLETED_EXTENSION))?;

    Ok(FileReferences {
        high_priority_file: Arc::new(Mutex::new(BufWriter::new(high_prio_file))),
        low_priority_file: Arc::new(Mutex::new(BufWriter::new(low_prio_file))),
        completed_file_index_file: Arc::new(Mutex::new(BufWriter::new(completed_file))),
    })
}

impl<T> InternalQueueFileManager<T> where T: Send + Clone + Serialize + DeserializeOwned {
    pub fn new(filename_prefix: String, require_flush: bool) -> Result<InternalQueueFileManager<T>, Error> {
        let p = Path::new(&filename_prefix.clone()).to_owned();
        let parent_folder = p.parent().expect("No parent for path");
        create_dir_all(parent_folder)?;

        let file_references = open_for_append(&p)?;

        Ok(InternalQueueFileManager {
            file_prefix: p,
            open_files: Arc::new(RwLock::new(file_references)),
            _pd: PhantomData,
            gc_lock: Arc::new(Mutex::new(())),
            require_flush
        })
    }

    fn get_file_path(&self, extension: &str) -> PathBuf {
        get_file_path(&self.file_prefix, extension)
    }

    pub fn save_item(&self, item: &QueueItem<T>) -> Result<(), Error> {
        if let Ok(mut references) = self.open_files.read() {
            let mut file_ref = match item.priority {
                Priority::Low => &references.low_priority_file,
                Priority::High => &references.high_priority_file,
            };

            if let Ok(mut file) = file_ref.lock() {
                let mut encoded = match serialize(item) {
                    Err(e) => return Err(Error::FailedToSerializeWorkItem(e)),
                    Ok(encoded) => encoded,
                };

                // Write the data to the disk, and ensure the
                // content has been flushed to disk.
                file.write(&encoded)?;
                if self.require_flush {
                    file.flush()?;
                }

                Ok(())
            } else {
                Err(Error::MutexCorrupted)
            }
        } else {
            Err(Error::MutexCorrupted)
        }
    }

    pub fn load_items(&mut self) -> Result<StoredItems<T>, Error>
    {
        if let Ok(mut guard) = self.open_files.read() {
            // Load the completed ids
            let completed_ids: HashSet<Uuid> =
                FileItemReader::new_from_file(&self.get_file_path(COMPLETED_EXTENSION))?.collect();

            let high_priority: Vec<QueueItem<T>> =
                FileItemReader::new_from_file(&self.get_file_path(HIGH_PRIORITY_EXTENSION))?
                    .filter(|item: &QueueItem<T>| !completed_ids.contains(&item.id))
                    .collect();

            let low_priority: Vec<QueueItem<T>> =
                FileItemReader::new_from_file(&self.get_file_path(LOW_PRIORITY_EXTENSION))?
                    .filter(|item: &QueueItem<T>| !completed_ids.contains(&item.id))
                    .collect();

            Ok(StoredItems { low_priority, high_priority })
        } else {
            Err(Error::MutexCorrupted)
        }
    }

    pub fn mark_as_completed(&mut self, id: &Uuid) -> Result<(), Error> {
        if let Ok(mut references) = self.open_files.read() {
            if let Ok(mut completed) = references.completed_file_index_file.lock() {
                let encoded = match serialize(id) {
                    Err(e) => return Err(Error::FailedToSerializeWorkItem(e)),
                    Ok(encoded) => encoded,
                };

                completed.write(&encoded)?;
                completed.flush()?;

                Ok(())
            } else {
                Err(Error::MutexCorrupted)
            }
        } else {
            Err(Error::MutexCorrupted)
        }
    }

    pub fn run_garbage_collection(&mut self) -> Result<(), Error> {
        if let Ok(lck) = self.gc_lock.lock() {
            let gc_files_path = Path::new(&format!("{}_gc", self.file_prefix.to_string_lossy())).to_path_buf();

            // Ensure we don't bite ourselves while running parallel
            if let Ok(mut guard) = self.open_files.write() {
                let mut temp_target = open_for_append(&gc_files_path)?;
                *guard = temp_target;
                // Automatically drop the existing target and the lock
                // When this happen it will allow the queue to continue accepting items
                // additionally it will close the normal target files so we can clean them up
            }


            let high_priority_file = self.get_file_path(HIGH_PRIORITY_EXTENSION);
            let high_priority_backup = self.get_file_path(&format!("{}.bak", HIGH_PRIORITY_EXTENSION));
            let low_priority_file = self.get_file_path(LOW_PRIORITY_EXTENSION);
            let low_priority_backup = self.get_file_path(&format!("{}.bak", LOW_PRIORITY_EXTENSION));
            let completed_file = self.get_file_path(COMPLETED_EXTENSION);

            // Create a backup of the original files, so we don't risk losing data
            rename(&high_priority_file, &high_priority_backup)?;
            rename(&low_priority_file, &low_priority_backup)?;

            // Read the completed ids, so we know which items we can remove as garbage
            let completed_ids: HashSet<Uuid> = FileItemReader::new_from_file(&completed_file)?.collect();

            // Actually write out the new items
            // First for high priority
            let mut target = BufWriter::new(File::create(high_priority_file)?);
            for item in FileItemReader::new_from_file(&high_priority_backup)?.filter(|item: &QueueItem<T>| !completed_ids.contains(&item.id)) {
                serialize_into(&mut target, &item)?;
            };
            target.flush()?;

            // And then for low priority
            target = BufWriter::new(File::create(low_priority_file)?);
            for item in FileItemReader::new_from_file(&low_priority_backup)?.filter(|item: &QueueItem<T>| !completed_ids.contains(&item.id)) {
                serialize_into(&mut target, &item)?;
            };
            target.flush()?;
            drop(target);
            drop(completed_ids);

            // Remove the backup files, since the garbage collected files have now been saved.
            remove_file(high_priority_backup)?;
            remove_file(low_priority_backup)?;
            remove_file(completed_file)?;

            // Change back to writing to the normal files
            if let Ok(mut guard) = self.open_files.write() {
                let mut normal_target = open_for_append(&self.file_prefix)?;
                *guard = normal_target;
            }

            let completed_gc_file = get_file_path(&gc_files_path, COMPLETED_EXTENSION);
            let high_priority_gc_file = get_file_path(&gc_files_path, HIGH_PRIORITY_EXTENSION);
            let low_priority_gc_file = get_file_path(&gc_files_path, LOW_PRIORITY_EXTENSION);

            // Copy the data we got while we were garbage collecting into the normal files
            // This will offset the order slightly, but it's the best we can do to stay active
            // while GC is running
            // And the best solution i could find that made rust compile the code...
            if let Ok(mut guard) = self.open_files.read() {
                // I know it's slightly in-efficient to deserialize and serialize, but i can't be bother to
                // do the binary copy right now, in a way that doesn't break the target
                // TODO: Binary copy this
                if let Ok(mut completed) = guard.completed_file_index_file.lock() {
                    for item in FileItemReader::<Uuid, File>::new_from_file(&completed_gc_file)? {
                        serialize_into(&mut *completed, &item)?;
                    };
                    completed.flush()?;
                } else {
                    return Err(Error::MutexCorrupted);
                };
                if let Ok(mut high_priority) = guard.high_priority_file.lock() {
                    for item in FileItemReader::<QueueItem<T>, File>::new_from_file(&high_priority_gc_file)? {
                        serialize_into(&mut *high_priority, &item)?;
                    };
                    high_priority.flush()?;
                } else {
                    return Err(Error::MutexCorrupted);
                };
                if let Ok(mut low_priority) = guard.low_priority_file.lock() {
                    for item in FileItemReader::<QueueItem<T>, File>::new_from_file(&low_priority_gc_file)? {
                        serialize_into(&mut *low_priority, &item)?;
                    };
                    low_priority.flush()?;
                } else {
                    return Err(Error::MutexCorrupted);
                };
            }

            // Lastly remove the temporary gc files
            remove_file(&completed_gc_file)?;
            remove_file(&high_priority_gc_file)?;
            remove_file(&low_priority_gc_file)?;

            // If we have come this far without failure it's apparently a miracle
            Ok(())
        } else {
            Err(Error::MutexCorrupted)
        }
    }

    pub fn flush_data(&mut self) -> Result<(), Error> {
        if let Ok(mut guard) = self.open_files.read() {
            if let Ok(mut file) = guard.high_priority_file.lock() {
                file.flush();
            } else {
                return Err(Error::MutexCorrupted);
            }
            if let Ok(mut file) = guard.low_priority_file.lock() {
                file.flush();
            } else {
                return Err(Error::MutexCorrupted);
            }
            if let Ok(mut file) = guard.completed_file_index_file.lock() {
                file.flush();
            } else {
                return Err(Error::MutexCorrupted);
            }
        } else {
            return Err(Error::MutexCorrupted);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{QueueItem, Tags};
    use crate::test_helpers::setup_test_storage;

    use super::*;

    fn setup() -> String {
        format!("{}_test", setup_test_storage().unwrap())
    }

    #[test]
    fn can_save_item() {
        let storage_path = setup();
        let mut manager =
            InternalQueueFileManager::new(storage_path, true).expect("Failed to create manager");

        manager
            .save_item(&QueueItem::new(
                "foo".to_string(),
                Tags::from(vec!["foo"]),
                Priority::High,
            ))
            .unwrap();
        manager
            .save_item(&QueueItem::new(
                "bar".to_string(),
                Tags::from(vec!["foo"]),
                Priority::Low,
            ))
            .unwrap();

        let StoredItems {
            high_priority,
            low_priority
        } = manager.load_items().unwrap();

        assert_eq!(high_priority.len(), 1);
        assert_eq!(low_priority.len(), 1);

        assert_eq!(high_priority.get(0).unwrap().data, "foo".to_string());
        assert_eq!(low_priority.get(0).unwrap().data, "bar".to_string());
    }

    #[test]
    fn can_save_and_read_across_threads() {
        let storage_path = setup();
        let mut manager =
            InternalQueueFileManager::new(storage_path, true).expect("Failed to create manager");

        let mut threads = Vec::new();

        for i in 0..100 {
            let mut m1 = manager.clone();
            threads.push(std::thread::spawn(move || {
                for i in 0..100 {
                    m1.save_item(&QueueItem::new(format!("foo{}", i), Tags::from(vec!["foo"]), Priority::High)).unwrap();
                    m1.save_item(&QueueItem::new(format!("foo{}", i * 1000), Tags::from(vec!["foo"]), Priority::Low)).unwrap();
                }
            }));
            let mut m2 = manager.clone();
            threads.push(std::thread::spawn(move || {
                m2.load_items().unwrap();
            }));
        }

        for thread in threads {
            thread.join().unwrap();
        }
    }

    #[test]
    fn can_mark_items_as_completed() {
        let storage_path = setup();
        let mut manager = InternalQueueFileManager::new(storage_path, true).unwrap();

        let item = QueueItem::new("foo".to_string(), Tags::new(), Priority::High);

        manager.save_item(&item).unwrap();
        manager.save_item(&QueueItem::new("bar".to_string(), Tags::new(), Priority::High)).unwrap();

        manager.mark_as_completed(&item.id).unwrap();

        let StoredItems { high_priority, low_priority } = manager.load_items().unwrap();

        assert_eq!(high_priority.len(), 1);
        assert_eq!(low_priority.len(), 0);
        assert_eq!(high_priority.get(0).unwrap().data, "bar".to_string());
    }

    #[test]
    fn can_run_garbage_collection() {
        let storage_path = setup();

        let mut manager = InternalQueueFileManager::new(storage_path.clone(), true).unwrap();

        let item1 = QueueItem::new("foo".to_string(), Tags::new(), Priority::High);
        let item2 = QueueItem::new("bar".to_string(), Tags::new(), Priority::High);
        let item3 = QueueItem::new("baz".to_string(), Tags::new(), Priority::High);

        manager.save_item(&item1).unwrap();
        manager.save_item(&item2).unwrap();
        manager.save_item(&item3).unwrap();

        manager.mark_as_completed(&item1.id).unwrap();

        manager.run_garbage_collection().unwrap();

        drop(manager);

        manager = InternalQueueFileManager::new(storage_path, true).unwrap();

        let StoredItems { low_priority, high_priority } = manager.load_items().unwrap();

        assert_eq!(high_priority.len(), 2);
        assert_eq!(high_priority, vec![item2, item3]);
    }

    #[test]
    fn can_add_items_will_gc_is_running_without_loss() {
        let storage_path = setup();

        let mut manager = InternalQueueFileManager::new(storage_path.clone(), true).unwrap();

        // Add a bunch of items we don't really care about
        for i in 0..100000 {
            let item = QueueItem::new("foo".to_string(), Tags::new(), if i % 2 == 0 { Priority::High } else { Priority::Low });
            manager.save_item(&item).expect("Failed to save trash item");
            manager.mark_as_completed(&item.id);
        }
        // Create fake queue items
        let mut fake_items = Vec::new();
        for i in 0..5 {
            fake_items.push(QueueItem::new("foo".to_string(), Tags::new(), if i % 2 != 0 { Priority::High } else { Priority::Low }));
        }

        let mut m = manager.clone();
        let handle = std::thread::spawn(move || {
            m.run_garbage_collection().expect("Garbage collection failed");
        });


        for item in &fake_items {
            manager.save_item(&item).unwrap();
        }

        let hp_items_set: HashSet<Uuid> = fake_items.iter().filter(|item| item.priority == Priority::High).map(|item| item.id).collect();
        let lp_items_set: HashSet<Uuid> = fake_items.iter().filter(|item| item.priority == Priority::Low).map(|item| item.id).collect();

        // Wait for GC to finish
        handle.join().unwrap();

        let StoredItems { high_priority, low_priority } = manager.load_items().unwrap();

        let hp_set: HashSet<Uuid> = high_priority.iter().map(|item| item.id).collect();
        let lp_set: HashSet<Uuid> = low_priority.iter().map(|item| item.id).collect();
        assert_eq!(hp_set, hp_items_set);
        assert_eq!(lp_set, lp_items_set);
    }

    #[test]
    #[ignore]
    fn can_gc_many_items() {
        let storage_path = setup();

        let mut manager = InternalQueueFileManager::new(storage_path.clone(), false).unwrap();

        // Add a bunch of items we don't really care about
        // And we don't really care how long it takes
        for i in 0..10000000 {
            let item = QueueItem::new("foo".to_string(), Tags::new(), if i % 2 == 0 { Priority::High } else { Priority::Low });
            manager.save_item(&item).expect("Failed to save trash item");
            manager.mark_as_completed(&item.id);
        }

        manager.run_garbage_collection().unwrap();
    }

    mod how_does_the_lib_work {
        use std::io::Cursor;

        use bincode::{deserialize_from, serialize};

        use crate::models::{QueueItem, Tags};

        use super::*;

        #[test]
        fn serialize_deserialize() {
            let item1 = QueueItem::new(
                "foo".to_string(),
                Tags::from(vec!["foo", "bar", "baz"]),
                Priority::High,
            );
            let item2 = QueueItem::new(
                "bar".to_string(),
                Tags::from(vec!["Cake is fantastic", "I can make icecream"]),
                Priority::Low,
            );

            let mut b1 = serialize(&item1).unwrap();
            let mut b2 = serialize(&item2).unwrap();

            b1.append(&mut b2);

            let mut reader = Cursor::new(b1);

            let i1: QueueItem<String> = deserialize_from(&mut reader).unwrap();
            let i2: QueueItem<String> = deserialize_from(&mut reader).unwrap();

            assert_eq!(item1.id, i1.id);
            assert_eq!(item1.data, i1.data);
            assert_eq!(item1.required_tags, i1.required_tags);
            assert_eq!(item2.id, i2.id);
            assert_eq!(item2.data, i2.data);
            assert_eq!(item2.required_tags, i2.required_tags);
        }
    }
}
