use std::io;

pub fn io_error<T: std::fmt::Display>(message: T) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{}", message))
}

pub fn io_error2<T: std::fmt::Display, T2: std::fmt::Display>(
    message: T,
    message2: T2,
) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{} {}", message, message2))
}
