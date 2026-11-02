//! Long operations (transfers, backups, OS install) run one at a time on a worker thread, report
//! progress to the window, and can be cancelled between items.

use std::cell::Cell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::device;
use crate::error::{CmdError, CmdResult};

type Work = Box<dyn FnOnce(&JobCtx) -> CmdResult<String> + Send>;

struct Job {
    id: u64,
    title: String,
    work: Work,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    pub id: u64,
    pub title: String,
    /// queued, running, done, failed or cancelled
    pub state: &'static str,
    pub done: u64,
    pub total: u64,
    pub detail: String,
    pub message: String,
    pub error_kind: Option<&'static str>,
}

pub struct Jobs {
    tx: Mutex<Option<mpsc::Sender<Job>>>,
    next: AtomicU64,
    cancelled: Mutex<HashSet<u64>>,
    running: Mutex<Option<u64>>,
}

impl Default for Jobs {
    fn default() -> Self {
        Jobs { tx: Mutex::new(None), next: AtomicU64::new(1), cancelled: Mutex::new(HashSet::new()), running: Mutex::new(None) }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct JobCtx {
    app: AppHandle,
    id: u64,
    title: String,
    last: Cell<Option<Instant>>,
}

impl JobCtx {
    pub fn app(&self) -> &AppHandle {
        &self.app
    }

    /// Report progress; frequent calls are thinned out.
    pub fn progress(&self, done: u64, total: u64, detail: &str) {
        let now = Instant::now();
        if done < total && self.last.get().is_some_and(|t| now - t < Duration::from_millis(60)) {
            return;
        }
        self.last.set(Some(now));
        emit(&self.app, self.id, &self.title, "running", done, total, detail, "", None);
    }
}

#[allow(clippy::too_many_arguments)]
fn emit(
    app: &AppHandle,
    id: u64,
    title: &str,
    state: &'static str,
    done: u64,
    total: u64,
    detail: &str,
    message: &str,
    error_kind: Option<&'static str>,
) {
    let event = JobEvent {
        id,
        title: title.to_owned(),
        state,
        done,
        total,
        detail: detail.to_owned(),
        message: message.to_owned(),
        error_kind,
    };
    let _ = app.emit("job", event);
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<Job>();
    *lock(&app.state::<Jobs>().tx) = Some(tx);
    std::thread::Builder::new()
        .name("jobs".into())
        .spawn(move || {
            for job in rx {
                let jobs = app.state::<Jobs>();
                if lock(&jobs.cancelled).remove(&job.id) {
                    emit(&app, job.id, &job.title, "cancelled", 0, 0, "", "Cancelled", Some("cancelled"));
                    continue;
                }
                *lock(&jobs.running) = Some(job.id);
                if let Ok(calc) = device::calc(&app) {
                    calc.clear_cancel(); // a Cancel that arrived after the previous job ended
                }
                log::info!("{}…", job.title);
                emit(&app, job.id, &job.title, "running", 0, 0, "", "", None);
                let ctx = JobCtx { app: app.clone(), id: job.id, title: job.title.clone(), last: Cell::new(None) };
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (job.work)(&ctx)))
                    .unwrap_or_else(|_| Err(CmdError::new("app", "Something went wrong inside CalcBridge")));
                *lock(&jobs.running) = None;
                match outcome {
                    Ok(message) => {
                        log::info!("{}: {message}", job.title);
                        emit(&app, job.id, &job.title, "done", 1, 1, "", &message, None);
                    }
                    Err(e) if e.kind == "cancelled" => {
                        log::info!("{}: cancelled", job.title);
                        emit(&app, job.id, &job.title, "cancelled", 0, 0, "", "Cancelled", Some("cancelled"));
                    }
                    Err(e) => {
                        log::warn!("{} failed: {}", job.title, e.message);
                        emit(&app, job.id, &job.title, "failed", 0, 0, "", &e.message, Some(e.kind));
                    }
                }
            }
        })
        .expect("start the job worker");
}

/// Queue a job; returns its id at once.
pub fn submit(app: &AppHandle, title: impl Into<String>, work: impl FnOnce(&JobCtx) -> CmdResult<String> + Send + 'static) -> CmdResult<u64> {
    let jobs = app.state::<Jobs>();
    let id = jobs.next.fetch_add(1, Ordering::SeqCst);
    let title = title.into();
    emit(app, id, &title, "queued", 0, 0, "", "", None);
    let job = Job { id, title, work: Box::new(work) };
    lock(&jobs.tx)
        .as_ref()
        .ok_or_else(|| CmdError::new("app", "The job worker isn't running"))?
        .send(job)
        .map_err(|_| CmdError::new("app", "The job worker stopped"))?;
    Ok(id)
}

pub fn cancel(app: &AppHandle, id: u64) {
    let jobs = app.state::<Jobs>();
    if *lock(&jobs.running) == Some(id) {
        if let Ok(calc) = device::calc(app) {
            calc.cancel();
        }
    } else {
        lock(&jobs.cancelled).insert(id);
    }
}
