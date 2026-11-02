//! Diagnostic: read one file from the handheld with a timestamped packet trace.
//!     cargo run -p calcbridge-link --example trace_read -- /path/on/handheld.tns

use std::sync::Arc;
use std::time::Instant;

use calcbridge_link::{Calculator, Options};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "/Examples".into());
    let t0 = Instant::now();
    let opts = Options {
        trace: Some(Arc::new(move |line: &str| println!("{:>8.1} ms {line}", t0.elapsed().as_secs_f64() * 1000.0))),
        ..Options::default()
    };
    let calc = Calculator::open_usb(None, opts, None).expect("open");
    let entry = calc.stat(&path).expect("stat");
    println!("stat: {entry:?}");
    if let Some(e) = entry.filter(|e| !e.is_dir) {
        let data = calc.read_file(&e.path, None).expect("read");
        println!("read {} bytes", data.len());
    }
    calc.close();
}
