use std::io;

pub fn io_error<T: ToString>(message: T) -> io::Error {
    return io::Error::new(
        io::ErrorKind::Other,
        format!("Error: {}", message.to_string()),
    );
}

pub fn io_error2<T: ToString, T2: ToString>(message: T, message2: T2) -> io::Error {
    return io::Error::new(
        io::ErrorKind::Other,
        format!("{} {}", message.to_string(), message2.to_string()),
    );
}
