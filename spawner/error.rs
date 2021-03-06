use crate::sys::error::SysError;

use std::fmt;
use std::io;

enum ErrorKind {
    Sys(SysError),
    Other(String),
    Io(io::Error),
}

pub struct Error {
    kind: ErrorKind,
}

impl Error {
    fn new(k: ErrorKind) -> Self {
        Self { kind: k }
    }

    pub fn last_os_error() -> Self {
        Error::from(SysError::last())
    }
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.kind {
            ErrorKind::Io(e) => write!(f, "{}", e),
            ErrorKind::Sys(e) => write!(f, "{}", e),
            ErrorKind::Other(s) => write!(f, "{}", s),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self)?;
        Ok(())
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::new(ErrorKind::Io(err))
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::new(ErrorKind::Other(s))
    }
}

impl From<&'static str> for Error {
    fn from(s: &'static str) -> Self {
        Error::from(s.to_string())
    }
}

impl From<SysError> for Error {
    fn from(e: SysError) -> Self {
        Error::new(ErrorKind::Sys(e))
    }
}
