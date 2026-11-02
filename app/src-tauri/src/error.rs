use calcbridge_link::{Error, RemoteKind};
use serde::Serialize;

/// What a command reports to the UI: a short machine-readable kind and a sentence for people.
#[derive(Debug, Clone, Serialize)]
pub struct CmdError {
    pub kind: &'static str,
    pub message: String,
}

pub type CmdResult<T> = Result<T, CmdError>;

impl CmdError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        CmdError { kind, message: message.into() }
    }

    pub fn no_device() -> Self {
        CmdError::new("noDevice", "No calculator is connected")
    }
}

impl From<Error> for CmdError {
    fn from(e: Error) -> Self {
        let kind = match &e {
            Error::NoDevice(_) => "noDevice",
            Error::Busy(_) => "busy",
            Error::Link(_) | Error::LinkReset => "link",
            Error::Protocol(_) => "protocol",
            Error::Remote { kind: RemoteKind::NotFound, .. } => "notFound",
            Error::Remote { kind: RemoteKind::Exists, .. } => "exists",
            Error::Remote { .. } => "refused",
            Error::Invalid(_) => "invalid",
            Error::Cancelled => "cancelled",
            Error::Io(_) => "io",
        };
        CmdError::new(kind, e.to_string())
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        CmdError::new("io", e.to_string())
    }
}

impl From<calcbridge_tns::TnsError> for CmdError {
    fn from(e: calcbridge_tns::TnsError) -> Self {
        CmdError::new("document", e.to_string())
    }
}

impl From<calcbridge_tns::MathSyntaxError> for CmdError {
    fn from(e: calcbridge_tns::MathSyntaxError) -> Self {
        CmdError::new("math", e.0)
    }
}

impl From<tauri::Error> for CmdError {
    fn from(e: tauri::Error) -> Self {
        CmdError::new("app", e.to_string())
    }
}
