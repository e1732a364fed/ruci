pub mod record;

use std::{fmt, io};

use bytes::BytesMut;

/// remove first character, and return the trimmed str
pub fn rm_first(value: &str) -> &str {
    let mut chars = value.chars();
    chars.next();
    chars.as_str()
}

/// bytes to Captalized hex string(like 1234FF)
pub struct HexSlice<'a>(pub &'a [u8]);

impl fmt::Display for HexSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.0.len();
        write!(f, "{:06},", len)?;

        for byte in self.0 {
            write!(f, "{:02X}", byte)?;
        }
        writeln!(f)?;
        Ok(())
    }
}

/// generate an io::ErrorKind::Other
pub fn io_error<T: std::fmt::Display>(message: T) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{}", message))
}

pub fn buf_to_ob(b: BytesMut) -> Option<BytesMut> {
    if b.is_empty() {
        None
    } else {
        Some(b)
    }
}
