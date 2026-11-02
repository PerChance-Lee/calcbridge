//! Protocol and calculator behaviour against the simulator (ported from the 1.x Python suite).

use std::path::Path;
use std::time::Duration;

use calcbridge_link::navnet::{self, Packet};
use calcbridge_link::nnse::{self, Message};
use calcbridge_link::services::{self, rle_decode, rle_encode};
use calcbridge_link::sim::{Sim, SimConfig};
use calcbridge_link::transport::Transport;
use calcbridge_link::{Calculator, Error, LinkConfig, Options, OsStage, RemoteKind};

fn fast_link() -> LinkConfig {
    // keep retry timeouts short; the simulator answers instantly
    LinkConfig {
        ack_timeout: Duration::from_millis(200),
        probe_timeout: Duration::from_millis(40),
        ..LinkConfig::default()
    }
}

fn opts(state: &Path) -> Options {
    Options { state_file: Some(state.join("state.json")), trace: None, link: fast_link() }
}

fn quiet_sim() -> Sim {
    Sim::new(SimConfig { idle_drop: None, ..SimConfig::default() })
}

fn open(sim: &Sim, dir: &Path) -> Calculator {
    Calculator::open(Box::new(sim.transport("sim")), opts(dir), None).expect("open")
}

fn setup() -> (Sim, tempfile::TempDir, Calculator) {
    let sim = quiet_sim();
    let dir = tempfile::tempdir().unwrap();
    let calc = open(&sim, dir.path());
    (sim, dir, calc)
}

/// Deterministic bytes for payloads.
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

// ------------------------------------------------------------ encodings
#[test]
fn navnet_address_packet_matches_real_hardware_capture() {
    // Logged from a real CX II: the host's first NavNet packet (address assignment).
    let pkt = Packet::new(0x4003, 0x4003, [0x64, 0x01, 0xFF, 0x00]).encode().unwrap();
    assert_eq!(hex(&pkt), "54fd64004003640140031343040000fa6401ff00");
}

#[test]
fn navnet_device_info_request_matches_capture() {
    let pkt = Packet::new(0x8000, 0x4020, [0x01]).encode().unwrap();
    assert_eq!(hex(&pkt), "54fd64008000640140200100010000fc01");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn navnet_roundtrip_small_and_big() {
    for n in [0, 1, 254, 255, 1000, 1440] {
        let data = noise(n, n as u64);
        let p = Packet::parse(&Packet::new(0x8001, 0x4060, data.clone()).encode().unwrap()).unwrap();
        assert_eq!((p.data, p.src_sid, p.dst_sid), (data, 0x8001, 0x4060));
    }
    assert!(Packet::new(1, 2, vec![0; 1441]).encode().is_err());
}

#[test]
fn navnet_rejects_corruption() {
    let mut pkt = Packet::new(0x8000, 0x4020, *b"hello").encode().unwrap();
    *pkt.last_mut().unwrap() ^= 1;
    assert!(Packet::parse(&pkt).is_err());
}

#[test]
fn nnse_checksum_and_roundtrip() {
    let msg = Message::new(nnse::SVC_STREAM, *b"abc").ack(1).seq(513).encode();
    assert_eq!(nnse::checksum(&msg), 0xFFFF);
    let m = Message::decode(&msg).unwrap();
    assert_eq!((m.seqno, m.data.as_slice(), m.req_ack), (513, &b"abc"[..], 1));
    let mut bad = msg.clone();
    *bad.last_mut().unwrap() ^= 0xFF;
    assert!(Message::decode(&bad).is_none());
}

#[test]
fn rle_roundtrip() {
    for seed in 0..5u64 {
        let mut px = Vec::new();
        let mut r = noise(4096, seed).into_iter().cycle();
        let mut next = || r.next().unwrap();
        while px.len() < 320 * 240 * 2 {
            let first = match next() % 3 {
                0 => 0u16,
                1 => 0xFFFF,
                _ => u16::from_le_bytes([next(), next()]),
            };
            let unit = [first.to_le_bytes(), [next(), next()]].concat();
            let reps = [1, 1, 2, 50, 200][usize::from(next()) % 5];
            for _ in 0..reps {
                px.extend_from_slice(&unit);
            }
        }
        px.truncate(320 * 240 * 2);
        assert_eq!(rle_decode(16, &rle_encode(16, &px), px.len()), px);
    }
}

// ------------------------------------------------------- link behaviour
#[test]
fn fresh_link_handshake_and_info() {
    let (sim, _dir, calc) = setup();
    let info = calc.info().unwrap();
    assert_eq!(info.name, "Simulated CX II");
    assert_eq!(info.os_version, "5.40.259");
    assert_eq!((info.lcd_width, info.flash_total), (320, sim.flash_total()));
    assert_eq!((info.file_extension.as_str(), info.os_extension.as_str()), (".tns", ".tco2"));
    assert_eq!(info.run_level, "OS");
    assert_eq!(info.model(), "TI-Nspire CX II");
}

#[test]
fn rediscovers_sequence_on_silent_link() {
    let sim = Sim::new(SimConfig { fresh_link: false, start_seq: 37, idle_drop: None, ..SimConfig::default() });
    let dir = tempfile::tempdir().unwrap();
    let calc = open(&sim, dir.path());
    assert!(!calc.list_dir("/").unwrap().is_empty());
    calc.close();
    let first_run = sim.stat("out_of_order");
    assert!(first_run >= 36, "probed up from 0: {first_run}");
    // the second run starts just behind the saved number instead of probing from zero
    let calc = open(&sim, dir.path());
    assert!(sim.stat("out_of_order") - first_run <= 4);
    let before = sim.stat("out_of_order");
    assert!(!calc.info().unwrap().name.is_empty());
    calc.close();
    assert_eq!(sim.stat("out_of_order"), before);
}

#[test]
fn survives_idle_drop() {
    let sim = Sim::new(SimConfig { idle_drop: Some(Duration::from_millis(300)), ..SimConfig::default() });
    let dir = tempfile::tempdir().unwrap();
    let calc = open(&sim, dir.path());
    calc.info().unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(!calc.list_dir("/").unwrap().is_empty());
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(calc.screenshot().unwrap().width, 320);
    assert!(sim.stat("idle_drops") >= 2);
}

#[test]
fn survives_random_packet_loss() {
    let sim = Sim::new(SimConfig { idle_drop: None, drop_rate: 0.15, seed: 7, ..SimConfig::default() });
    let dir = tempfile::tempdir().unwrap();
    let calc = open(&sim, dir.path());
    let data = noise(20000, 3);
    calc.write_file("/lossy.tns", &data, None, false).unwrap();
    assert_eq!(calc.read_file("/lossy.tns", None).unwrap(), data);
    assert!(sim.stat("random_drops") > 0);
}

#[test]
fn reconnects_after_replug() {
    let (sim, _dir, calc) = setup();
    calc.info().unwrap();
    sim.replug();
    // the next call notices the new handshake, reopens the session and retries
    assert!(!calc.list_dir("/").unwrap().is_empty());
}

#[test]
fn stray_packet_for_old_session_is_refused_once() {
    // A leftover packet for a session that already ended must be nacked, not loop or break the next call.
    let (sim, _dir, calc) = setup();
    calc.info().unwrap();
    let stray = Packet {
        src_addr: navnet::CALC_ADDR,
        dst_addr: navnet::HOST_ADDR,
        ..Packet::new(0x4024, 0x8000, *b"\x00leftover")
    };
    sim.send_navnet(stray.encode().unwrap());
    let before = sim.stat("request_4024");
    assert!(!calc.list_dir("/").unwrap().is_empty());
    assert_eq!(sim.stat("request_4024"), before);
}

#[test]
fn unplugged_raises() {
    let (sim, _dir, calc) = setup();
    calc.info().unwrap();
    sim.unplug();
    assert!(matches!(calc.info(), Err(Error::NoDevice(_))));
}

// --------------------------------------------------------------- files
#[test]
fn list_root() {
    let (_sim, _dir, calc) = setup();
    let entries = calc.list_dir("/").unwrap();
    let get = |n: &str| entries.iter().find(|e| e.name == n).unwrap_or_else(|| panic!("{n} missing"));
    for n in ["MyLib", "MyWidgets", "PyLib", "Examples", "themes.csv"] {
        get(n);
    }
    assert!(get("MyLib").is_dir && !get("themes.csv").is_dir);
    assert_eq!(get("themes.csv").path, "/themes.csv");
}

#[test]
fn write_read_roundtrip() {
    let (_sim, _dir, calc) = setup();
    for size in [0, 1, 1438, 1439, 1440, 2878, 5000, 150_000] {
        let data = noise(size, size as u64);
        let path = format!("/rt{size}.tns");
        calc.write_file(&path, &data, None, false).unwrap();
        assert_eq!(calc.read_file(&path, None).unwrap(), data, "size {size}");
        assert_eq!(calc.stat(&path).unwrap().unwrap().size, size as u64);
    }
}

#[test]
fn progress_reports_reach_total() {
    let (_sim, _dir, calc) = setup();
    let data = noise(10000, 1);
    let mut seen = Vec::new();
    calc.write_file("/p.tns", &data, Some(&mut |d, t| seen.push((d, t))), false).unwrap();
    assert_eq!(seen.last(), Some(&(10000, 10000)));
    seen.clear();
    calc.read_file("/p.tns", Some(&mut |d, t| seen.push((d, t)))).unwrap();
    assert_eq!(seen.last(), Some(&(10000, 10000)));
}

#[test]
fn overwrite_protection() {
    let (_sim, _dir, calc) = setup();
    calc.write_file("/a.tns", b"one", None, false).unwrap();
    assert!(calc.write_file("/a.tns", b"two", None, false).unwrap_err().is_exists());
    calc.write_file("/a.tns", b"two", None, true).unwrap();
    assert_eq!(calc.read_file("/a.tns", None).unwrap(), b"two");
}

#[test]
fn missing_file_errors() {
    let (_sim, _dir, calc) = setup();
    assert!(calc.read_file("/nope.tns", None).unwrap_err().is_not_found());
    assert!(calc.list_dir("/nope").unwrap_err().is_not_found());
    assert!(calc.delete("/nope.tns").unwrap_err().is_not_found());
    assert!(calc.stat("/nope.tns").unwrap().is_none());
}

#[test]
fn folders_rename_move_copy_delete() {
    let (_sim, _dir, calc) = setup();
    calc.mkdir("/T/sub/deep", true).unwrap();
    calc.write_file("/T/sub/deep/x.tns", b"x", None, false).unwrap();
    calc.write_file("/T/y.tns", b"y", None, false).unwrap();
    assert_eq!(calc.rename("/T/y.tns", "z.tns").unwrap(), "/T/z.tns");
    calc.move_to("/T/z.tns", "/T/sub/z.tns").unwrap();
    calc.copy("/T/sub", "/T/sub2").unwrap();
    assert_eq!(calc.read_file("/T/sub2/deep/x.tns", None).unwrap(), b"x");
    assert_eq!(calc.read_file("/T/sub2/z.tns", None).unwrap(), b"y");
    assert!(calc.copy("/T/sub", "/T/sub2").unwrap_err().is_exists());
    calc.delete("/T").unwrap();
    assert!(calc.stat("/T").unwrap().is_none());
}

#[test]
fn removing_a_full_folder_directly_is_refused() {
    let (_sim, _dir, calc) = setup();
    calc.mkdir("/F", false).unwrap();
    calc.write_file("/F/a.tns", b"a", None, false).unwrap();
    let err = calc.with_session(|s| services::remove_dir(s, "/F")).unwrap_err();
    assert!(matches!(err, Error::Remote { kind: RemoteKind::Refused, .. }), "{err}");
    calc.delete("/F").unwrap(); // Calculator::delete empties the folder first
    assert!(!calc.exists("/F").unwrap());
}

#[test]
fn walk_lists_folders_before_contents() {
    let (_sim, _dir, calc) = setup();
    calc.mkdir("/W/a", true).unwrap();
    calc.write_file("/W/a/f.tns", b"1", None, false).unwrap();
    let paths: Vec<String> = calc.walk("/W").unwrap().into_iter().map(|e| e.path).collect();
    assert_eq!(paths, ["/W/a", "/W/a/f.tns"]);
}

#[test]
fn bad_names_rejected() {
    let (_sim, _dir, calc) = setup();
    for path in ["/q/b?.tns", "/a:b.tns", "/", "/x?.tns", "/tab\there.tns"] {
        assert!(matches!(calc.write_file(path, b"", None, false), Err(Error::Invalid(_))), "{path}");
    }
}

#[test]
fn upload_download_trees() {
    let (_sim, dir, calc) = setup();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("inner")).unwrap();
    std::fs::write(src.join("one.tns"), vec![b'1'; 3000]).unwrap();
    std::fs::write(src.join("inner").join("two.tns"), vec![b'2'; 10]).unwrap();
    assert_eq!(calc.upload(&src, "/", None, false, None, None).unwrap(), "/src");
    let out = dir.path().join("out");
    assert_eq!(calc.download("/src", &out, None).unwrap(), 3010);
    assert_eq!(std::fs::read(out.join("inner").join("two.tns")).unwrap(), vec![b'2'; 10]);
}

#[test]
fn backup_and_restore() {
    let (_sim, dir, calc) = setup();
    calc.mkdir("/B/c", true).unwrap();
    calc.write_file("/B/c/doc.tns", b"doc", None, false).unwrap();
    let bk = dir.path().join("bk");
    let files = calc.backup(&bk, None).unwrap();
    assert!(files >= 3);
    assert_eq!(std::fs::read(bk.join("B").join("c").join("doc.tns")).unwrap(), b"doc");
    calc.delete("/B").unwrap();
    // only the missing .tns comes back: themes.csv isn't a document and Getting Started is still there
    assert_eq!(calc.restore(&bk, "/", false, None, None).unwrap(), 1);
    assert_eq!(calc.read_file("/B/c/doc.tns", None).unwrap(), b"doc");
}

#[test]
fn cancel_between_items() {
    let (_sim, dir, calc) = setup();
    for i in 0..5 {
        calc.write_file(&format!("/c{i}.tns"), b"x", None, false).unwrap();
    }
    calc.cancel();
    assert!(matches!(calc.backup(&dir.path().join("bk"), None), Err(Error::Cancelled)));
}

#[test]
fn screenshot() {
    let (_sim, _dir, calc) = setup();
    let s = calc.screenshot().unwrap();
    assert_eq!((s.width, s.height, s.bpp), (320, 240, 16));
    assert_eq!(s.pixels.len(), 320 * 240 * 2);
    assert_eq!(s.to_rgba().len(), 320 * 240 * 4);
}

#[test]
fn os_install_on_simulator() {
    let (sim, _dir, calc) = setup();
    let mut image = b"TI-Nspire.tco2".to_vec();
    image.extend(noise(5000, 9));
    let mut stages = Vec::new();
    calc.install_os(&image, Some(&mut |stage, _, _| stages.push(stage))).unwrap();
    assert_eq!(sim.os_installed(), Some(image));
    assert!(stages.contains(&OsStage::Upload) && stages.contains(&OsStage::Install));
}

#[test]
fn os_install_rejects_tiny_file() {
    let (_sim, _dir, calc) = setup();
    assert!(calc.install_os(b"short", None).is_err());
}

#[test]
fn os_install_rejects_a_bad_image() {
    let (sim, _dir, calc) = setup();
    let err = calc.install_os(&noise(5000, 4), None).unwrap_err();
    assert!(err.to_string().contains("damaged"), "{err}");
    assert!(sim.os_installed().is_none());
}

#[test]
fn link_error_when_calc_silent() {
    let sim = Sim::new(SimConfig { fresh_link: false, start_seq: 500, idle_drop: None, ..SimConfig::default() });
    let dir = tempfile::tempdir().unwrap();
    let mut o = opts(dir.path());
    o.link.probe_limit = 3;
    assert!(matches!(Calculator::open(Box::new(sim.transport("sim")), o, None), Err(Error::Link(_))));
}

#[test]
fn keys_reach_the_calculator() {
    let (sim, _dir, calc) = setup();
    calc.press("enter").unwrap();
    calc.type_text("a1").unwrap();
    assert_eq!(sim.keys_pressed(), [0x0D1000, 0x616600, 0x317300]);
    assert!(calc.type_text("€").is_err());
}

// ---------------------------------------------- regressions from the 1.x review
struct FlakyTransport {
    inner: Box<dyn Transport>,
    n: u32,
    fail_on: Vec<u32>,
}

impl Transport for FlakyTransport {
    fn write(&mut self, data: &[u8], timeout: Duration) -> calcbridge_link::Result<()> {
        self.n += 1;
        if self.fail_on.contains(&self.n) {
            return Err(Error::Link("USB write timed out".into()));
        }
        self.inner.write(data, timeout)
    }

    fn read(&mut self, max: usize, timeout: Duration) -> calcbridge_link::Result<Option<Vec<u8>>> {
        self.inner.read(max, timeout)
    }

    fn device_key(&self) -> String {
        self.inner.device_key()
    }
}

// a link reset while waiting for a reply used to wedge the connection
#[test]
fn reset_while_waiting_for_reply() {
    let (sim, _dir, calc) = setup();
    calc.info().unwrap();
    sim.replug_after_next_ack();
    assert!(!calc.list_dir("/").unwrap().is_empty()); // notices the reset, re-handshakes, retries
    assert!(!calc.info().unwrap().name.is_empty()); // and the link keeps working afterwards
    assert!(!calc.list_dir("/Examples").unwrap().is_empty());
}

// one USB write timeout used to leave the host a sequence number ahead for good
#[test]
fn write_timeout_does_not_desync() {
    let sim = quiet_sim();
    let dir = tempfile::tempdir().unwrap();
    let flaky = FlakyTransport { inner: Box::new(sim.transport("sim")), n: 0, fail_on: vec![12, 30] };
    let calc = Calculator::open(Box::new(flaky), opts(dir.path()), None).unwrap();
    for _ in 0..4 {
        assert!(!calc.list_dir("/").unwrap().is_empty());
    }
    calc.close();
    let calc = open(&sim, dir.path());
    assert!(!calc.info().unwrap().name.is_empty()); // the saved sequence number is right too
}

// copying a folder into itself recursed until the path limit
#[test]
fn copy_or_move_folder_into_itself_is_refused() {
    let (_sim, _dir, calc) = setup();
    calc.mkdir("/A", false).unwrap();
    calc.write_file("/A/x.tns", b"x", None, false).unwrap();
    assert!(matches!(calc.copy("/A", "/A/A"), Err(Error::Invalid(_))));
    assert!(matches!(calc.move_to("/A", "/A/A"), Err(Error::Invalid(_))));
    let names: Vec<String> = calc.list_dir("/A").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["x.tns"]);
}

// a stale Cancel used to abort the next job
#[test]
fn cancel_is_one_shot() {
    let (_sim, _dir, calc) = setup();
    calc.cancel();
    assert!(matches!(calc.check_cancel(), Err(Error::Cancelled)));
    calc.check_cancel().unwrap(); // cleared
}

// folder upload / restore sent files the handheld refuses
#[test]
fn upload_folder_sends_only_tns() {
    let (_sim, dir, calc) = setup();
    let d = dir.path().join("pack");
    std::fs::create_dir(&d).unwrap();
    std::fs::write(d.join("a.tns"), b"a").unwrap();
    std::fs::write(d.join("notes.txt"), b"n").unwrap();
    std::fs::write(d.join("desktop.ini"), b"[x]").unwrap();
    let mut skipped = Vec::new();
    calc.upload(&d, "/", None, false, None, Some(&mut skipped)).unwrap();
    let names: Vec<String> = calc.list_dir("/pack").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["a.tns"]);
    let skipped: Vec<String> = skipped.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(skipped, ["notes.txt"]); // OS clutter isn't reported
    assert!(matches!(calc.upload(&d.join("notes.txt"), "/", None, false, None, None), Err(Error::Invalid(_))));
}

#[test]
fn simulator_refuses_non_tns_like_the_handheld() {
    let (_sim, _dir, calc) = setup();
    let err = calc.write_file("/x.txt", b"x", None, false).unwrap_err();
    assert!(err.to_string().contains(".tns"), "{err}");
}

#[test]
fn restore_counts_only_written() {
    let (_sim, dir, calc) = setup();
    let bk = dir.path().join("bk");
    assert!(calc.backup(&bk, None).unwrap() >= 2);
    let mut skipped = Vec::new();
    assert_eq!(calc.restore(&bk, "/", false, None, Some(&mut skipped)).unwrap(), 0); // all already there
    assert!(skipped.iter().any(|p| p.ends_with("themes.csv")));
    calc.delete("/Examples").unwrap();
    assert_eq!(calc.restore(&bk, "/", false, None, None).unwrap(), 1);
}

// top-level download names weren't sanitised
#[test]
fn local_names() {
    use calcbridge_link::local_name;
    assert_eq!(local_name("a:b.tns"), "a_b.tns");
    assert_eq!(local_name("nul.tns"), "_nul.tns");
    assert_eq!(local_name("x?.tns"), "x_.tns");
    assert_eq!(local_name("dots..."), "dots");
}

// probing only searched upward from the saved number
#[test]
fn probe_finds_sequence_below_saved_number() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("state.json"), r#"{"sim": 900}"#).unwrap();
    let sim = Sim::new(SimConfig { fresh_link: false, start_seq: 20, idle_drop: None, ..SimConfig::default() });
    let calc = open(&sim, dir.path());
    assert!(!calc.info().unwrap().name.is_empty());
}

// the state file is shared with CalcBridge 1.x, which writes it with spaces
#[test]
fn reads_the_python_state_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("state.json"), r#"{"default": 4252, "sim": 41}"#).unwrap();
    let sim = Sim::new(SimConfig { fresh_link: false, start_seq: 41, idle_drop: None, ..SimConfig::default() });
    let calc = open(&sim, dir.path());
    // probing starts at 41 - 4: 37 (sent twice), 38, 39 are dropped; 40 is acked as a duplicate
    assert_eq!(sim.stat("out_of_order"), 4);
    calc.close();
    let saved = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(v["default"], 4252); // other devices' numbers are kept
    assert_eq!(v["sim"], 41);
}

// deleting "/" would empty the whole handheld
#[test]
fn the_top_folder_cannot_be_deleted() {
    let (_sim, _dir, calc) = setup();
    for root in ["/", "", "//"] {
        assert!(matches!(calc.delete(root), Err(Error::Invalid(_))), "{root:?}");
    }
    assert!(calc.exists("/Examples").unwrap());
}
