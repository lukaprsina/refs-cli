//! Why `add` or `update` ended before it edited anything (ADR 0008, 0012). `cli` is the one place
//! that turns it into words and an exit code.

use crate::diagnostic::{LockError, SourceError};
use crate::prompt::Abort;
use crate::registry::RegistryError;
use crate::registry::pin::Stop;

#[derive(Debug)]
pub enum Failure {
    /// The Registry could not be asked, or has no such package.
    Registry(RegistryError),
    /// The remote could not be looked at: a bad URL or ref, or its tags could not be listed.
    Source(SourceError),
    /// `refs.lock` could not be read.
    Lock(LockError),
    /// A question ended without an answer.
    Asked(Abort),
    /// The request cannot be done as given, or a question had to be asked with no one to ask:
    /// the message says what to do.
    Refused(String),
    /// The command line is wrong: the message says how.
    Usage(String),
}

impl From<Stop> for Failure {
    fn from(stop: Stop) -> Failure {
        match stop {
            Stop::Source(e) => Failure::Source(e),
            Stop::Asked(abort) => Failure::Asked(abort),
            Stop::Refused(why) => Failure::Refused(why),
        }
    }
}
