//! Reads real documents if CALCBRIDGE_TI_SAMPLES points at a folder of .tns files (TI-made documents
//! aren't redistributable, so none ship with the repo).

use calcbridge_tns::{TnsDocument, r2d};

#[test]
fn reads_real_documents() {
    let Ok(dir) = std::env::var("CALCBRIDGE_TI_SAMPLES") else {
        eprintln!("CALCBRIDGE_TI_SAMPLES not set; skipping");
        return;
    };
    let mut checked = 0;
    for dir in dir.split(';') {
        for entry in std::fs::read_dir(dir).expect("sample folder") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("tns") {
                continue;
            }
            let blob = std::fs::read(&path).unwrap();
            let doc = TnsDocument::from_bytes(&blob).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(!doc.pages().is_empty(), "{}: no pages", path.display());
            for (p, c, tree) in doc.notes_trees() {
                let paras = r2d::parse_r2d(&tree).unwrap_or_else(|e| panic!("{} {p}.{c}: {e}", path.display()));
                assert!(!paras.is_empty());
            }
            println!("{}: {}", path.display(), doc.summary().lines().count());
            checked += 1;
        }
    }
    assert!(checked > 0);
}
