//! Damaged and hostile documents must give errors, never panics: the Viewer and the CLI open files
//! from anywhere.

use calcbridge_tns::{Paragraph, TnsDocument, build_lua, build_notes, build_python, r2d};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Everything the app does with a document it opens.
fn exercise(blob: &[u8]) {
    let Ok(doc) = TnsDocument::from_bytes(blob) else { return };
    let _ = doc.summary();
    let _ = doc.pages();
    let _ = doc.python_files();
    let _ = doc.lua_script();
    for (_, _, tree) in doc.notes_trees() {
        let _ = r2d::parse_r2d(&tree);
    }
}

fn samples() -> Vec<Vec<u8>> {
    vec![
        build_notes(&[
            vec![Paragraph::title("Title", 0x1F5FD0), Paragraph::new(r"$x_{k+1}=\frac{a}{\sqrt{b}}$ and **bold**")],
            vec![Paragraph::new(r"$\abs{x}\le\pi$")],
        ])
        .unwrap(),
        build_python("prog.py", "print('hi')\nfor i in range(3):\n    print(i)\n", &[]).unwrap(),
        build_lua("function on.paint(gc) gc:drawString('x', 1, 2) end").unwrap(),
    ]
}

#[test]
fn truncated_documents_never_panic() {
    for blob in samples() {
        for cut in (0..blob.len()).step_by(7) {
            exercise(&blob[..cut]);
        }
    }
}

#[test]
fn corrupted_documents_never_panic() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for blob in samples() {
        for _ in 0..1500 {
            let mut bad = blob.clone();
            for _ in 0..1 + rng.below(6) {
                let i = rng.below(bad.len());
                bad[i] = rng.next() as u8;
            }
            exercise(&bad);
        }
    }
}

#[test]
fn garbage_never_panics() {
    let mut rng = Rng(42);
    for _ in 0..500 {
        let n = rng.below(4000);
        let mut junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        if rng.below(2) == 0 && junk.len() > 10 {
            junk[..10].copy_from_slice(b"*TIMLP0500"); // get past the first check
        }
        exercise(&junk);
        let text = String::from_utf8_lossy(&junk);
        let _ = r2d::parse_r2d(&text);
        let _ = r2d::markup_to_view(&text);
    }
}

/// Below the encryption: damaged TIXC token streams and damaged Notes trees.
#[test]
fn damaged_inner_layers_never_panic() {
    use calcbridge_tns::tixc;
    let doc = TnsDocument::from_bytes(&samples()[0]).unwrap();
    let mut rng = Rng(7);
    for entry in &doc.entries {
        let Ok(tokens) = tixc::encode(&entry.text()) else { continue };
        for _ in 0..1500 {
            let mut bad = tokens.clone();
            for _ in 0..1 + rng.below(4) {
                let i = rng.below(bad.len());
                bad[i] = rng.next() as u8;
            }
            if rng.below(4) == 0 {
                bad.truncate(rng.below(bad.len()));
            }
            let _ = tixc::decode(&bad);
        }
    }
    for (_, _, tree) in doc.notes_trees() {
        let chars: Vec<char> = tree.chars().collect();
        for _ in 0..1500 {
            let mut bad = chars.clone();
            for _ in 0..1 + rng.below(4) {
                let i = rng.below(bad.len());
                bad[i] = ['<', '>', '"', '/', 'x', '0', '(', ')', '√', '&'][rng.below(10)];
            }
            let _ = r2d::parse_r2d(&bad.iter().collect::<String>());
        }
    }
}

/// Lua scripts are CDATA: non-ASCII text and a script ending in "]" must come back as written.
/// (The handheld reads CDATA as plain UTF-8, verified on a CX II.)
#[test]
fn lua_scripts_round_trip() {
    for script in ["gc:drawString(\"pi=π e=é ≤\", 10, 30)", "x = t[1]", "y = a[b[1]]", "s = \"]]\" .. \"]\"", "-- ]] ]"]
    {
        let doc = TnsDocument::from_bytes(&build_lua(script).unwrap()).unwrap();
        let back = doc.lua_script().unwrap_or_else(|| panic!("no script back for {script:?}"));
        assert!(back.contains(script), "{script:?} came back as {back:?}");
    }
}
