use std::io::Cursor;
use std::io::Error as IOError;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

pub fn get_size(data: &[u8]) -> Result<i32, IOError> {
    let mut reader = Cursor::new(data);

    reader.read_i32::<LittleEndian>()
}

pub fn get_size_array(size: i32) -> Result<Vec<u8>, IOError> {
    let mut writer = vec![];
    writer.write_i32::<LittleEndian>(size)?;
    Ok(writer)
}
