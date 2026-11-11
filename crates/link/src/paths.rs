//! Per-user locations for CalcBridge's own state (shared with the 1.x Python app, so switching
//! versions keeps the remembered sequence numbers).

use std::path::PathBuf;

fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home).join("CalcBridge")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/CalcBridge")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
            .join("calcbridge")
    }
}

pub fn seq_state_file() -> PathBuf {
    config_dir().join("link-state.json")
}
