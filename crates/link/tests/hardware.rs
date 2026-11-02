//! End-to-end checks against a real TI-Nspire CX II. Opt in with
//!     CALCBRIDGE_HARDWARE=1 cargo test -p calcbridge-link --test hardware -- --nocapture
//!
//! Everything happens inside a scratch folder (/calcbridge-selftest-<time>) that is removed
//! afterwards; nothing else on the handheld is written to. OS install is never exercised here, and
//! no keys are pressed.

#![cfg(feature = "usb")]

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use calcbridge_link::{Calculator, Error, Options, usb};

fn noise(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn step<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    println!("ok  {name} ({} ms)", t.elapsed().as_millis());
    out
}

#[test]
fn hardware_end_to_end() {
    if std::env::var_os("CALCBRIDGE_HARDWARE").is_none() {
        eprintln!("CALCBRIDGE_HARDWARE not set; skipping");
        return;
    }
    let devices = usb::list().expect("list");
    println!("devices: {devices:?}");
    assert!(!devices.is_empty(), "no CX II connected");
    let calc = step("open", || {
        Calculator::open_usb(None, Options::default(), Some(&mut |a, n| println!("   probing {a}/{n}"))).expect("open")
    });
    println!("    device key {}", calc.device_key());
    for leftover in
        calc.list_dir("/").unwrap().into_iter().filter(|e| e.is_dir && e.name.starts_with("calcbridge-selftest-"))
    {
        println!("    removing {} left by an interrupted run", leftover.path);
        calc.delete(&leftover.path).unwrap();
    }
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let root = format!("/calcbridge-selftest-{stamp}");
    calc.mkdir(&root, false).expect("scratch folder");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&calc, &root)));
    if calc.exists(&root).unwrap_or(false) {
        calc.delete(&root).expect("remove the scratch folder");
    }
    assert!(!calc.exists(&root).unwrap(), "scratch folder left behind");
    calc.close();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn run(calc: &Calculator, root: &str) {
    let info = step("info", || calc.info().unwrap());
    println!(
        "    {} \"{}\" OS {} battery {} free {}/{}",
        info.model(),
        info.name,
        info.os_version,
        info.battery,
        info.flash_free,
        info.flash_total
    );
    assert!(!info.name.is_empty() && info.os_version.matches('.').count() == 2);
    assert!(0 < info.flash_free && info.flash_free <= info.flash_total);
    assert_eq!((info.lcd_width, info.lcd_height), (320, 240));
    assert_eq!(info.file_extension, ".tns");

    let names: Vec<String> = step("list /", || calc.list_dir("/").unwrap()).into_iter().map(|e| e.name).collect();
    assert!(names.iter().any(|n| *n == root[1..]), "{names:?}");
    assert!(names.iter().any(|n| n == "MyLib"));

    for size in [1, 1438, 1439, 1440, 1460, 5000, 100_000] {
        let data = noise(size, size as u64);
        let path = format!("{root}/rt{size}.tns");
        let t = Instant::now();
        calc.write_file(&path, &data, None, false).unwrap();
        let w = t.elapsed();
        assert_eq!(calc.stat(&path).unwrap().unwrap().size, size as u64);
        let t = Instant::now();
        assert_eq!(calc.read_file(&path, None).unwrap(), data, "size {size}");
        println!("ok  roundtrip {size} bytes (write {} ms, read {} ms)", w.as_millis(), t.elapsed().as_millis());
    }

    step("zero-byte file", || match calc.write_file(&format!("{root}/empty.tns"), b"", None, false) {
        Ok(()) => assert_eq!(calc.read_file(&format!("{root}/empty.tns"), None).unwrap(), b""),
        Err(e) => println!("    this OS refuses empty files: {e}"),
    });

    step("overwrite and missing", || {
        let p = format!("{root}/o.tns");
        calc.write_file(&p, b"one", None, false).unwrap();
        assert!(calc.write_file(&p, b"two", None, false).unwrap_err().is_exists());
        calc.write_file(&p, b"two!", None, true).unwrap();
        assert_eq!(calc.read_file(&p, None).unwrap(), b"two!");
        assert!(calc.read_file(&format!("{root}/missing.tns"), None).unwrap_err().is_not_found());
        assert!(calc.stat(&format!("{root}/missing.tns")).unwrap().is_none());
    });

    step("refuses non-.tns uploads", || {
        let err = calc.with_session(|s| calcbridge_link::services::write_file(s, &format!("{root}/x.txt"), b"x", None));
        assert!(matches!(&err, Err(Error::Remote { code: Some(0xFF15), .. })), "{err:?}");
    });

    step("folders: copy, move, rename", || {
        calc.mkdir(&format!("{root}/A/B"), true).unwrap();
        calc.write_file(&format!("{root}/A/B/f.tns"), &[b'f'; 3000], None, false).unwrap();
        calc.write_file(&format!("{root}/A/g.tns"), b"g", None, false).unwrap();
        calc.copy(&format!("{root}/A/g.tns"), &format!("{root}/A/g copy.tns")).unwrap();
        assert_eq!(calc.read_file(&format!("{root}/A/g copy.tns"), None).unwrap(), b"g");
        calc.copy(&format!("{root}/A"), &format!("{root}/A2")).unwrap();
        assert_eq!(calc.read_file(&format!("{root}/A2/B/f.tns"), None).unwrap(), [b'f'; 3000]);
        assert_eq!(calc.rename(&format!("{root}/A2/g.tns"), "h.tns").unwrap(), format!("{root}/A2/h.tns"));
        calc.move_to(&format!("{root}/A2/h.tns"), &format!("{root}/A/B/h.tns")).unwrap();
        let mut names: Vec<String> =
            calc.list_dir(&format!("{root}/A/B")).unwrap().into_iter().map(|e| e.name).collect();
        names.sort();
        assert_eq!(names, ["f.tns", "h.tns"]);
        calc.delete(&format!("{root}/A2")).unwrap();
        assert!(calc.stat(&format!("{root}/A2")).unwrap().is_none());
    });

    step("names with spaces and unicode", || {
        for name in ["with space.tns", "Ünïcødé ε.tns"] {
            calc.write_file(&format!("{root}/{name}"), b"n", None, false).unwrap();
            assert!(calc.list_dir(root).unwrap().iter().any(|e| e.name == name), "{name}");
        }
    });

    step("survives idle", || {
        calc.info().unwrap();
        std::thread::sleep(Duration::from_millis(2500)); // the calc drops the first packet after ~1.5 s idle
        calc.list_dir(root).unwrap();
        std::thread::sleep(Duration::from_millis(2500));
        assert_eq!(calc.screenshot().unwrap().width, 320);
    });

    let shot = step("screenshot", || calc.screenshot().unwrap());
    assert_eq!((shot.width, shot.height, shot.bpp), (320, 240, 16));
    assert_eq!(shot.pixels.len(), 320 * 240 * 2);
    let t = Instant::now();
    for _ in 0..5 {
        calc.screenshot().unwrap();
    }
    println!("    {:.1} screenshots/s", 5.0 / t.elapsed().as_secs_f64());

    let tmp = tempfile::tempdir().unwrap();
    step("upload and download a tree", || {
        let src = tmp.path().join("tree");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.tns"), noise(2000, 1)).unwrap();
        std::fs::write(src.join("sub").join("b.tns"), noise(7000, 2)).unwrap();
        calc.upload(&src, root, None, false, None, None).unwrap();
        let out = tmp.path().join("back");
        calc.download(&format!("{root}/tree"), &out, None).unwrap();
        assert_eq!(std::fs::read(out.join("a.tns")).unwrap(), noise(2000, 1));
        assert_eq!(std::fs::read(out.join("sub").join("b.tns")).unwrap(), noise(7000, 2));
    });

    step("backup everything (read-only on the handheld)", || {
        let bk = tmp.path().join("backup");
        let n = calc.backup(&bk, None).unwrap();
        println!("    backed up {n} files");
        assert!(n >= 1);
        let files: Vec<_> =
            calc.walk("/").unwrap().into_iter().filter(|e| !e.is_dir && !e.path.starts_with(root)).collect();
        let sample = &files[0];
        let local = calcbridge_link::calculator::local_path(&bk, &sample.path);
        assert_eq!(std::fs::read(&local).unwrap(), calc.read_file(&sample.path, None).unwrap());
    });

    // Restore our own files, not the user's: copies of real documents (MyLib libraries especially)
    // make the handheld show a "Document Deleted" notice when the scratch folder is removed.
    step("restore a backup folder into the scratch area", || {
        let src = tmp.path().join("restore-src");
        std::fs::create_dir_all(src.join("inner")).unwrap();
        std::fs::write(src.join("one.tns"), noise(3000, 5)).unwrap();
        std::fs::write(src.join("inner").join("two.tns"), noise(300, 6)).unwrap();
        std::fs::write(src.join("notes.txt"), b"not a document").unwrap();
        let mut skipped = Vec::new();
        let restored = calc.restore(&src, &format!("{root}/restored"), false, None, Some(&mut skipped)).unwrap();
        assert_eq!(restored, 2);
        assert_eq!(skipped.len(), 1);
        assert_eq!(calc.read_file(&format!("{root}/restored/inner/two.tns"), None).unwrap(), noise(300, 6));
    });
}
