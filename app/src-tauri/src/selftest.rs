//! `CalcBridge --self-test`: check that a packaged build's Rust side works without opening a window
//! (USB enumeration, the protocol stack against the simulator, documents and math). Prints a report;
//! exits 0 when everything passed. CI runs it on every packaged build.

use std::io::Write;

use calcbridge_link::sim::{Sim, SimConfig};
use calcbridge_link::{Calculator, Options, usb};
use calcbridge_tns::{Paragraph, TnsDocument, build_notes, r2d};

fn check(name: &str, out: &mut Vec<String>, f: impl FnOnce() -> Result<String, String>) -> bool {
    let line = match f() {
        Ok(detail) => format!("ok      {name}: {detail}"),
        Err(e) => format!("FAILED  {name}: {e}"),
    };
    let ok = line.starts_with("ok");
    out.push(line);
    ok
}

pub fn run() -> i32 {
    let mut out = vec![format!("CalcBridge {} self-test", env!("CARGO_PKG_VERSION"))];
    let mut ok = true;
    ok &= check("USB", &mut out, || {
        usb::list().map(|d| format!("{} calculator(s) connected", d.len())).map_err(|e| e.to_string())
    });
    ok &= check("simulated handheld", &mut out, || {
        let sim = Sim::new(SimConfig { idle_drop: None, ..SimConfig::default() });
        let calc =
            Calculator::open(Box::new(sim.transport("sim")), Options { state_file: None, ..Options::default() }, None)
                .map_err(|e| e.to_string())?;
        let info = calc.info().map_err(|e| e.to_string())?;
        calc.write_file("/selftest.tns", b"*TIMLP0500", None, false).map_err(|e| e.to_string())?;
        let files = calc.list_dir("/").map_err(|e| e.to_string())?.len();
        let shot = calc.screenshot().map_err(|e| e.to_string())?;
        let png = shot.png().map_err(|e| e.to_string())?;
        calc.close();
        Ok(format!("\"{}\", {files} items, {}x{} screen, {} byte PNG", info.name, shot.width, shot.height, png.len()))
    });
    ok &= check("documents", &mut out, || {
        let blob = build_notes(&[vec![Paragraph::new(r"$x_{k+1}=x_{k}-\frac{f(x_{k})}{f'(x_{k})}$")]])
            .map_err(|e| e.to_string())?;
        let doc = TnsDocument::from_bytes(&blob).map_err(|e| e.to_string())?;
        let trees = doc.notes_trees();
        let paras = r2d::parse_r2d(&trees.first().ok_or("no Notes page")?.2)?;
        Ok(format!("{} byte document, {} paragraph(s) back", blob.len(), paras.len()))
    });
    out.push(if ok { "self-test passed".into() } else { "self-test FAILED".into() });
    let report = out.join("\n");
    // A windowed release build on Windows has no console, so also leave the report in a file.
    let _ = std::io::stdout().write_all(format!("{report}\n").as_bytes());
    if let Ok(path) = std::env::var("CALCBRIDGE_SELFTEST_OUT") {
        let _ = std::fs::write(path, &report);
    }
    if ok { 0 } else { 1 }
}
