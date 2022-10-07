use std::{fmt, io};

pub enum Error {
    General,
    Io(io::Error),
    Lock,
    Channel,
    Timeout,
    DECODE,
    Unknown,
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::General => write!(f, "general error"),
            Error::Io(e) => write!(f, "io error = {}", e),
            Error::Lock => write!(f, "lock error"),
            Error::Channel => write!(f, "channel error"),
            Error::Timeout => write!(f, "timeout error"),
            Error::DECODE => write!(f, "decode error"),
            Error::Unknown => write!(f, "unknown error"),
        }
    }
}

