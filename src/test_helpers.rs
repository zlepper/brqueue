use std::fs::{create_dir_all, remove_dir_all};
use std::io::Error;

use uuid::Uuid;

// Ensures a test space is available for running tests
// Returns a folder that can be worked within
#[cfg(test)]
pub fn setup_test_storage() -> Result<String, Error> {
    let name = format!("test_storage/{}/", Uuid::new_v4().to_string());
    match std::fs::remove_dir_all(&name) {
        Ok(()) => {}
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    };
    std::fs::create_dir_all(&name)?;
    Ok(name)
}