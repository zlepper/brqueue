use std::collections::HashMap;
use std::convert::From;
use std::error::Error as StdError;
use std::fmt::{Display, Error as FmtError, Formatter};
use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Error as IOError;
use std::io::ErrorKind as IOErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{PoisonError, RwLock};
use std::sync::Arc;

use bcrypt;
use bincode::{deserialize_from, Error as BinCodeError, serialize_into};
use serde_derive::{Deserialize, Serialize};

// In debug builds we use a much smaller number of bcrypt rounds
// as it's extremely slow, which is really annoying when developing.
#[cfg(not(debug_assertions))]
const BCRYPT_ROUNDS: u32 = 13;

#[cfg(debug_assertions)]
const BCRYPT_ROUNDS: u32 = 6;

#[derive(Debug)]
pub enum AuthenticationError {
    IOError(IOError),
    SerializationError(BinCodeError),
    MutexCorrupted,
    BcryptError(bcrypt::BcryptError),
    UserAlreadyExists,
}

impl Display for AuthenticationError {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        match self {
            AuthenticationError::IOError(e) => write!(f, "IOError: {}", e),
            AuthenticationError::SerializationError(e) => write!(f, "Serialization error: {}", e),
            AuthenticationError::MutexCorrupted => write!(f, "Mutex corrupted"),
            AuthenticationError::BcryptError(e) => write!(f, "Bcrypt error: {}", e),
            AuthenticationError::UserAlreadyExists => write!(f, "User already exists"),
        }
    }
}

impl From<IOError> for AuthenticationError {
    fn from(e: IOError) -> Self {
        AuthenticationError::IOError(e)
    }
}

impl From<BinCodeError> for AuthenticationError {
    fn from(e: BinCodeError) -> Self {
        AuthenticationError::SerializationError(e)
    }
}

impl<T> From<PoisonError<T>> for AuthenticationError {
    fn from(_: PoisonError<T>) -> Self {
        AuthenticationError::MutexCorrupted
    }
}

impl From<bcrypt::BcryptError> for AuthenticationError {
    fn from(e: bcrypt::BcryptError) -> Self {
        AuthenticationError::BcryptError(e)
    }
}

impl StdError for AuthenticationError {}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
struct User {
    // The username of the user
    username: String,
    // The hashed password of the user
    password: String,
}

impl User {
    fn new(username: String, pw_hash: String) -> User {
        User { username, password: pw_hash }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AuthenticationData {
    users: HashMap<String, User>
}

impl AuthenticationData {
    fn new() -> AuthenticationData {
        AuthenticationData {
            users: HashMap::new(),
        }
    }

    fn add_user(&mut self, username: String, pw: String) -> Result<(), bcrypt::BcryptError> {
        let pw_hash = bcrypt::hash(&pw, BCRYPT_ROUNDS)?;

        self.users.insert(username.clone(), User::new(username, pw_hash));

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Authentication {
    data: Arc<RwLock<AuthenticationData>>,
    data_path: PathBuf,
}

fn load(path: &Path) -> Result<AuthenticationData, AuthenticationError> {
    let file = match File::open(path) {
        Err(ref e) if e.kind() == IOErrorKind::NotFound => return Ok(AuthenticationData::new()),
        Ok(f) => f,
        Err(e) => return Err(AuthenticationError::from(e)),
    };

    let reader = BufReader::new(file);
    Ok(deserialize_from(reader)?)
}

impl Authentication {
    pub fn new(path: PathBuf) -> Result<Authentication, AuthenticationError> {
        let data = load(&path)?;

        Ok(Authentication {
            data: Arc::new(RwLock::new(data)),
            data_path: path,
        })
    }

    fn save_changes(&self) -> Result<(), AuthenticationError> {
        let writer = BufWriter::new(File::create(&self.data_path)?);

        let data = self.data.read()?;

        serialize_into(writer, &*data)?;

        Ok(())
    }

    pub fn verify_user(&self, username: &str, password: &str) -> Result<bool, AuthenticationError> {
        let guard = self.data.read()?;

        let user = match guard.users.get(username) {
            Some(user) => user,
            None => return Ok(false),
        };

        Ok(bcrypt::verify(password, &user.password)?)
    }

    pub fn add_user(&mut self, username: String, password: String) -> Result<(), AuthenticationError> {
        let mut guard = self.data.write()?;

        if guard.users.contains_key(&username) {
            return Err(AuthenticationError::UserAlreadyExists);
        }

        guard.add_user(username, password);

        drop(guard);

        self.save_changes()?;

        Ok(())
    }

    // Adds the given user, only if no users currently exists
    // Returns true if the user was added, false otherwise
    pub fn add_default_user(&mut self, username: String, password: String) -> Result<bool, AuthenticationError> {
        let mut guard = self.data.write()?;

        if !guard.users.is_empty() {
            return Ok(false);
        }

        guard.add_user(username, password);

        drop(guard);

        self.save_changes()?;

        Ok(true)
    }
}

#[cfg(test)]
mod test {
    use crate::test_helpers::setup_test_storage;

    use super::*;

    fn setup() -> String {
        format!("{}users", setup_test_storage().unwrap())
    }

    #[test]
    fn can_add_user() {
        let path = setup();

        let mut a = Authentication::new(PathBuf::from(path)).unwrap();

        a.add_user("u1".to_string(), "pw".to_string()).unwrap();


        assert_eq!(a.verify_user("u1", "pw").unwrap(), true);
        assert_eq!(a.verify_user("u1", "wrong_pw").unwrap(), false);
        assert_eq!(a.verify_user("wrong_user", "pw").unwrap(), false);
    }

    #[test]
    fn can_load_users() {
        let path = setup();

        let mut a = Authentication::new(PathBuf::from(path.clone())).unwrap();

        a.add_user("u1".to_string(), "pw".to_string()).unwrap();

        drop(a);

        a = Authentication::new(PathBuf::from(path)).unwrap();

        assert_eq!(a.verify_user("u1", "pw").unwrap(), true);
        assert_eq!(a.verify_user("u1", "wrong_pw").unwrap(), false);
        assert_eq!(a.verify_user("wrong_user", "pw").unwrap(), false);
    }

    #[test]
    fn add_default_user_when_no_user_exists() {
        let path = setup();

        let mut a = Authentication::new(PathBuf::from(path.clone())).unwrap();

        assert!(a.add_default_user("guest".to_string(), "guest".to_string()).unwrap());
        assert!(a.verify_user("guest", "guest").unwrap());
    }

    #[test]
    fn _add_default_user_cant_add_when_other_exists() {
        let path = setup();

        let mut a = Authentication::new(PathBuf::from(path.clone())).unwrap();

        a.add_user("u".to_string(), "p".to_string()).unwrap();

        assert!(!a.add_default_user("guest".to_string(), "guest".to_string()).unwrap());
        assert!(!a.verify_user("guest", "guest").unwrap());
    }
}