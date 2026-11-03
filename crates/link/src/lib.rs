//! The calculator side of CalcBridge: the CX II's NNSE link layer, NavNet sessions and services
//! (files, screenshots, keys, OS install), a high-level [`Calculator`], and an in-process simulator
//! that reproduces the quirks seen on a real CX II.
//!
//! Byte layouts follow libnspire (GPL-3.0) and tilibs (GPL-2.0+); every service except OS install
//! was checked against a CX II on OS 5.4.

pub mod calculator;
pub mod error;
pub mod keys;
pub mod navnet;
pub mod nnse;
pub mod paths;
pub mod services;
pub mod sim;
pub mod transport;
#[cfg(feature = "usb")]
pub mod usb;

pub use calculator::{Calculator, Options, check_name, is_junk, is_sendable, local_name};
pub use error::{Error, RemoteKind, Result};
pub use nnse::{LinkConfig, Trace};
pub use services::{DeviceInfo, DirEntry, OsStage, Screenshot};
pub use transport::Transport;
