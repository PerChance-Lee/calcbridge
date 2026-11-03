//! TI-Nspire `.tns` documents: reading, writing, and building Notes pages with real 2D math boxes,
//! Python programs and Lua scripts.
//!
//! The container is a ZIP variant (first local-header magic `*TIMLP` + version, end record `TIPD`);
//! XML parts use TI's "method 13" (3DES-encrypted, raw-deflated TIXC token streams). See
//! `docs/PROTOCOL.md` in the repository for the format notes this is built from.

pub mod container;
pub mod crypto;
pub mod document;
pub mod notes;
pub mod r2d;
pub mod textio;
pub mod tixc;
pub mod tixc_decode;
pub mod xml;

pub use container::{Entry, Method};
pub use document::{Page, TnsDocument, build_lua, build_notes, build_python, build_text};
pub use notes::{MathSyntaxError, Paragraph};

#[derive(Debug, thiserror::Error)]
pub enum TnsError {
    #[error("not a TI-Nspire document (.tns)")]
    NotTns,
    #[error("damaged .tns: {0}")]
    Damaged(String),
    #[error("couldn't decode the document's XML: {0}")]
    Tixc(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, TnsError>;
