//! Long operations (transfers, backups, OS install) run one at a time on a worker thread, report
//! progress to the window, and can be cancelled between items.
//!
//! A job belongs to the connection that was open when it was asked for: if that connection is gone
//! by the time the job (or any later step of it) runs, it stops instead of carrying on against
//! whatever calculator is connected now.

use std::cell::Cell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use calcbridge_link::Calculator;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::device;
use crate::error::{CmdError, CmdResult};

type Work = Box<dyn FnOnce(&JobCtx) -> CmdResult<String> + Send>;

struct Job {
    id: u64,
    title: String,
    /// The connection the job was asked for on.
    calc: Option<Arc<Calculator>>,
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

#[derive(Default)]
struct State {
    running: Option<(u64, Option<Arc<Calculator>>)>,
    cancelled: HashSet<u64>,
}

pub struct Jobs {
    tx: Mutex<Option<mpsc::Sender<Job>>>,
    next: AtomicU64,
    /// One lock for both, so a Cancel can't fall between "not running yet" and "running".
    state: Mutex<State>,
}

impl Default for Jobs {
    fn default() -> Self {
        Jobs { tx: Mutex::new(None), next: AtomicU64::new(1), state: Mutex::new(State::default()) }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn changed() -> CmdError {
    CmdError::new("cancelled", "The calculator was disconnected or changed, so this stopped")
}

pub struct JobCtx {
    app: AppHandle,
    id: u64,
    title: String,
    calc: Option<Arc<Calculator>>,
    last: Cell<Option<Instant>>,
}

impl JobCtx {
    /// Run a step on the job's own calculator, as long as it's still the open connection.
    pub fn run<T>(&self, f: impl FnOnce(&Calculator) -> calcbridge_link::Result<T>) -> CmdResult<T> {
        let calc = self.calc.as_ref().ok_or_else(CmdError::no_device)?;
        if !device::is_current(&self.app, calc) {
            return Err(changed());
        }
        device::run_on(&self.app, calc, f)
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

    pub fn app(&self) -> &AppHandle {
        &self.app
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
                run_job(&app, job);
            }
        })
        .expect("start the job worker");
}

fn run_job(app: &AppHandle, job: Job) {
    let jobs = app.state::<Jobs>();
    let gone = job.calc.as_ref().is_some_and(|c| !device::is_current(app, c));
    if let Some(calc) = &job.calc {
        calc.clear_cancel(); // a Cancel that arrived after the previous job ended
    }
    {
        let mut st = lock(&jobs.state);
        if st.cancelled.remove(&job.id) || gone {
            drop(st);
            let why = if gone { changed().message } else { "Cancelled".into() };
            emit(app, job.id, &job.title, "cancelled", 0, 0, "", &why, Some("cancelled"));
            return;
        }
        st.running = Some((job.id, job.calc.clone()));
    }
    log::info!("{}…", job.title);
    emit(app, job.id, &job.title, "running", 0, 0, "", "", None);
    let ctx = JobCtx { app: app.clone(), id: job.id, title: job.title.clone(), calc: job.calc, last: Cell::new(None) };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (job.work)(&ctx)))
        .unwrap_or_else(|_| Err(CmdError::new("app", "Something went wrong inside CalcBridge")));
    lock(&jobs.state).running = None;
    match outcome {
        Ok(message) => {
            log::info!("{}: {message}", job.title);
            emit(app, job.id, &job.title, "done", 1, 1, "", &message, None);
        }
        Err(e) if e.kind == "cancelled" => {
            log::info!("{}: {}", job.title, e.message);
            emit(app, job.id, &job.title, "cancelled", 0, 0, "", &e.message, Some("cancelled"));
        }
        Err(e) => {
            log::warn!("{} failed: {}", job.title, e.message);
            emit(app, job.id, &job.title, "failed", 0, 0, "", &e.message, Some(e.kind));
        }
    }
}

/// Queue a job on the open connection; returns its id at once.
pub fn submit(
    app: &AppHandle,
    title: impl Into<String>,
    work: impl FnOnce(&JobCtx) -> CmdResult<String> + Send + 'static,
) -> CmdResult<u64> {
    let jobs = app.state::<Jobs>();
    let id = jobs.next.fetch_add(1, Ordering::SeqCst);
    let title = title.into();
    let calc = device::calc(app).ok();
    emit(app, id, &title, "queued", 0, 0, "", "", None);
    let job = Job { id, title, calc, work: Box::new(work) };
    lock(&jobs.tx)
        .as_ref()
        .ok_or_else(|| CmdError::new("app", "The job worker isn't running"))?
        .send(job)
        .map_err(|_| CmdError::new("app", "The job worker stopped"))?;
    Ok(id)
}

pub fn cancel(app: &AppHandle, id: u64) {
    let jobs = app.state::<Jobs>();
    let mut st = lock(&jobs.state);
    match &st.running {
        Some((running, calc)) if *running == id => {
            if let Some(calc) = calc {
                calc.cancel();
            }
        }
        _ => {
            st.cancelled.insert(id);
        }
    }
}
