pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What kind of refusal a [`Error::Remote`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteKind {
    /// The path doesn't exist.
    NotFound,
    /// Something with that name is already there.
    Exists,
    /// Any other refusal (folder not empty, name not allowed, out of memory, ...).
    Refused,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No calculator is connected (or it was unplugged).
    #[error("{0}")]
    NoDevice(String),
    /// The USB interface is held by another program.
    #[error("{0}")]
    Busy(String),
    /// The calculator stopped answering on the link layer.
    #[error("{0}")]
    Link(String),
    /// The calculator restarted the link (it was re-plugged or woke up); the session must re-open.
    #[error("The calculator restarted the connection")]
    LinkReset,
    /// The calculator sent something we don't understand.
    #[error("{0}")]
    Protocol(String),
    /// The calculator refused a request.
    #[error("{message}")]
    Remote { kind: RemoteKind, message: String, code: Option<u16> },
    /// A request that can't be made as asked (a bad name, a folder into itself, a non-.tns upload).
    #[error("{0}")]
    Invalid(String),
    #[error("Cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    pub(crate) fn remote(kind: RemoteKind, message: impl Into<String>, code: Option<u16>) -> Self {
        Error::Remote { kind, message: message.into(), code }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::Remote { kind: RemoteKind::NotFound, .. })
    }

    pub fn is_exists(&self) -> bool {
        matches!(self, Error::Remote { kind: RemoteKind::Exists, .. })
    }

    /// The connection is unusable (unplugged, or the calculator stopped answering) and must be
    /// reopened before anything else can work.
    pub fn is_disconnect(&self) -> bool {
        matches!(self, Error::NoDevice(_) | Error::Link(_) | Error::Busy(_))
    }
}
