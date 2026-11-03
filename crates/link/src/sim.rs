//! Device side of NNSE + NavNet with an in-memory file system.
//!
//! It reproduces the behaviour seen on a real CX II (OS 5.4): the one-time address/time handshake,
//! the "only last+1" sequence rule with duplicate re-acks, dropping the first packet after ~1.5 s
//! idle, and the reply formats of each service. Open it with
//! `Calculator::open(Box::new(sim.transport("sim")), ...)`.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::navnet::{self, Packet};
use crate::nnse::{self, Message};
use crate::services::{padded, rle_encode, scan_padded};
use crate::transport::Transport;
use crate::{Error, Result};

const OK: [u8; 2] = [0xFF, 0x00];
const MISSING: [u8; 2] = [0xFF, 0x0A];

#[derive(Debug, Clone)]
pub struct SimConfig {
    /// A freshly plugged-in calculator does the address/time handshake; otherwise the link is
    /// silent and the host has to find the sequence number.
    pub fresh_link: bool,
    /// The next host sequence number accepted on a silent link.
    pub start_seq: u16,
    /// Drop the first stream packet after this much idle time, like the real calculator.
    pub idle_drop: Option<Duration>,
    /// Drop this fraction of stream packets at random.
    pub drop_rate: f64,
    pub name: String,
    pub seed: u64,
    pub os_version: [u8; 4],
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            fresh_link: true,
            start_seq: 0,
            idle_drop: Some(Duration::from_millis(1500)),
            drop_rate: 0.0,
            name: "Simulated CX II".into(),
            seed: 1,
            os_version: [5, 40, 1, 3],
        }
    }
}

enum Node {
    Dir(BTreeMap<String, Node>),
    File(Vec<u8>),
}

impl Node {
    fn dir() -> Node {
        Node::Dir(BTreeMap::new())
    }

    fn is_dir(&self) -> bool {
        matches!(self, Node::Dir(_))
    }

    fn size(&self) -> usize {
        match self {
            Node::File(d) => d.len(),
            Node::Dir(_) => 0,
        }
    }

    fn used(&self) -> usize {
        match self {
            Node::File(d) => d.len(),
            Node::Dir(c) => c.values().map(Node::used).sum(),
        }
    }

    fn deep_clone(&self) -> Node {
        match self {
            Node::File(d) => Node::File(d.clone()),
            Node::Dir(c) => Node::Dir(c.iter().map(|(k, v)| (k.clone(), v.deep_clone())).collect()),
        }
    }
}

fn parts(path: &str) -> Vec<&str> {
    path.split('/').filter(|p| !p.is_empty()).collect()
}

/// What a host service connection is in the middle of.
enum Mode {
    Idle,
    List(VecDeque<(String, bool, usize)>),
    Read(Vec<u8>),
    Write { path: String, size: usize, buf: Vec<u8> },
    Os { size: usize, buf: Vec<u8>, first: bool },
}

struct State {
    cfg: SimConfig,
    fresh_link: bool,
    handshake_done: bool,
    /// The next host sequence number we accept.
    expected: u16,
    rng: u64,
    calc_seq: u16,
    last_rx: Instant,
    out: VecDeque<Vec<u8>>,
    root: Node,
    sessions: HashMap<u16, Mode>,
    flash_total: u64,
    os_installed: Option<Vec<u8>>,
    plugged: bool,
    screen: Vec<u8>,
    rle_cache: Option<Vec<u8>>,
    stats: HashMap<String, u64>,
    keys_pressed: Vec<u32>,
    replug_after_ack: bool,
    refused_reads: std::collections::HashSet<String>,
}

/// A simulated CX II. Clones share the same calculator.
#[derive(Clone)]
pub struct Sim(Arc<Mutex<State>>);

impl Default for Sim {
    fn default() -> Self {
        Sim::new(SimConfig::default())
    }
}

impl Sim {
    pub fn new(cfg: SimConfig) -> Sim {
        let mut root = BTreeMap::new();
        for folder in ["MyLib", "MyWidgets", "PyLib", "Examples"] {
            root.insert(folder.to_owned(), Node::dir());
        }
        let mut st = State {
            fresh_link: cfg.fresh_link,
            handshake_done: !cfg.fresh_link,
            expected: cfg.start_seq,
            rng: cfg.seed,
            calc_seq: 1000,
            last_rx: Instant::now(),
            out: VecDeque::new(),
            root: Node::Dir(root),
            sessions: HashMap::new(),
            flash_total: 96_862_208,
            os_installed: None,
            plugged: true,
            screen: default_screen(),
            rle_cache: None,
            stats: HashMap::new(),
            keys_pressed: Vec::new(),
            replug_after_ack: false,
            refused_reads: Default::default(),
            cfg,
        };
        let mut doc = b"*TIMLP0500".to_vec();
        doc.resize(210, 0);
        st.put("/Examples/Getting Started.tns", doc);
        st.put("/themes.csv", b"eWindowTitleBarBackground,0xFFFFFF,0xFF0000\n".to_vec());
        Sim(Arc::new(Mutex::new(st)))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn transport(&self, key: &str) -> SimTransport {
        SimTransport { sim: self.clone(), key: key.to_owned() }
    }

    // ------------------------------------------------------- test hooks
    pub fn put(&self, path: &str, data: impl Into<Vec<u8>>) {
        self.state().put(path, data.into());
    }

    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        match self.state().node(path) {
            Some(Node::File(d)) => Some(d.clone()),
            _ => None,
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        self.state().node(path).is_some()
    }

    pub fn flash_total(&self) -> u64 {
        self.state().flash_total
    }

    pub fn unplug(&self) {
        self.state().plugged = false;
    }

    /// Simulate re-plugging: the calc restarts the link and asks for an address again.
    pub fn replug(&self) {
        self.state().replug();
    }

    /// Restart the link right after acking the next request, while the host waits for the reply.
    /// Refuse to hand over this file when it's read (as if it were damaged).
    pub fn refuse_reads(&self, path: &str) {
        self.state().refused_reads.insert(path.to_owned());
    }

    pub fn replug_after_next_ack(&self) {
        self.state().replug_after_ack = true;
    }

    /// How often something happened: "idle_drops", "random_drops", "out_of_order", "request_4060", ...
    pub fn stat(&self, name: &str) -> u64 {
        self.state().stats.get(name).copied().unwrap_or(0)
    }

    pub fn keys_pressed(&self) -> Vec<u32> {
        self.state().keys_pressed.clone()
    }

    pub fn os_installed(&self) -> Option<Vec<u8>> {
        self.state().os_installed.clone()
    }

    /// Replace the screen (320x240 RGB565 little endian).
    pub fn set_screen(&self, pixels: Vec<u8>) {
        let mut st = self.state();
        st.screen = pixels;
        st.rle_cache = None;
    }

    /// Queue an unsolicited NavNet packet from the calculator.
    pub fn send_navnet(&self, packet: Vec<u8>) {
        self.state().send(nnse::SVC_STREAM, packet, 1, nnse::ADDR_CALC, nnse::ADDR_ME);
    }

    fn host_write(&self, data: &[u8]) -> Result<()> {
        self.state().host_write(data)
    }

    fn host_read(&self) -> Result<Option<Vec<u8>>> {
        self.state().host_read()
    }
}

fn default_screen() -> Vec<u8> {
    let (w, h) = (320u32, 240u32);
    let mut px = Vec::with_capacity((w * h * 2) as usize);
    for y in 0..h {
        for x in 0..w {
            let (r, g) = (x * 31 / w, y * 63 / h);
            let b = if (x / 40 + y / 40) % 2 == 1 { 20 } else { 8 };
            px.extend_from_slice(&(((r << 11) | (g << 5) | b) as u16).to_le_bytes()); // like the CX II
        }
    }
    px
}

impl State {
    // --------------------------------------------------------------- fs
    fn node(&self, path: &str) -> Option<&Node> {
        let mut n = &self.root;
        for part in parts(path) {
            match n {
                Node::Dir(c) => n = c.get(part)?,
                Node::File(_) => return None,
            }
        }
        Some(n)
    }

    /// The folder holding `path`, and the last name in it.
    fn parent_mut(&mut self, path: &str, create: bool) -> Option<(&mut BTreeMap<String, Node>, String)> {
        let parts = parts(path);
        let (last, dirs) = parts.split_last()?;
        let mut n = &mut self.root;
        for part in dirs {
            let Node::Dir(c) = n else { return None };
            if !c.contains_key(*part) {
                if !create {
                    return None;
                }
                c.insert((*part).to_owned(), Node::dir());
            }
            n = c.get_mut(*part)?;
        }
        match n {
            Node::Dir(c) => Some((c, (*last).to_owned())),
            Node::File(_) => None,
        }
    }

    fn put(&mut self, path: &str, data: Vec<u8>) {
        if let Some((parent, name)) = self.parent_mut(path, true) {
            parent.insert(name, Node::File(data));
        }
    }

    fn replug(&mut self) {
        self.plugged = true;
        self.handshake_done = false;
        self.fresh_link = true;
        self.sessions.clear();
    }

    fn random(&mut self) -> f64 {
        // splitmix64
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    fn count(&mut self, what: &str) {
        *self.stats.entry(what.to_owned()).or_default() += 1;
    }

    // -------------------------------------------------------- transport
    fn host_write(&mut self, data: &[u8]) -> Result<()> {
        if !self.plugged {
            return Err(Error::NoDevice("unplugged".into()));
        }
        let Some(m) = Message::decode(data) else { return Ok(()) };
        let now = Instant::now();
        let idle = now - self.last_rx;
        self.last_rx = now;
        if m.service & nnse::ACK_FLAG != 0 {
            return Ok(()); // the host acking our packets; we don't retransmit, so nothing to do
        }
        match m.service {
            nnse::SVC_ADDRREQ if m.data.first() == Some(&0x80) => {
                if !self.handshake_done && self.fresh_link {
                    self.fresh_link = false;
                    self.send(nnse::SVC_UNKNOWN, vec![0x01], 1, nnse::ADDR_CALC, nnse::ADDR_ME);
                    self.send(nnse::SVC_TIME, vec![0x00], 0, nnse::ADDR_CALC, nnse::ADDR_ME);
                }
                return Ok(());
            }
            nnse::SVC_TIME => {
                self.handshake_done = true;
                self.expected = m.seqno.wrapping_add(1);
                return Ok(());
            }
            nnse::SVC_STREAM if self.handshake_done => {}
            _ => return Ok(()),
        }
        if self.cfg.idle_drop.is_some_and(|limit| idle > limit) {
            self.count("idle_drops");
            return Ok(());
        }
        if self.cfg.drop_rate > 0.0 && self.random() < self.cfg.drop_rate {
            self.count("random_drops");
            return Ok(());
        }
        if m.seqno == self.expected.wrapping_sub(1) {
            self.ack(&m); // a duplicate: ack again, don't process
            return Ok(());
        }
        if m.seqno != self.expected {
            self.count("out_of_order");
            return Ok(());
        }
        self.expected = self.expected.wrapping_add(1);
        self.ack(&m);
        if std::mem::take(&mut self.replug_after_ack) {
            self.replug(); // restart the link while the host waits for a reply
            return Ok(());
        }
        self.navnet(&m.data);
        Ok(())
    }

    fn host_read(&mut self) -> Result<Option<Vec<u8>>> {
        if !self.plugged {
            return Err(Error::NoDevice("unplugged".into()));
        }
        if self.out.is_empty() && self.fresh_link && !self.handshake_done {
            let mut req = vec![0x00];
            req.resize(65, 0);
            self.send(nnse::SVC_ADDRREQ, req, 0, nnse::ADDR_ALL, nnse::ADDR_ALL);
        }
        Ok(self.out.pop_front())
    }

    fn ack(&mut self, m: &Message) {
        let ack =
            Message::new(m.service | nnse::ACK_FLAG, Vec::new()).route(nnse::ADDR_CALC, nnse::ADDR_ME).seq(m.seqno);
        self.transmit(ack);
    }

    fn send(&mut self, service: u8, payload: Vec<u8>, req_ack: u8, src: u8, dest: u8) {
        self.calc_seq = self.calc_seq.wrapping_add(1);
        let m = Message::new(service, payload).ack(req_ack).route(src, dest).seq(self.calc_seq);
        self.transmit(m);
    }

    /// Like the real CX II, pad a message whose length is a multiple of 64 with one byte.
    fn transmit(&mut self, m: Message) {
        let mut raw = m.encode();
        if raw.len() % 64 == 0 {
            raw.push(0);
        }
        self.out.push_back(raw);
    }

    // ------------------------------------------------------------ navnet
    fn reply(&mut self, p: &Packet, data: Vec<u8>) {
        let pkt = Packet {
            src_addr: navnet::CALC_ADDR,
            dst_addr: navnet::HOST_ADDR,
            ..Packet::new(p.dst_sid, p.src_sid, data)
        };
        let raw = pkt.encode().expect("simulator replies fit in one packet");
        self.send(nnse::SVC_STREAM, raw, 1, nnse::ADDR_CALC, nnse::ADDR_ME);
    }

    fn navnet(&mut self, raw: &[u8]) {
        let Ok(p) = Packet::parse(raw) else { return };
        let (dst, src) = (p.dst_sid, p.src_sid);
        if dst == navnet::SID_ADDR_ASSIGN || [navnet::SID_ACK, navnet::SID_ACK_ZERO, navnet::SID_NACK].contains(&src) {
            return; // the host acking/refusing one of our packets isn't a request
        }
        // The host ends a service session with a packet *from* 0x40DE (to the service) naming its
        // session id, as libnspire does.
        if src == navnet::SID_DISCONNECT || dst == navnet::SID_DISCONNECT {
            if let &[a, b, ..] = p.data.as_slice() {
                self.sessions.remove(&u16::from_be_bytes([a, b]));
            }
            self.count("disconnects");
            return;
        }
        self.count(&format!("request_{dst:04x}"));
        let mode = self.sessions.remove(&src).unwrap_or(Mode::Idle);
        let mode = match dst {
            0x4020 => {
                self.devinfo(&p);
                mode
            }
            0x4024 => {
                self.screen(&p);
                mode
            }
            0x4042 => {
                self.key(&p);
                mode
            }
            0x4060 => self.files(&p, mode),
            0x4080 => self.os(&p, mode),
            _ => mode,
        };
        self.sessions.insert(src, mode);
    }

    fn devinfo(&mut self, p: &Packet) {
        match p.data.first() {
            Some(&1) => {
                let used = self.root.used() as u64;
                let mut b = vec![0x01];
                for v in [self.flash_total - used, self.flash_total, 32_000_000, 36_220_968] {
                    b.extend_from_slice(&u64::to_be_bytes(v));
                }
                b.extend_from_slice(&[0xFF, 0, 0, 32]); // battery, pad, charging, clock
                b.extend_from_slice(&self.cfg.os_version);
                b.extend_from_slice(&[5, 0, 0, 42, 5, 13, 0, 110]); // boot1, boot2
                b.extend_from_slice(&4u32.to_be_bytes()); // hardware version
                for v in [2u16, 0, 0, 320, 240] {
                    b.extend_from_slice(&v.to_be_bytes()); // run level, (lcd x, y), width, height
                }
                b.extend_from_slice(&[16, 1, 0x1D]); // bpp, sample mode, hardware type
                let mut eid = b"SIM0000000000000".to_vec();
                eid.resize(17, 0);
                b.extend_from_slice(&eid);
                b.extend_from_slice(b"SIM000000000000000000000000");
                self.reply(p, b);
            }
            Some(&2) => {
                let mut b = vec![0x02];
                b.extend_from_slice(self.cfg.name.as_bytes());
                b.push(0);
                self.reply(p, b);
            }
            Some(&3) => self.reply(p, b"\x03.tns\0.tco2\0".to_vec()),
            _ => {}
        }
    }

    fn screen(&mut self, p: &Packet) {
        let data = match &self.rle_cache {
            Some(d) => d.clone(),
            None => {
                let d = rle_encode(16, &self.screen);
                self.rle_cache = Some(d.clone());
                d
            }
        };
        let mut head = vec![0x00];
        head.extend_from_slice(&(data.len() as u32).to_be_bytes());
        for v in [0u16, 0, 320, 240] {
            head.extend_from_slice(&v.to_be_bytes());
        }
        head.extend_from_slice(&[16, 0]);
        self.reply(p, head);
        for chunk in data.chunks(1439) {
            let mut b = vec![0x00];
            b.extend_from_slice(chunk);
            self.reply(p, b);
        }
    }

    fn key(&mut self, p: &Packet) {
        let d = &p.data;
        if d.len() == 26 && d[0] == 0 {
            // the key event (the 01 00 00 80 packet before it carries no key)
            let code = (u32::from(d[6]) << 16) | (u32::from(d[8]) << 8) | u32::from(d[24]);
            self.keys_pressed.push(code);
        }
    }

    fn files(&mut self, p: &Packet, mut mode: Mode) -> Mode {
        let d = p.data.as_slice();
        match &mut mode {
            Mode::Write { path, size, buf } if d.first() == Some(&0x05) => {
                buf.extend_from_slice(&d[1..]);
                if buf.len() >= *size {
                    let mut data = std::mem::take(buf);
                    data.truncate(*size);
                    let path = std::mem::take(path);
                    self.put(&path, data);
                    self.reply(p, OK.to_vec());
                    return Mode::Idle;
                }
                return mode;
            }
            Mode::Read(data) if d.first() == Some(&0x04) => {
                let data = std::mem::take(data);
                for chunk in data.chunks(1439) {
                    let mut b = vec![0x05];
                    b.extend_from_slice(chunk);
                    self.reply(p, b);
                }
                return Mode::Idle;
            }
            Mode::Read(_) if d.starts_with(&OK) => return Mode::Idle,
            Mode::List(items) if d.first() == Some(&0x0E) => {
                match items.pop_front() {
                    Some((name, is_dir, size)) => {
                        let mut rest = padded(&name);
                        rest.extend_from_slice(&(size as u32).to_be_bytes());
                        rest.extend_from_slice(&2u32.to_be_bytes());
                        rest.extend_from_slice(&[u8::from(is_dir), 0]);
                        let mut b = vec![0x10, 0x00, rest.len() as u8];
                        b.extend(rest);
                        self.reply(p, b);
                    }
                    None => self.reply(p, OK.to_vec()),
                }
                return mode;
            }
            Mode::List(_) if d.first() == Some(&0x0F) => {
                self.reply(p, OK.to_vec());
                return Mode::Idle;
            }
            _ => {}
        }
        // a new command
        let Some(&op) = d.first() else { return mode };
        if op == 0x0D {
            let Ok((path, _)) = scan_padded(d, 1) else { return mode };
            let listing = match self.node(&path) {
                None => Err(MISSING.to_vec()),
                Some(Node::File(_)) => Err(vec![0xFF, 0x0F]),
                Some(Node::Dir(c)) => Ok(c.iter().map(|(n, c)| (n.clone(), c.is_dir(), c.size())).collect()),
            };
            return match listing {
                Ok(items) => {
                    self.reply(p, OK.to_vec());
                    Mode::List(items)
                }
                Err(code) => {
                    self.reply(p, code);
                    mode
                }
            };
        }
        let &[a, b, ..] = d else { return mode };
        match u16::from_be_bytes([a, b]) {
            0x2001 => {
                let Ok((path, _)) = scan_padded(d, 2) else { return mode };
                let reply = match self.node(&path) {
                    None => MISSING.to_vec(),
                    Some(n) => {
                        let mut r = vec![0x20];
                        r.extend_from_slice(&(n.size() as u32).to_be_bytes());
                        r.extend_from_slice(&2u32.to_be_bytes());
                        r.extend_from_slice(&[u8::from(n.is_dir()), 0]);
                        r
                    }
                };
                self.reply(p, reply);
            }
            0x0701 => {
                let Ok((path, _)) = scan_padded(d, 2) else { return mode };
                if self.refused_reads.contains(&path) {
                    self.reply(p, vec![0xFF, 0x04]);
                    return mode;
                }
                match self.node(&path) {
                    Some(Node::File(data)) => {
                        let data = data.clone();
                        let mut r = vec![0x03, 0x01];
                        r.extend_from_slice(&[0; 9]);
                        r.extend_from_slice(&(data.len() as u32).to_be_bytes());
                        self.reply(p, r);
                        return Mode::Read(data);
                    }
                    _ => self.reply(p, MISSING.to_vec()),
                }
            }
            0x0301 => {
                let Ok((path, off)) = scan_padded(d, 2) else { return mode };
                let Some(size) = d.get(off..off + 4).map(|s| u32::from_be_bytes(s.try_into().unwrap()) as usize) else {
                    return mode;
                };
                let room = self.flash_total as usize - self.root.used();
                let name = match self.parent_mut(&path, false) {
                    Some((_, name)) if size <= room => name,
                    _ => {
                        self.reply(p, MISSING.to_vec());
                        return mode;
                    }
                };
                if !name.to_lowercase().ends_with(".tns") {
                    self.reply(p, vec![0xFF, 0x15]); // like the real CX II: only .tns documents
                    return mode;
                }
                self.reply(p, vec![0x04]);
                if size == 0 {
                    self.put(&path, Vec::new());
                    self.reply(p, OK.to_vec());
                    return Mode::Idle;
                }
                return Mode::Write { path, size, buf: Vec::new() };
            }
            0x0A03 => {
                let Ok((path, _)) = scan_padded(d, 2) else { return mode };
                let created = match self.parent_mut(&path, false) {
                    Some((parent, name)) if !parent.contains_key(&name) => {
                        parent.insert(name, Node::dir());
                        true
                    }
                    _ => false,
                };
                self.reply(p, if created { OK.to_vec() } else { MISSING.to_vec() });
            }
            code @ (0x0B03 | 0x0901) => {
                let Ok((path, _)) = scan_padded(d, 2) else { return mode };
                let want_dir = code == 0x0B03;
                let removed = match self.parent_mut(&path, false) {
                    Some((parent, name)) => match parent.get(&name) {
                        Some(Node::Dir(c)) if want_dir && c.is_empty() => parent.remove(&name).is_some(),
                        Some(Node::File(_)) if !want_dir => parent.remove(&name).is_some(),
                        _ => false,
                    },
                    None => false,
                };
                self.reply(p, if removed { OK.to_vec() } else { MISSING.to_vec() });
            }
            code @ (0x2101 | 0x0C01) => {
                let Ok((src, off)) = scan_padded(d, 2) else { return mode };
                let Ok((dst, _)) = scan_padded(d, off) else { return mode };
                let done = self.move_or_copy(&src, &dst, code == 0x0C01);
                self.reply(p, if done { OK.to_vec() } else { MISSING.to_vec() });
            }
            _ => self.reply(p, vec![0xFF, 0x0F]),
        }
        mode
    }

    fn move_or_copy(&mut self, src: &str, dst: &str, copy: bool) -> bool {
        let Some(node) = self.node(src) else { return false };
        if copy && node.is_dir() {
            return false;
        }
        let copied = copy.then(|| node.deep_clone());
        let dst_free = match self.parent_mut(dst, false) {
            Some((parent, name)) => !parent.contains_key(&name),
            None => false,
        };
        let into_itself = dst.trim_end_matches('/').starts_with(&format!("{}/", src.trim_end_matches('/')));
        if !dst_free || into_itself {
            return false;
        }
        let node = match copied {
            Some(n) => n,
            None => {
                let Some((parent, name)) = self.parent_mut(src, false) else { return false };
                let Some(n) = parent.remove(&name) else { return false };
                n
            }
        };
        if let Some((parent, name)) = self.parent_mut(dst, false) {
            parent.insert(name, node);
        }
        true
    }

    fn os(&mut self, p: &Packet, mode: Mode) -> Mode {
        let d = p.data.as_slice();
        match (d.first(), mode) {
            (Some(&0x03), _) if d.len() >= 5 => {
                let size = u32::from_be_bytes([d[1], d[2], d[3], d[4]]) as usize;
                self.reply(p, vec![0x04]);
                Mode::Os { size, buf: Vec::new(), first: true }
            }
            (Some(&0x05), Mode::Os { size, mut buf, first }) => {
                buf.extend_from_slice(&d[1..]);
                if first {
                    // stand-in for the real signature check: accept images that start like our tests'
                    let ok = buf.starts_with(b"TI-Nspire");
                    self.reply(p, if ok { OK.to_vec() } else { vec![0xFF, 0x04] }); // FF 04: damaged
                    if !ok {
                        return Mode::Idle;
                    }
                }
                if buf.len() >= size {
                    for pct in [0, 25, 50, 75, 100] {
                        self.reply(p, vec![0x00, pct]);
                    }
                    self.os_installed = Some(buf);
                    return Mode::Idle;
                }
                Mode::Os { size, buf, first: false }
            }
            (_, mode) => mode,
        }
    }
}

/// Transport that talks to a [`Sim`] in-process.
pub struct SimTransport {
    sim: Sim,
    key: String,
}

impl Transport for SimTransport {
    fn write(&mut self, data: &[u8], _timeout: Duration) -> Result<()> {
        self.sim.host_write(data)
    }

    fn read(&mut self, max: usize, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(mut msg) = self.sim.host_read()? {
                msg.truncate(max);
                return Ok(Some(msg));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            std::thread::sleep((deadline - now).min(Duration::from_millis(5)));
        }
    }

    fn device_key(&self) -> String {
        self.key.clone()
    }
}
