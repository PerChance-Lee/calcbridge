//! calcbridge: the CalcBridge command line.
//!
//! Run it from PowerShell, cmd or Terminal. (Git Bash on Windows rewrites a bare "/" argument into
//! a Windows path; handheld paths mangled that way are repaired.)

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use calcbridge_link::sim::{Sim, SimConfig};
use calcbridge_link::{Calculator, Options, keys, usb};
use calcbridge_tns::{Paragraph, TnsDocument, build_lua, build_notes, build_python, build_text, document, textio};
use clap::{Parser, Subcommand, ValueEnum};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(name = "calcbridge", version, about = "CalcBridge command line for the TI-Nspire CX II")]
struct Cli {
    /// Talk to a simulated calculator instead of a real one
    #[arg(long, global = true)]
    sim: bool,
    /// Print every packet exchanged with the calculator
    #[arg(long, global = true)]
    trace: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List connected calculators
    Devices,
    /// Handheld details
    Info,
    /// List a folder
    Ls {
        #[arg(default_value = "/")]
        path: String,
        #[arg(short, long)]
        recursive: bool,
    },
    /// Download a file or folder
    Get { remote: String, local: Option<PathBuf> },
    /// Upload files or folders into a handheld folder
    Put {
        #[arg(required = true, num_args = 1..)]
        local: Vec<PathBuf>,
        remote: String,
        /// Replace existing files
        #[arg(short, long)]
        force: bool,
    },
    /// Make a folder
    Mkdir {
        path: String,
        #[arg(short, long)]
        parents: bool,
    },
    /// Delete files or folders (a folder with everything in it)
    Rm {
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Move or rename on the handheld
    Mv { src: String, dst: String },
    /// Copy on the handheld (folders too)
    Cp { src: String, dst: String },
    /// Save a screenshot (.png, .jpg or .bmp)
    Shot {
        #[arg(default_value = "screen.png")]
        file: PathBuf,
    },
    /// Copy everything on the handheld into a folder
    Backup { folder: PathBuf },
    /// Upload a backup folder's documents
    Restore {
        folder: PathBuf,
        #[arg(default_value = "/")]
        remote: String,
        /// Replace existing files
        #[arg(short, long)]
        force: bool,
    },
    /// Install an OS image (.tco2/.tcc2/.tct2); the handheld checks it and restarts
    InstallOs {
        file: PathBuf,
        /// Send it even if the extension doesn't match this handheld
        #[arg(long)]
        force: bool,
    },
    /// Press keys (esc home enter up down a 1 ...; key*3 repeats; see `calcbridge keys`)
    Key {
        #[arg(required = true, allow_hyphen_values = true)]
        keys: Vec<String>,
        /// Seconds between presses
        #[arg(long, default_value_t = 0.3)]
        delay: f64,
    },
    /// Type text on the handheld
    Type {
        #[arg(allow_hyphen_values = true)]
        text: String,
        /// Seconds between presses
        #[arg(long, default_value_t = 0.1)]
        delay: f64,
    },
    /// List the key names `key` understands
    Keys,
    /// Make a .tns from a .py, .lua, .txt or .md file (no calculator needed)
    Build {
        source: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = Kind::Auto)]
        kind: Kind,
    },
    /// Describe a .tns file (no calculator needed)
    View {
        file: PathBuf,
        /// Save its Python files into this folder
        #[arg(long, value_name = "FOLDER")]
        extract: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Kind {
    Auto,
    Notes,
    Python,
    Lua,
    Text,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn size(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "down*3" -> ("down", 3); a plain "*" (multiply) is just a key.
fn parse_key_arg(arg: &str) -> (&str, u32) {
    if let Some((name, n)) = arg.rsplit_once('*') {
        if !name.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(n) = n.parse() {
                return (name, n);
            }
        }
    }
    (arg, 1)
}

/// Undo Git Bash's path conversion: on Windows it turns "/folder" into "C:/Program Files/Git/folder".
fn remote(path: &str) -> String {
    if cfg!(windows) {
        let b = path.as_bytes();
        if b.len() >= 3 && b[0].is_ascii_alphabetic() && &b[1..3] == b":/" {
            let rest = &path[3..];
            let at = rest.match_indices("Git/").map(|(i, _)| i).find(|&i| i == 0 || rest.as_bytes()[i - 1] == b'/');
            if let Some(i) = at {
                return format!("/{}", &rest[i + 4..]);
            }
        }
    }
    path.to_owned()
}

/// Progress on one line of stderr; cleared when done.
fn progress_line(label: &str) -> impl FnMut(u64, u64) + '_ {
    move |done, total| {
        if total >= 64 * 1024 {
            eprint!("\r{label} {:>3}%  {} / {} bytes   ", done * 100 / total.max(1), size(done), size(total));
            if done >= total {
                eprint!("\r{}\r", " ".repeat(label.len() + 40));
            }
            let _ = std::io::stderr().flush();
        }
    }
}

fn open(cli: &Cli) -> Res<Calculator> {
    let trace: Option<calcbridge_link::Trace> = cli.trace.then(|| Arc::new(|line: &str| eprintln!("{line}")) as _);
    if cli.sim {
        let sim = Sim::new(SimConfig { idle_drop: None, ..SimConfig::default() });
        let opts = Options { state_file: None, trace, ..Options::default() };
        return Ok(Calculator::open(Box::new(sim.transport("sim")), opts, None)?);
    }
    let mut searching = |done: usize, total: usize| {
        eprint!("\rfinding the connection ({done}/{total})...");
        let _ = std::io::stderr().flush();
    };
    let calc = Calculator::open_usb(None, Options { trace, ..Options::default() }, Some(&mut searching))?;
    Ok(calc)
}

fn run(cli: Cli) -> Res {
    match &cli.cmd {
        Cmd::Build { source, output, kind } => return build(source, output.as_deref(), *kind),
        Cmd::View { file, extract } => return view(file, extract.as_deref()),
        Cmd::Keys => {
            println!("{}", keys::names().collect::<Vec<_>>().join(" "));
            return Ok(());
        }
        Cmd::Devices => {
            let found = usb::list()?;
            if found.is_empty() {
                println!("no TI-Nspire CX II connected");
            }
            for d in found {
                println!(
                    "{}  serial {}  bus {} address {}",
                    d.product,
                    d.serial.as_deref().unwrap_or("-"),
                    d.bus,
                    d.address
                );
            }
            return Ok(());
        }
        _ => {}
    }
    let calc = open(&cli)?;
    let result = run_on(&calc, cli.cmd);
    calc.close();
    result
}

fn run_on(c: &Calculator, cmd: Cmd) -> Res {
    match cmd {
        Cmd::Info => {
            let i = c.info()?;
            let rows = [
                ("Name", i.name.clone()),
                ("Model", i.model()),
                ("OS", i.os_version.clone()),
                ("Boot loaders", format!("{} / {}", i.boot1_version, i.boot2_version)),
                ("Product ID", i.electronic_id.clone()),
                ("Storage", format!("{} of {} bytes used", size(i.flash_total - i.flash_free), size(i.flash_total))),
                ("Memory", format!("{} of {} bytes used", size(i.ram_total - i.ram_free), size(i.ram_total))),
                ("Battery", format!("{}{}", i.battery, if i.charging { " (charging)" } else { "" })),
                ("Screen", format!("{}x{} {}-bit", i.lcd_width, i.lcd_height, i.lcd_bpp)),
            ];
            for (k, v) in rows {
                println!("{k:13} {v}");
            }
        }
        Cmd::Ls { path, recursive } => {
            let path = remote(&path);
            let entries = if recursive { c.walk(&path)? } else { c.list_dir(&path)? };
            for e in entries {
                let shown = if recursive { &e.path } else { &e.name };
                println!("{:>12}  {shown}", if e.is_dir { "<DIR>".to_owned() } else { size(e.size) });
            }
        }
        Cmd::Get { remote: r, local } => {
            let r = remote(&r);
            let local = local
                .unwrap_or_else(|| PathBuf::from(calcbridge_link::local_name(calcbridge_link::services::basename(&r))));
            let n = c.download(&r, &local, Some(&mut progress_line("receiving")))?;
            println!("received {} bytes", size(n));
        }
        Cmd::Put { local, remote: r, force } => {
            let r = remote(&r);
            for path in local {
                let mut skipped = Vec::new();
                let sent = c.upload(&path, &r, Some(&mut progress_line("sending")), force, None, Some(&mut skipped))?;
                println!("sent {sent}");
                for s in skipped {
                    println!("skipped {} (the handheld only takes .tns documents)", s.display());
                }
            }
        }
        Cmd::Mkdir { path, parents } => c.mkdir(&remote(&path), parents)?,
        Cmd::Rm { paths } => {
            for p in paths {
                let p = remote(&p);
                c.delete(&p)?;
                println!("deleted {p}");
            }
        }
        Cmd::Mv { src, dst } => c.move_to(&remote(&src), &remote(&dst))?,
        Cmd::Cp { src, dst } => c.copy(&remote(&src), &remote(&dst))?,
        Cmd::Shot { file } => {
            let s = c.screenshot()?;
            s.save(&file)?;
            println!("saved {}x{} screenshot to {}", s.width, s.height, file.display());
        }
        Cmd::Backup { folder } => {
            let mut failed = Vec::new();
            let n = c.backup(&folder, Some(&mut |p: &str, i, t| println!("[{i}/{t}] {p}")), Some(&mut failed))?;
            println!("backed up {n} files to {}", folder.display());
            for (path, why) in &failed {
                println!("couldn't read {path}: {why}");
            }
        }
        Cmd::Restore { folder, remote: r, force } => {
            let mut skipped = Vec::new();
            let n = c.restore(
                &folder,
                &remote(&r),
                force,
                Some(&mut |p: &str, i, t| println!("[{i}/{t}] {p}")),
                Some(&mut skipped),
            )?;
            println!("restored {n} files");
            if !skipped.is_empty() {
                println!("skipped {} (not documents, or already there)", skipped.len());
            }
        }
        Cmd::InstallOs { file, force } => {
            let want = c.info()?.os_extension;
            let name = file.to_string_lossy().to_lowercase();
            if !force && !want.is_empty() && !name.ends_with(&want.to_lowercase()) {
                return Err(format!("this handheld needs a {want} file (use --force to send anyway)").into());
            }
            let image = std::fs::read(&file)?;
            c.install_os(
                &image,
                Some(&mut |stage, d, t| {
                    eprint!("\r{stage:?}: {d}/{t}   ");
                    let _ = std::io::stderr().flush();
                }),
            )?;
            println!("\ndone; the handheld restarts");
        }
        Cmd::Key { keys: names, delay } => {
            for arg in &names {
                let (name, n) = parse_key_arg(arg);
                let code =
                    keys::key_code(name).ok_or_else(|| format!("unknown key {name:?} (see `calcbridge keys`)"))?;
                for _ in 0..n {
                    c.press_key(code)?;
                    std::thread::sleep(Duration::from_secs_f64(delay.max(0.0)));
                }
            }
        }
        Cmd::Type { text, delay } => {
            let codes = keys::text_codes(&text).map_err(|ch| format!("the handheld has no key for {ch:?}"))?;
            for code in codes {
                c.press_key(code)?;
                std::thread::sleep(Duration::from_secs_f64(delay.max(0.0)));
            }
        }
        Cmd::Devices | Cmd::Keys | Cmd::Build { .. } | Cmd::View { .. } => unreachable!("handled without a calculator"),
    }
    Ok(())
}

fn build(source: &Path, output: Option<&Path>, kind: Kind) -> Res {
    let out = output.map_or_else(|| source.with_extension("tns"), Path::to_path_buf);
    let same = |a: &Path, b: &Path| {
        a == b || std::fs::canonicalize(a).ok().zip(std::fs::canonicalize(b).ok()).is_some_and(|(x, y)| x == y)
    };
    if same(source, &out) {
        return Err(format!(
            "{} is already a .tns; building it would overwrite it (use -o to write somewhere else)",
            source.display()
        )
        .into());
    }
    let src = textio::read_text(source)?;
    let ext = source.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let kind = match kind {
        Kind::Auto => match ext.as_str() {
            "md" | "notes" => Kind::Notes,
            "py" => Kind::Python,
            "lua" => Kind::Lua,
            _ => Kind::Text,
        },
        k => k,
    };
    let data = match kind {
        Kind::Notes => build_notes(&[src.lines().map(Paragraph::from).collect()])?,
        Kind::Python => {
            let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            build_python(&format!("{}.py", document::safe_identifier(&stem)), &src, &[])?
        }
        Kind::Lua => build_lua(&src)?,
        Kind::Text | Kind::Auto => build_text(&src, None)?,
    };
    std::fs::write(&out, data)?;
    println!("wrote {}", out.display());
    Ok(())
}

fn view(file: &Path, extract: Option<&Path>) -> Res {
    let doc = TnsDocument::from_bytes(&std::fs::read(file)?)?;
    println!("{}", doc.summary());
    if let Some(folder) = extract {
        std::fs::create_dir_all(folder)?;
        for (name, code) in doc.python_files() {
            // names come from the document; keep them inside the folder
            let Some(path) = textio::safe_join(folder, &name) else {
                println!("skipped unsafe name {name:?}");
                continue;
            };
            std::fs::write(&path, code)?;
            println!("extracted {}", path.file_name().unwrap_or_default().to_string_lossy());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_args() {
        assert_eq!(parse_key_arg("*"), ("*", 1));
        assert_eq!(parse_key_arg("down*3"), ("down", 3));
        assert_eq!(parse_key_arg("a"), ("a", 1));
    }

    #[test]
    fn git_bash_paths() {
        if cfg!(windows) {
            assert_eq!(remote("C:/Program Files/Git/"), "/");
            assert_eq!(remote("C:/Program Files/Git/MyLib/x.tns"), "/MyLib/x.tns");
        }
        assert_eq!(remote("/MyLib"), "/MyLib");
    }

    #[test]
    fn sizes() {
        assert_eq!(size(0), "0");
        assert_eq!(size(1234567), "1,234,567");
    }

    #[test]
    fn view_extract_is_safe() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("d.tns");
        std::fs::write(&doc, build_python("ok.py", "print(1)\n", &[("../escape.py", "x = 1\n")]).unwrap()).unwrap();
        view(&doc, Some(&dir.path().join("out"))).unwrap();
        assert!(!dir.path().join("escape.py").exists());
        assert!(dir.path().join("out").join("ok.py").exists() && dir.path().join("out").join("escape.py").exists());
    }

    #[test]
    fn build_never_overwrites_its_source() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("doc.tns");
        std::fs::write(&doc, b"original").unwrap();
        assert!(build(&doc, None, Kind::Auto).is_err());
        assert!(build(&doc, Some(&doc), Kind::Auto).is_err());
        assert_eq!(std::fs::read(&doc).unwrap(), b"original");
    }

    #[test]
    fn builds_each_kind() {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in
            [("a.md", "# Title\n$x^{2}$"), ("b.py", "print(1)\n"), ("c.lua", "x = 1\n"), ("d.txt", "hi")]
        {
            let src = dir.path().join(name);
            std::fs::write(&src, body).unwrap();
            build(&src, None, Kind::Auto).unwrap();
            let doc = TnsDocument::from_bytes(&std::fs::read(src.with_extension("tns")).unwrap()).unwrap();
            assert!(!doc.pages().is_empty(), "{name}");
        }
    }
}
