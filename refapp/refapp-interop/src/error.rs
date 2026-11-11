//! The driver's error.

use std::fmt;

/// Why a run ended without finishing.
#[derive(Debug)]
pub enum Error {
    /// A socket, a file, or a log write failed.
    Io(std::io::Error),
    /// A node broke the step contract of §8.1 (APP-090).
    Contract(String),
    /// The case or the loss script cannot be driven.
    Script(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "i/o: {e}"),
            Error::Contract(m) => write!(f, "step contract: {m}"),
            Error::Script(m) => write!(f, "script: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
