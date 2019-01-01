use std::fs::File;
use std::io::BufReader;
use std::io::Error as IOError;
use std::io::Read;
use std::marker::PhantomData;
use std::path::Path;

use bincode::deserialize_from;
use serde::de::DeserializeOwned;
use serde::Serialize;

pub struct FileItemReader<T: Serialize + DeserializeOwned + Send + Clone, R: Read> {
    reader: BufReader<R>,
    _pd: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned + Send + Clone> FileItemReader<T, File> {
    pub fn new_from_file(path: &Path) -> Result<FileItemReader<T, File>, IOError> {
        let mut reader = BufReader::new(File::open(path)?);

        Ok(FileItemReader {
            reader,
            _pd: PhantomData,
        })
    }
}

impl<T: Serialize + DeserializeOwned + Send + Clone, R: Read> Iterator for FileItemReader<T, R> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if let Ok(item) = deserialize_from(&mut self.reader) {
            Some(item)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::create_dir;
    use std::io::Write;

    use bincode::serialize;

    use crate::models::{Priority, QueueItem, Tags};

    use super::*;

    #[test]
    fn can_read() {
        let filename = "test_storage/file_item_reader";

        create_dir("test_storage");

        let mut file = File::create(filename).unwrap();

        let original_items = vec![
            QueueItem::new("foo".to_string(), Tags::new(), Priority::High),
            QueueItem::new("bar".to_string(), Tags::new(), Priority::High),
            QueueItem::new("baz".to_string(), Tags::new(), Priority::High),
        ];

        for item in &original_items {
            file.write(&serialize(&item).unwrap());
        }

        // Close the file so we don't conflict with the reader below
        drop(file);

        let mut reader = FileItemReader::new_from_file(Path::new(filename)).unwrap();

        let read_items: Vec<QueueItem<String>> = reader.collect();

        for (original, read) in read_items.iter().zip(original_items) {
            assert_eq!(*original, read);
        }
    }
}
