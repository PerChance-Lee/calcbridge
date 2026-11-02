//! Everything the window can ask for.

use std::path::{Path, PathBuf};

use calcbridge_link::services::{basename, join};
use calcbridge_link::{Calculator, DeviceInfo, DirEntry, OsStage, check_name, is_sendable, local_name, usb};
use calcbridge_tns::r2d::{self, ViewParagraph};
use calcbridge_tns::{Page, Paragraph, TnsDocument, build_lua, build_notes, build_python, build_text, document, textio};
use serde::{Deserialize, Serialize};
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, async_runtime};

use crate::device::{self, Device, Status};
use crate::error::{CmdError, CmdResult};
use crate::jobs::{self, JobCtx};
use crate::applog::{self, LogLine};

/// Run blocking work off the async runtime's threads.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> CmdResult<T> + Send + 'static) -> CmdResult<T> {
    async_runtime::spawn_blocking(f).await.map_err(|e| CmdError::new("app", e.to_string()))?
}

fn app_is_simulated(app: &AppHandle) -> bool {
    app.state::<Device>().is_simulated()
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

// ------------------------------------------------------------------ device
#[tauri::command]
pub fn status(app: AppHandle) -> Status {
    app.state::<Device>().status()
}

#[tauri::command]
pub async fn connect(app: AppHandle, simulated: bool) -> CmdResult<DeviceInfo> {
    blocking(move || device::connect(&app, simulated, true)).await
}

#[tauri::command]
pub async fn disconnect(app: AppHandle) -> CmdResult<()> {
    blocking(move || {
        device::disconnect(&app);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn refresh_info(app: AppHandle) -> CmdResult<DeviceInfo> {
    blocking(move || device::refresh_info(&app)).await
}

#[tauri::command]
pub fn set_auto_connect(app: AppHandle, on: bool) {
    app.state::<Device>().set_auto_connect(on);
}

#[tauri::command]
pub async fn usb_devices() -> CmdResult<Vec<usb::UsbDevice>> {
    blocking(|| Ok(usb::list()?)).await
}

#[tauri::command]
pub fn log_lines() -> Vec<LogLine> {
    applog::lines()
}

// --------------------------------------------------------- handheld files
#[tauri::command]
pub async fn list_dir(app: AppHandle, path: String) -> CmdResult<Vec<DirEntry>> {
    blocking(move || device::run(&app, |c| c.list_dir(&path))).await
}

#[tauri::command]
pub async fn exists(app: AppHandle, paths: Vec<String>) -> CmdResult<Vec<bool>> {
    blocking(move || device::run(&app, |c| paths.iter().map(|p| c.exists(p)).collect())).await
}

#[tauri::command]
pub async fn make_dir(app: AppHandle, path: String) -> CmdResult<()> {
    blocking(move || {
        device::run(&app, |c| c.mkdir(&path, false))?;
        log::info!("created folder {path}");
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn rename(app: AppHandle, path: String, new_name: String) -> CmdResult<String> {
    blocking(move || {
        let to = device::run(&app, |c| c.rename(&path, new_name.trim()))?;
        log::info!("renamed {path} to {to}");
        Ok(to)
    })
    .await
}

/// Delete handheld files and folders (folders with everything in them).
#[tauri::command]
pub fn delete(app: AppHandle, paths: Vec<String>) -> CmdResult<u64> {
    let title = if paths.len() == 1 { format!("Deleting {}", basename(&paths[0])) } else { format!("Deleting {} items", paths.len()) };
    jobs::submit(&app, title, move |ctx| {
        for (i, p) in paths.iter().enumerate() {
            ctx.progress(i as u64, paths.len() as u64, p);
            device::run(ctx.app(), |c| {
                c.check_cancel()?;
                c.delete(p)
            })?;
        }
        Ok(format!("Deleted {}", plural(paths.len(), "item")))
    })
}

/// "Copy of name", then "Copy of name (2)", ..., like TI's software, for a name taken in `folder`.
fn copy_name(c: &Calculator, folder: &str, name: &str, is_dir: bool) -> calcbridge_link::Result<String> {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && !is_dir => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut n = 1;
    loop {
        let candidate = if n == 1 { format!("Copy of {stem}{ext}") } else { format!("Copy of {stem} ({n}){ext}") };
        if !c.exists(&join(folder, &candidate))? {
            return Ok(candidate);
        }
        n += 1;
    }
}

/// Copy or move handheld items into a handheld folder. Copies whose name is taken become
/// "Copy of …"; a move into a folder that already has the name is refused before anything moves.
#[tauri::command]
pub fn copy_or_move(app: AppHandle, paths: Vec<String>, folder: String, keep_original: bool) -> CmdResult<u64> {
    let verb = if keep_original { "Copying" } else { "Moving" };
    jobs::submit(&app, format!("{verb} {} to {folder}", plural(paths.len(), "item")), move |ctx| {
        let app = ctx.app();
        if !keep_original {
            for p in &paths {
                let dst = join(&folder, basename(p));
                if dst != *p && device::run(app, |c| c.exists(&dst))? {
                    return Err(CmdError::new("exists", format!("{} is already in {folder}", basename(p))));
                }
            }
        }
        for (i, p) in paths.iter().enumerate() {
            ctx.progress(i as u64, paths.len() as u64, p);
            device::run(app, |c| {
                c.check_cancel()?;
                let name = basename(p);
                let dst = join(&folder, name);
                if !keep_original {
                    return if dst == *p { Ok(()) } else { c.move_to(p, &dst) };
                }
                let dst = if c.exists(&dst)? {
                    let is_dir = c.stat(p)?.is_some_and(|e| e.is_dir);
                    join(&folder, &copy_name(c, &folder, name, is_dir)?)
                } else {
                    dst
                };
                c.copy(p, &dst)
            })?;
        }
        Ok(format!("{} {}", if keep_original { "Copied" } else { "Moved" }, plural(paths.len(), "item")))
    })
}

/// What `upload` does with names that already exist on the handheld.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Existing {
    Replace,
    Skip,
}

/// A file the handheld won't take as it is, but CalcBridge can turn into a document.
fn convertible(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    [(".py", "python"), (".lua", "lua"), (".txt", "text"), (".md", "notes")]
        .into_iter()
        .find(|(ext, _)| lower.ends_with(ext))
        .map(|(_, kind)| kind)
}

/// The document a convertible file becomes: Python and Lua as programs, Markdown as Notes markup
/// (math and all, like the Create tab), plain text as a Notes page titled with the file name.
fn convert_file(path: &Path, kind: &str) -> CmdResult<Vec<u8>> {
    let text = textio::read_text(path)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(match kind {
        "python" => build_python(&format!("{}.py", document::safe_identifier(&stem)), &text, &[])?,
        "lua" => build_lua(&text)?,
        "notes" => match build_notes(&notes_pages(&text, 0x1F5FD0)) {
            Ok(doc) => doc,
            Err(e) => {
                log::warn!("{}: {e}; sending it as plain text", path.display());
                build_text(&text, Some(&stem))?
            }
        },
        _ => build_text(&text, Some(&stem))?,
    })
}

/// Send local files and folders into a handheld folder. Inside folders only .tns documents go (the
/// handheld refuses anything else); with `convert`, picked .py/.lua/.txt/.md files are turned into
/// documents first.
#[tauri::command]
pub fn upload(app: AppHandle, locals: Vec<String>, folder: String, existing: Existing, convert: Option<bool>) -> CmdResult<u64> {
    let title = match locals.as_slice() {
        [one] => format!("Sending {}", Path::new(one).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        many => format!("Sending {} items", many.len()),
    };
    let convert = convert.unwrap_or(false);
    jobs::submit(&app, title, move |ctx| upload_job(ctx, &locals, &folder, existing, convert))
}

struct UploadItem {
    local: PathBuf,
    remote: String,
    size: u64,
    /// Build a document of this kind from the file instead of sending it as it is.
    convert: Option<&'static str>,
}

fn walk_upload(
    path: &Path,
    remote_folder: &str,
    items: &mut Vec<UploadItem>,
    folders: &mut Vec<String>,
    refused: &mut Vec<String>,
) -> CmdResult<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let remote = join(remote_folder, &name);
    let meta = std::fs::metadata(path)?;
    if meta.is_dir() {
        folders.push(remote.clone());
        let mut children = std::fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        children.sort_by_key(|e| e.file_name());
        for child in children {
            walk_upload(&child.path(), &remote, items, folders, refused)?;
        }
    } else if is_sendable(&name) {
        items.push(UploadItem { local: path.to_path_buf(), remote, size: meta.len(), convert: None });
    } else if !calcbridge_link::is_junk(&name) {
        refused.push(name);
    }
    Ok(())
}

fn upload_job(ctx: &JobCtx, locals: &[String], folder: &str, existing: Existing, convert: bool) -> CmdResult<String> {
    // Work out every file first, so progress can count bytes across all of them.
    let mut items = Vec::new();
    let mut folders = Vec::new();
    let mut refused = Vec::new();
    for local in locals {
        let path = Path::new(local);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match convertible(&name) {
            Some(kind) if convert && path.is_file() && !is_sendable(&name) => {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let size = std::fs::metadata(path)?.len();
                let remote = join(folder, &format!("{stem}.tns"));
                items.push(UploadItem { local: path.to_path_buf(), remote, size, convert: Some(kind) });
            }
            _ => walk_upload(path, folder, &mut items, &mut folders, &mut refused)?,
        }
    }
    for item in &items {
        check_name(basename(&item.remote)).map_err(CmdError::from)?;
    }
    let total: u64 = items.iter().map(|i| i.size).sum::<u64>().max(1);
    let app = ctx.app();
    for f in &folders {
        device::run(app, |c| if c.exists(f)? { Ok(()) } else { c.mkdir(f, false) })?;
    }
    let (mut sent, mut converted, mut skipped, mut done_bytes) = (0usize, 0usize, 0usize, 0u64);
    for item in &items {
        device::run(app, |c| c.check_cancel())?;
        let exists = device::run(app, |c| c.exists(&item.remote))?;
        if exists && existing == Existing::Skip {
            skipped += 1;
            done_bytes += item.size;
            continue;
        }
        let data = match item.convert {
            Some(kind) => convert_file(&item.local, kind)?,
            None => std::fs::read(&item.local)?,
        };
        // a converted file's size differs from its source; scale its progress to the source size
        let (base, scale) = (done_bytes, item.size as f64 / data.len().max(1) as f64);
        device::run(app, |c| {
            let mut progress = |d: u64, _| ctx.progress(base + (d as f64 * scale) as u64, total, &item.remote);
            c.write_file(&item.remote, &data, Some(&mut progress), true)
        })?;
        done_bytes += item.size;
        sent += 1;
        converted += usize::from(item.convert.is_some());
    }
    let mut msg = format!("Sent {}", plural(sent, "file"));
    if converted > 0 {
        msg += &format!(" ({converted} converted into documents)");
    }
    if skipped > 0 {
        msg += &format!(", skipped {skipped} already there");
    }
    if !refused.is_empty() {
        let shown: Vec<_> = refused.iter().take(5).cloned().collect();
        msg += &format!(
            "; left out {} the handheld doesn't accept (only .tns documents): {}{}",
            plural(refused.len(), "file"),
            shown.join(", "),
            if refused.len() > 5 { ", …" } else { "" }
        );
    }
    Ok(msg)
}

/// Receive handheld files and folders into a local folder.
#[tauri::command]
pub fn download(app: AppHandle, paths: Vec<String>, folder: String) -> CmdResult<u64> {
    let title = match paths.as_slice() {
        [one] => format!("Receiving {}", basename(one)),
        many => format!("Receiving {} items", many.len()),
    };
    jobs::submit(&app, title, move |ctx| {
        let app = ctx.app();
        // list everything first so progress can count bytes
        let mut files = Vec::new();
        for p in &paths {
            let st = device::run(app, |c| c.stat(p))?.ok_or_else(|| CmdError::new("notFound", format!("{p} doesn't exist")))?;
            let base = calcbridge_link::services::dirname(p);
            if st.is_dir {
                files.push((st.clone(), base.clone()));
                for e in device::run(app, |c| c.walk(p))? {
                    files.push((e, base.clone()));
                }
            } else {
                files.push((st, base));
            }
        }
        let total: u64 = files.iter().filter(|(e, _)| !e.is_dir).map(|(e, _)| e.size).sum::<u64>().max(1);
        let mut done_bytes = 0;
        let mut count = 0;
        for (e, base) in &files {
            device::run(app, |c| c.check_cancel())?;
            let rel = e.path.strip_prefix(base.trim_end_matches('/')).unwrap_or(&e.path);
            let local = calcbridge_link::calculator::local_path(Path::new(&folder), rel);
            if e.is_dir {
                std::fs::create_dir_all(&local)?;
                continue;
            }
            let start = done_bytes;
            let data = device::run(app, |c| c.read_file(&e.path, Some(&mut |d, _| ctx.progress(start + d, total, &e.path))))?;
            if let Some(parent) = local.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&local, data)?;
            done_bytes += e.size;
            count += 1;
        }
        Ok(format!("Received {} into {folder}", plural(count, "file")))
    })
}

#[tauri::command]
pub fn backup(app: AppHandle, folder: String) -> CmdResult<u64> {
    jobs::submit(&app, "Backing up the handheld", move |ctx| {
        let n = device::run(ctx.app(), |c| {
            c.backup(Path::new(&folder), Some(&mut |p: &str, i, t| ctx.progress(i as u64, t as u64, p)))
        })?;
        Ok(format!("Backed up {} to {folder}", plural(n, "file")))
    })
}

#[tauri::command]
pub fn restore(app: AppHandle, folder: String, target: String, replace: bool) -> CmdResult<u64> {
    jobs::submit(&app, "Restoring a backup", move |ctx| {
        let mut skipped = Vec::new();
        let n = device::run(ctx.app(), |c| {
            c.restore(
                Path::new(&folder),
                &target,
                replace,
                Some(&mut |p: &str, i, t| ctx.progress(i as u64, t as u64, p)),
                Some(&mut skipped),
            )
        })?;
        let mut msg = format!("Restored {}", plural(n, "document"));
        if !skipped.is_empty() {
            msg += &format!(" ({} skipped: already there, or not a document)", skipped.len());
        }
        Ok(msg)
    })
}

#[tauri::command]
pub async fn os_image_check(app: AppHandle, path: String) -> CmdResult<OsImageCheck> {
    blocking(move || {
        let info = device::run(&app, |c| c.info())?;
        let size = std::fs::metadata(&path)?.len();
        let matches = path.to_lowercase().ends_with(&info.os_extension.to_lowercase());
        Ok(OsImageCheck { size, wanted_extension: info.os_extension, matches, current_version: info.os_version })
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsImageCheck {
    size: u64,
    wanted_extension: String,
    matches: bool,
    current_version: String,
}

#[tauri::command]
pub fn install_os(app: AppHandle, path: String) -> CmdResult<u64> {
    jobs::submit(&app, "Installing an OS", move |ctx| {
        let image = std::fs::read(&path)?;
        device::run(ctx.app(), |c| {
            c.install_os(&image, Some(&mut |stage, d, t| {
                let detail = match stage {
                    OsStage::Upload => "Sending the OS image",
                    OsStage::Install => "The handheld is installing it",
                };
                // Upload is the first 90%, the handheld's own install the rest.
                let (done, total) = match stage {
                    OsStage::Upload => (d * 90 / t.max(1), 100),
                    OsStage::Install => (90 + d / 10, 100),
                };
                ctx.progress(done, total, detail);
            }))
        })?;
        if !app_is_simulated(ctx.app()) {
            device::os_restarting(ctx.app());
        }
        Ok("The handheld accepted the OS and is restarting".into())
    })
}

#[tauri::command]
pub fn cancel_job(app: AppHandle, id: u64) {
    jobs::cancel(&app, id);
}

// ------------------------------------------------------------- screen/keys
/// The screen as `width u16 LE, height u16 LE, bpp, 0`, then the raw pixels.
#[tauri::command]
pub async fn screenshot(app: AppHandle) -> CmdResult<Response> {
    blocking(move || {
        let shot = device::run(&app, |c| c.screenshot())?;
        let mut out = Vec::with_capacity(6 + shot.pixels.len());
        out.extend_from_slice(&shot.width.to_le_bytes());
        out.extend_from_slice(&shot.height.to_le_bytes());
        out.extend_from_slice(&[shot.bpp, 0]);
        out.extend_from_slice(&shot.pixels);
        *app.state::<Device>().last_shot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(shot);
        Ok(Response::new(out))
    })
    .await
}

/// Save the screenshot last shown.
#[tauri::command]
pub async fn save_screenshot(app: AppHandle, path: String) -> CmdResult<()> {
    blocking(move || {
        let shot = app.state::<Device>().last_shot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let shot = match shot {
            Some(s) => s,
            None => device::run(&app, |c| c.screenshot())?,
        };
        shot.save(Path::new(&path))?;
        log::info!("saved a screenshot to {path}");
        Ok(())
    })
    .await
}

/// Write an image the window made (a capture with its border and zoom) to a file.
#[tauri::command]
pub async fn save_png(path: String, base64: String) -> CmdResult<()> {
    blocking(move || {
        let bytes = decode_base64(&base64).ok_or_else(|| CmdError::new("invalid", "That image data is damaged"))?;
        std::fs::write(&path, bytes)?;
        log::info!("saved {path}");
        Ok(())
    })
    .await
}

fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[tauri::command]
pub async fn press_key(app: AppHandle, key: String) -> CmdResult<()> {
    blocking(move || device::run(&app, |c| c.press(&key))).await
}

#[tauri::command]
pub async fn type_text(app: AppHandle, text: String) -> CmdResult<()> {
    blocking(move || device::run(&app, |c| c.type_text(&text))).await
}

// ------------------------------------------------------------- this computer
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    /// Seconds since 1970.
    modified: Option<u64>,
}

fn hidden(entry: &std::fs::DirEntry, meta: &std::fs::Metadata) -> bool {
    if entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if meta.file_attributes() & (HIDDEN | SYSTEM) != 0 {
            return true;
        }
    }
    #[cfg(not(windows))]
    let _ = meta;
    false
}

#[tauri::command]
pub async fn local_list(path: String, show_hidden: bool) -> CmdResult<Vec<LocalEntry>> {
    blocking(move || {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&path)? {
            let Ok(entry) = entry else { continue };
            let Ok(meta) = entry.metadata() else { continue };
            if !show_hidden && hidden(&entry, &meta) {
                continue;
            }
            out.push(LocalEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified: meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
            });
        }
        Ok(out)
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Places {
    home: String,
    documents: Option<String>,
    desktop: Option<String>,
    downloads: Option<String>,
    roots: Vec<String>,
    separator: String,
}

#[tauri::command]
pub fn local_places(app: AppHandle) -> Places {
    let paths = app.path();
    let s = |p: tauri::Result<PathBuf>| p.ok().filter(|p| p.is_dir()).map(|p| p.to_string_lossy().into_owned());
    let home = s(paths.home_dir()).unwrap_or_else(|| "/".into());
    let roots = if cfg!(windows) {
        (b'A'..=b'Z').map(|d| format!("{}:\\", d as char)).filter(|r| Path::new(r).is_dir()).collect()
    } else {
        vec!["/".into()]
    };
    Places {
        home,
        documents: s(paths.document_dir()),
        desktop: s(paths.desktop_dir()),
        downloads: s(paths.download_dir()),
        roots,
        separator: std::path::MAIN_SEPARATOR.to_string(),
    }
}

#[tauri::command]
pub async fn local_make_dir(path: String) -> CmdResult<()> {
    blocking(move || Ok(std::fs::create_dir(&path)?)).await
}

#[tauri::command]
pub async fn local_rename(path: String, new_name: String) -> CmdResult<String> {
    blocking(move || {
        let from = PathBuf::from(&path);
        let name = new_name.trim();
        if name.is_empty() || name.contains(['/', '\\']) {
            return Err(CmdError::new("invalid", "That isn't a valid name"));
        }
        let to = from.with_file_name(name);
        if to.exists() {
            return Err(CmdError::new("exists", format!("{} already exists", to.display())));
        }
        std::fs::rename(&from, &to)?;
        Ok(to.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
pub async fn local_exists(paths: Vec<String>) -> Vec<bool> {
    paths.iter().map(|p| Path::new(p).exists()).collect()
}

#[tauri::command]
pub async fn local_is_dir(paths: Vec<String>) -> Vec<bool> {
    paths.iter().map(|p| Path::new(p).is_dir()).collect()
}

#[tauri::command]
pub async fn write_text_file(path: String, content: String) -> CmdResult<()> {
    blocking(move || Ok(std::fs::write(&path, content)?)).await
}

#[tauri::command]
pub async fn read_text_file(path: String) -> CmdResult<String> {
    blocking(move || Ok(textio::read_text(Path::new(&path))?)).await
}

// ------------------------------------------------------------------ documents
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSpec {
    /// notes, python, lua or text
    kind: String,
    /// The document name without .tns
    name: String,
    content: String,
    /// Notes: color of the first line when it's a "# Title"
    #[serde(default)]
    title_color: Option<u32>,
}

/// Notes markup: one paragraph per line; "# " starts a bold title line; a line of "---" starts a
/// new page; $…$ is math and **…** bold.
fn notes_pages(content: &str, title_color: u32) -> Vec<Vec<Paragraph>> {
    let mut pages = vec![Vec::new()];
    for line in content.replace("\r\n", "\n").split('\n') {
        if line.trim() == "---" {
            pages.push(Vec::new());
            continue;
        }
        let para = match line.strip_prefix("# ") {
            Some(title) => Paragraph::title(title, title_color),
            None => Paragraph::new(line),
        };
        pages.last_mut().expect("at least one page").push(para);
    }
    pages
}

fn build_document(spec: &DocSpec) -> CmdResult<Vec<u8>> {
    Ok(match spec.kind.as_str() {
        "notes" => build_notes(&notes_pages(&spec.content, spec.title_color.unwrap_or(0x1F5FD0)))?,
        "python" => build_python(&format!("{}.py", document::safe_identifier(&spec.name)), &spec.content, &[])?,
        "lua" => build_lua(&spec.content)?,
        "text" => build_text(&spec.content, None)?,
        other => return Err(CmdError::new("invalid", format!("unknown document kind {other}"))),
    })
}

fn document_name(name: &str) -> CmdResult<String> {
    let name = name.trim().trim_end_matches(".tns").trim();
    let file = format!("{name}.tns");
    check_name(&file)?;
    Ok(file)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPage {
    paragraphs: Vec<ViewParagraph>,
    /// Lines whose math doesn't parse: (line number, problem)
    errors: Vec<(usize, String)>,
}

/// How Notes markup will look: pages of paragraphs, math as LaTeX for KaTeX.
#[tauri::command]
pub fn preview_notes(content: String, title_color: Option<u32>) -> Vec<PreviewPage> {
    let color = title_color.unwrap_or(0x1F5FD0);
    let mut pages = vec![PreviewPage { paragraphs: Vec::new(), errors: Vec::new() }];
    for (n, line) in content.replace("\r\n", "\n").split('\n').enumerate() {
        if line.trim() == "---" {
            pages.push(PreviewPage { paragraphs: Vec::new(), errors: Vec::new() });
            continue;
        }
        let page = pages.last_mut().expect("at least one page");
        let (markup, title) = match line.strip_prefix("# ") {
            Some(t) => (t, true),
            None => (line, false),
        };
        match r2d::markup_to_view(markup) {
            Ok(mut para) => {
                if title {
                    for span in &mut para {
                        if let r2d::Span::Text { style, .. } = span {
                            style.bold = true;
                            style.color = Some(format!("#{color:06X}"));
                        }
                    }
                }
                page.paragraphs.push(para);
            }
            Err(e) => {
                page.errors.push((n + 1, e.0));
                page.paragraphs.push(vec![r2d::Span::Text { text: line.to_owned(), style: r2d::Style::default() }]);
            }
        }
    }
    pages
}

#[tauri::command]
pub async fn save_document(spec: DocSpec, path: String) -> CmdResult<()> {
    blocking(move || {
        let data = build_document(&spec)?;
        std::fs::write(&path, data)?;
        log::info!("saved {path}");
        Ok(())
    })
    .await
}

/// Build a document and send it to a handheld folder (made if it's missing). Returns its handheld path.
#[tauri::command]
pub async fn send_document(app: AppHandle, spec: DocSpec, folder: String, replace: bool) -> CmdResult<String> {
    blocking(move || {
        let file = document_name(&spec.name)?;
        let data = build_document(&spec)?;
        let path = join(&folder, &file);
        device::run(&app, |c| {
            if folder != "/" && !c.exists(&folder)? {
                c.mkdir(&folder, true)?;
            }
            c.write_file(&path, &data, None, replace)
        })?;
        log::info!("sent {path}");
        Ok(path)
    })
    .await
}

/// Install a Python program as a module in /PyLib, named after the module as the handheld requires.
#[tauri::command]
pub async fn install_module(app: AppHandle, spec: DocSpec, replace: bool) -> CmdResult<String> {
    blocking(move || {
        let module = document::safe_identifier(&spec.name);
        let spec = DocSpec { kind: "python".into(), name: module.clone(), ..spec };
        let data = build_document(&spec)?;
        let path = join("/PyLib", &format!("{module}.tns"));
        device::run(&app, |c| {
            if !c.exists("/PyLib")? {
                c.mkdir("/PyLib", false)?;
            }
            c.write_file(&path, &data, None, replace)
        })?;
        log::info!("installed the module {module} in /PyLib");
        Ok(path)
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesView {
    problem: usize,
    page: usize,
    paragraphs: Vec<ViewParagraph>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedCode {
    name: String,
    code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocView {
    name: String,
    size: usize,
    summary: String,
    pages: Vec<Page>,
    notes: Vec<NotesView>,
    python: Vec<NamedCode>,
    lua: Option<String>,
}

fn view_document(name: String, blob: &[u8]) -> CmdResult<DocView> {
    let doc = TnsDocument::from_bytes(blob)?;
    let notes = doc
        .notes_trees()
        .into_iter()
        .map(|(problem, page, tree)| match r2d::parse_r2d(&tree) {
            Ok(paragraphs) => NotesView { problem, page, paragraphs, error: None },
            Err(e) => NotesView { problem, page, paragraphs: Vec::new(), error: Some(e) },
        })
        .collect();
    Ok(DocView {
        name,
        size: blob.len(),
        summary: doc.summary(),
        pages: doc.pages(),
        notes,
        python: doc.python_files().into_iter().map(|(name, code)| NamedCode { name, code }).collect(),
        lua: doc.lua_script(),
    })
}

#[tauri::command]
pub async fn open_local_document(path: String) -> CmdResult<DocView> {
    blocking(move || {
        let blob = std::fs::read(&path)?;
        let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        view_document(name, &blob)
    })
    .await
}

#[tauri::command]
pub async fn open_handheld_document(app: AppHandle, path: String) -> CmdResult<DocView> {
    blocking(move || {
        let blob = device::run(&app, |c| c.read_file(&path, None))?;
        view_document(basename(&path).to_owned(), &blob)
    })
    .await
}

/// Save a document's Python files into a folder (names from the document are kept inside it).
#[tauri::command]
pub async fn save_code_files(folder: String, files: Vec<(String, String)>) -> CmdResult<Vec<String>> {
    blocking(move || {
        let mut saved = Vec::new();
        for (name, code) in files {
            if let Some(path) = textio::safe_join(Path::new(&folder), &name) {
                std::fs::write(&path, code)?;
                saved.push(path.to_string_lossy().into_owned());
            }
        }
        Ok(saved)
    })
    .await
}

/// A name that's safe on this computer for a handheld item.
#[tauri::command]
pub fn local_name_for(name: String) -> String {
    local_name(&name)
}
