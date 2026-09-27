//! Failure modes of the capture path.
//!
//! The governing rule is that capture must never take down the thing it is
//! observing. An agent turn completes whether or not we managed to record it, so
//! every operation here returns a `Result` the caller is expected to log and
//! move on from — and the failures a *user* needs to know about are additionally
//! recorded on the Session row, because a log line nobody reads is the same as
//! silence.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// The store could not be opened or written — disk full, permissions, a
    /// read-only volume. Surfaces as `Stopped` capture health.
    Storage(String),
    /// Another process already holds this Project's writer lock. Not an error
    /// condition so much as a fact: the other window is doing the recording.
    AlreadyLocked,
    /// Redaction did not complete, so the content was not written. Fail closed —
    /// the Session is flagged and the raw content is dropped rather than stored.
    RedactionFailed(String),
    /// The database exists but is not a store we understand.
    SchemaTooNew { found: i64, supported: i64 },
    /// A blob could not be written or read back.
    Blob(String),
    /// The ingest service refused or could not be reached — a registry call
    /// (register, connect, list) that never touched the local store. Kept apart
    /// from `Storage` so a `422` from the server is not reported as a disk that
    /// cannot be written; the detail is already self-describing.
    Remote(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(detail) => write!(f, "session store is not writable: {detail}"),
            Self::AlreadyLocked => write!(
                f,
                "another Atlas window is already recording this project"
            ),
            Self::RedactionFailed(detail) => {
                write!(f, "redaction failed, content was not stored: {detail}")
            }
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "session store was written by a newer Atlas (schema {found}, this build supports {supported})"
            ),
            Self::Blob(detail) => write!(f, "spilled payload unavailable: {detail}"),
            Self::Remote(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Self::Storage(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
