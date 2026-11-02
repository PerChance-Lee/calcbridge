//! High-level calculator API used by the app and the command line.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::navnet::Session;
use crate::nnse::{Link, LinkConfig, SeqStore, Trace};
use crate::services::{self, DeviceInfo, DirEntry, OsStage, Screenshot, basename, dirname, join};
use crate::transport::Transport;
use crate::{Error, RemoteKind, Result, keys, paths};

/// Characters the handheld refuses in names, or that break its file browser.
const FORBIDDEN_CHARS: &str = "\\/:*?\"<>|";
const MAX_NAME: usize = 255;

pub fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(Error::Invalid("The name can't be empty".into()));
    }
    let mut bad: Vec<char> = name.chars().filter(|&c| FORBIDDEN_CHARS.contains(c) || u32::from(c) < 32).collect();
    bad.sort_unstable();
    bad.dedup();
    if !bad.is_empty() {
        let shown: Vec<String> = bad.iter().map(|c| c.escape_default().to_string()).collect();
        return Err(Error::Invalid(format!("The name can't contain {}", shown.join(" "))));
    }
    if name.len() > MAX_NAME {
        return Err(Error::Invalid("The name is too long".into()));
    }
    Ok(())
}

const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A handheld name made safe for the PC's file system (Windows is the strictest).
pub fn local_name(name: &str) -> String {
    let swapped: String =
        name.chars().map(|c| if "<>:\"\\/|?*".contains(c) || u32::from(c) < 32 { '_' } else { c }).collect();
    let trimmed = swapped.trim_end_matches([' ', '.']);
    let mut s = if trimmed.is_empty() { "_".to_owned() } else { trimmed.to_owned() };
    let stem = s.split('.').next().unwrap_or_default().to_uppercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        s.insert(0, '_');
    }
    s
}

/// The handheld only accepts .tns documents (others are refused with FF 15).
pub fn is_sendable(name: &str) -> bool {
    name.to_lowercase().ends_with(".tns")
}

/// OS clutter that shouldn't be reported as "skipped".
pub fn is_junk(name: &str) -> bool {
    name.starts_with('.') || matches!(name.to_lowercase().as_str(), "desktop.ini" | "thumbs.db")
}

fn inside(path: &str, folder: &str) -> bool {
    let (path, folder) = (path.trim_end_matches('/'), folder.trim_end_matches('/'));
    path == folder || path.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
}

/// Where a handheld path lands below a local folder.
pub fn local_path(dir: &Path, remote: &str) -> PathBuf {
    let mut p = dir.to_path_buf();
    for part in remote.split('/').filter(|s| !s.is_empty()) {
        p.push(local_name(part));
    }
    p
}

fn not_found(path: &str) -> Error {
    Error::remote(RemoteKind::NotFound, format!("{path} doesn't exist"), None)
}

fn exists(path: &str) -> Error {
    Error::remote(RemoteKind::Exists, format!("{path} already exists"), None)
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Every file below `dir`, folder by folder like `os.walk`, names sorted; symlinks aren't followed.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    let mut dirs = Vec::new();
    for e in entries {
        if e.file_type()?.is_dir() {
            dirs.push(e.path());
        } else {
            out.push(e.path());
        }
    }
    for d in dirs {
        collect_files(&d, out)?;
    }
    Ok(())
}

pub struct Options {
    /// Where to remember the sequence number between runs; `None` disables it.
    pub state_file: Option<PathBuf>,
    pub trace: Option<Trace>,
    pub link: LinkConfig,
}

impl Default for Options {
    fn default() -> Self {
        Options { state_file: Some(paths::seq_state_file()), trace: None, link: LinkConfig::default() }
    }
}

/// One open connection. Calls are serialised, so it can be shared between threads.
pub struct Calculator {
    session: Mutex<Session>,
    cancel: AtomicBool,
    key: String,
}

impl Calculator {
    pub fn open(
        transport: Box<dyn Transport>,
        opts: Options,
        progress: Option<&mut (dyn FnMut(usize, usize) + '_)>,
    ) -> Result<Calculator> {
        let key = transport.device_key();
        let link = Link::new(transport, SeqStore::new(opts.state_file), opts.trace, opts.link);
        let mut session = Session::new(link);
        session.open(progress)?; // on failure, dropping the session closes the transport
        Ok(Calculator { session: Mutex::new(session), cancel: AtomicBool::new(false), key })
    }

    /// Open the first connected CX II, or the one with this serial number.
    #[cfg(feature = "usb")]
    pub fn open_usb(
        serial: Option<&str>,
        opts: Options,
        progress: Option<&mut (dyn FnMut(usize, usize) + '_)>,
    ) -> Result<Calculator> {
        Self::open(Box::new(crate::usb::UsbTransport::open(serial)?), opts, progress)
    }

    pub fn device_key(&self) -> &str {
        &self.key
    }

    /// Save the link state and release the device.
    pub fn close(self) {
        self.session().link().close();
    }

    // ---------------------------------------------------------- plumbing
    fn session(&self) -> MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Run raw service calls (see [`crate::services`]), re-opening once if the calculator restarted the link.
    pub fn with_session<T>(&self, mut f: impl FnMut(&mut Session) -> Result<T>) -> Result<T> {
        let mut s = self.session();
        match f(&mut s) {
            Err(Error::LinkReset) => {
                log::info!("link restarted by the calculator; reopening and retrying once");
                s.open(None)?;
                f(&mut s)
            }
            r => r,
        }
    }

    /// Ask the running operation to stop at its next item. One-shot: the stop consumes it.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Forget a cancel request that arrived after its operation finished.
    pub fn clear_cancel(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }

    pub fn check_cancel(&self) -> Result<()> {
        if self.cancel.swap(false, Ordering::SeqCst) { Err(Error::Cancelled) } else { Ok(()) }
    }

    // ------------------------------------------------------------- basics
    pub fn info(&self) -> Result<DeviceInfo> {
        self.with_session(services::device_info)
    }

    pub fn screenshot(&self) -> Result<Screenshot> {
        self.with_session(services::screenshot)
    }

    pub fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        self.with_session(|s| services::list_dir(s, path))
    }

    pub fn stat(&self, path: &str) -> Result<Option<DirEntry>> {
        self.with_session(|s| services::stat(s, path))
    }

    pub fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.stat(path)?.is_some())
    }

    pub fn read_file(&self, path: &str, mut progress: Option<&mut (dyn FnMut(u64, u64) + '_)>) -> Result<Vec<u8>> {
        self.with_session(|s| services::read_file(s, path, progress.as_deref_mut()))
    }

    pub fn write_file(
        &self,
        path: &str,
        data: &[u8],
        mut progress: Option<&mut (dyn FnMut(u64, u64) + '_)>,
        overwrite: bool,
    ) -> Result<()> {
        check_name(basename(path))?;
        if !overwrite && self.exists(path)? {
            return Err(exists(path));
        }
        self.with_session(|s| services::write_file(s, path, data, progress.as_deref_mut()))
    }

    pub fn mkdir(&self, path: &str, parents: bool) -> Result<()> {
        check_name(basename(path))?;
        if parents {
            let parent = dirname(path);
            if parent != "/" && !self.exists(&parent)? {
                self.mkdir(&parent, true)?;
            }
            if self.exists(path)? {
                return Ok(());
            }
        }
        self.with_session(|s| services::make_dir(s, path))
    }

    /// Rename in place. Returns the new path.
    pub fn rename(&self, path: &str, new_name: &str) -> Result<String> {
        check_name(new_name)?;
        let dst = join(&dirname(path), new_name);
        self.move_to(path, &dst)?;
        Ok(dst)
    }

    pub fn move_to(&self, src: &str, dst: &str) -> Result<()> {
        if inside(dst, src) {
            return Err(Error::Invalid("A folder can't be moved into itself".into()));
        }
        if self.exists(dst)? {
            return Err(exists(dst));
        }
        self.with_session(|s| services::move_path(s, src, dst))
    }

    /// Copy a file or a whole folder on the handheld.
    pub fn copy(&self, src: &str, dst: &str) -> Result<()> {
        if inside(dst, src) {
            return Err(Error::Invalid("A folder can't be copied into itself".into()));
        }
        if self.exists(dst)? {
            return Err(exists(dst));
        }
        let st = self.stat(src)?.ok_or_else(|| not_found(src))?;
        if !st.is_dir {
            return self.with_session(|s| services::copy_file(s, src, dst));
        }
        let children = self.list_dir(src)?; // list before creating dst, so dst can never show up in it
        self.with_session(|s| services::make_dir(s, dst))?;
        for e in children {
            self.check_cancel()?;
            self.copy(&e.path, &join(dst, &e.name))?;
        }
        Ok(())
    }

    /// Delete a file, or a folder and everything in it.
    pub fn delete(&self, path: &str) -> Result<()> {
        let st = self.stat(path)?.ok_or_else(|| not_found(path))?;
        if st.is_dir {
            for e in self.list_dir(path)? {
                self.check_cancel()?;
                self.delete(&e.path)?;
            }
            self.with_session(|s| services::remove_dir(s, path))
        } else {
            self.with_session(|s| services::delete_file(s, path))
        }
    }

    /// Every entry below `path`, folders before their contents.
    pub fn walk(&self, path: &str) -> Result<Vec<DirEntry>> {
        let mut out = Vec::new();
        self.walk_into(path, &mut out)?;
        Ok(out)
    }

    fn walk_into(&self, path: &str, out: &mut Vec<DirEntry>) -> Result<()> {
        for e in self.list_dir(path)? {
            let folder = e.is_dir.then(|| e.path.clone());
            out.push(e);
            if let Some(folder) = folder {
                self.walk_into(&folder, out)?;
            }
        }
        Ok(())
    }

    /// (bytes used, bytes total) of the handheld's storage.
    pub fn usage(&self) -> Result<(u64, u64)> {
        let i = self.info()?;
        Ok((i.flash_total.saturating_sub(i.flash_free), i.flash_total))
    }

    // ------------------------------------------------------------- keys
    pub fn press_key(&self, code: u32) -> Result<()> {
        self.with_session(|s| keys::press(s, code))
    }

    pub fn press(&self, name: &str) -> Result<()> {
        let code = keys::key_code(name).ok_or_else(|| Error::Invalid(format!("unknown key {name:?}")))?;
        self.press_key(code)
    }

    pub fn type_text(&self, text: &str) -> Result<()> {
        let codes = keys::text_codes(text).map_err(|c| Error::Invalid(format!("can't type {c:?}")))?;
        for code in codes {
            self.press_key(code)?;
        }
        Ok(())
    }

    // ------------------------------------------------------- transfers
    /// Download a file or folder to a local path. Returns the bytes received.
    pub fn download(&self, remote: &str, local: &Path, mut progress: Option<&mut (dyn FnMut(u64, u64) + '_)>) -> Result<u64> {
        let st = self.stat(remote)?.ok_or_else(|| not_found(remote))?;
        if !st.is_dir {
            let data = self.read_file(remote, progress.as_deref_mut())?;
            if let Some(parent) = local.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent)?;
            }
            fs::write(local, &data)?;
            return Ok(data.len() as u64);
        }
        fs::create_dir_all(local)?;
        let mut total = 0;
        for e in self.list_dir(remote)? {
            self.check_cancel()?;
            total += self.download(&e.path, &local.join(local_name(&e.name)), progress.as_deref_mut())?;
        }
        Ok(total)
    }

    /// Upload a file or folder into `remote_folder`. Returns the remote path.
    ///
    /// The handheld only accepts .tns documents, so inside folders anything else (desktop.ini,
    /// .DS_Store, stray .py files, ...) is left out and listed in `skipped`.
    pub fn upload(
        &self,
        local: &Path,
        remote_folder: &str,
        mut progress: Option<&mut (dyn FnMut(u64, u64) + '_)>,
        overwrite: bool,
        name: Option<&str>,
        mut skipped: Option<&mut Vec<PathBuf>>,
    ) -> Result<String> {
        let name = match name {
            Some(n) => n.to_owned(),
            None => file_name(local),
        };
        if name.is_empty() {
            return Err(Error::Invalid(format!("Can't send {}", local.display())));
        }
        let dst = join(remote_folder, &name);
        if local.is_dir() {
            if !self.exists(&dst)? {
                self.mkdir(&dst, false)?;
            }
            let mut children = fs::read_dir(local)?.collect::<std::io::Result<Vec<_>>>()?;
            children.sort_by_key(|e| e.file_name());
            for child in children {
                self.check_cancel()?;
                let path = child.path();
                let child_name = child.file_name().to_string_lossy().into_owned();
                if !child.file_type()?.is_dir() && !is_sendable(&child_name) {
                    if let Some(list) = skipped.as_deref_mut() {
                        if !is_junk(&child_name) {
                            list.push(path);
                        }
                    }
                    continue;
                }
                self.upload(&path, &dst, progress.as_deref_mut(), overwrite, None, skipped.as_deref_mut())?;
            }
            return Ok(dst);
        }
        if !is_sendable(&name) {
            return Err(Error::Invalid(format!("{name}: the handheld only accepts TI-Nspire documents (.tns)")));
        }
        let data = fs::read(local)?;
        self.write_file(&dst, &data, progress, overwrite)?;
        Ok(dst)
    }

    /// Copy everything on the handheld into `local_dir`. Returns the number of files.
    pub fn backup(&self, local_dir: &Path, mut progress: Option<&mut (dyn FnMut(&str, usize, usize) + '_)>) -> Result<usize> {
        let entries = self.walk("/")?;
        let files: Vec<&DirEntry> = entries.iter().filter(|e| !e.is_dir).collect();
        for e in entries.iter().filter(|e| e.is_dir) {
            fs::create_dir_all(local_path(local_dir, &e.path))?;
        }
        fs::create_dir_all(local_dir)?;
        for (n, e) in files.iter().enumerate() {
            self.check_cancel()?;
            if let Some(p) = progress.as_deref_mut() {
                p(&e.path, n + 1, files.len());
            }
            let data = self.read_file(&e.path, None)?;
            fs::write(local_path(local_dir, &e.path), data)?;
        }
        Ok(files.len())
    }

    /// Upload a backup folder's .tns documents into `remote_folder`. Returns how many were written.
    ///
    /// Files the handheld won't take (anything not .tns, e.g. the system's own themes.csv) and files
    /// that already exist when `overwrite` is false are listed in `skipped`.
    pub fn restore(
        &self,
        local_dir: &Path,
        remote_folder: &str,
        overwrite: bool,
        mut progress: Option<&mut (dyn FnMut(&str, usize, usize) + '_)>,
        mut skipped: Option<&mut Vec<PathBuf>>,
    ) -> Result<usize> {
        let mut all = Vec::new();
        collect_files(local_dir, &mut all)?;
        let mut files = Vec::new();
        for path in all {
            let name = file_name(&path);
            if is_sendable(&name) {
                files.push(path);
            } else if !is_junk(&name) {
                if let Some(list) = skipped.as_deref_mut() {
                    list.push(path);
                }
            }
        }
        let mut written = 0;
        for (n, path) in files.iter().enumerate() {
            self.check_cancel()?;
            let rel = path.strip_prefix(local_dir).unwrap_or(path);
            let parts: Vec<String> = rel
                .parent()
                .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            let folder = if parts.is_empty() { remote_folder.to_owned() } else { join(remote_folder, &parts.join("/")) };
            if folder != "/" && !self.exists(&folder)? {
                self.mkdir(&folder, true)?;
            }
            let dst = join(&folder, &file_name(path));
            if let Some(p) = progress.as_deref_mut() {
                p(&dst, n + 1, files.len());
            }
            if !overwrite && self.exists(&dst)? {
                if let Some(list) = skipped.as_deref_mut() {
                    list.push(path.clone());
                }
                continue;
            }
            let data = fs::read(path)?;
            self.with_session(|s| services::write_file(s, &dst, &data, None))?;
            written += 1;
        }
        Ok(written)
    }

    pub fn install_os(&self, image: &[u8], mut progress: Option<&mut (dyn FnMut(OsStage, u64, u64) + '_)>) -> Result<()> {
        if image.len() < 1024 {
            return Err(Error::Invalid("That file is too small to be an OS image".into()));
        }
        self.with_session(|s| services::install_os(s, image, progress.as_deref_mut()))
    }
}
