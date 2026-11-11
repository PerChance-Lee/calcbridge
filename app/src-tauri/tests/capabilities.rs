//! What the window is allowed to call (capabilities/default.json) against what the page needs.

use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

/// The page listens for close requests (to warn about a running transfer). Tauri then holds every
/// close until the page calls `destroy()`, so without this permission the window can't be closed.
#[test]
fn the_window_can_be_closed() {
    let caps: serde_json::Value = serde_json::from_str(&read("capabilities/default.json")).unwrap();
    let granted: Vec<&str> = caps["permissions"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect();
    if read("../src/App.tsx").contains("onCloseRequested") {
        assert!(granted.contains(&"core:window:allow-destroy"), "{granted:?}");
    }
}
