//! The connection: opening it (real or simulated), noticing a calculator being plugged in or pulled
//! out, and telling the window about every change.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use calcbridge_link::sim::{Sim, SimConfig};
use calcbridge_link::{Calculator, DeviceInfo, Options, Screenshot, usb};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{CmdError, CmdResult};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Status {
    /// No calculator is plugged in.
    Absent,
    /// One is plugged in but not connected.
    Present,
    Connecting { detail: String },
    Connected { info: DeviceInfo, simulated: bool },
    /// Waiting for the handheld to come back (it's restarting after an OS install).
    Waiting { detail: String },
    Failed { message: String },
}

/// Where an OS install's restart has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restart {
    None,
    /// Installed; waiting for the handheld to leave USB.
    Leaving,
    /// It left; reconnect when it comes back.
    Gone,
}

pub struct Device {
    calc: RwLock<Option<Arc<Calculator>>>,
    status: Mutex<Status>,
    sim: Mutex<Option<Sim>>,
    simulated: AtomicBool,
    auto_connect: AtomicBool,
    /// Don't auto-connect until the calculator is unplugged (after a Disconnect, or a failed try).
    hold_off: AtomicBool,
    restart: Mutex<Restart>,
    /// Serialises connect and disconnect.
    switching: Mutex<()>,
    /// The last screenshot shown, for "Save".
    pub last_shot: Mutex<Option<Screenshot>>,
}

impl Default for Device {
    fn default() -> Self {
        Device {
            calc: RwLock::new(None),
            status: Mutex::new(Status::Absent),
            sim: Mutex::new(None),
            simulated: AtomicBool::new(false),
            auto_connect: AtomicBool::new(true),
            hold_off: AtomicBool::new(false),
            restart: Mutex::new(Restart::None),
            switching: Mutex::new(()),
            last_shot: Mutex::new(None),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Device {
    pub fn status(&self) -> Status {
        lock(&self.status).clone()
    }

    fn current(&self) -> Option<Arc<Calculator>> {
        self.calc.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn is_simulated(&self) -> bool {
        self.simulated.load(Ordering::SeqCst)
    }

    pub fn set_auto_connect(&self, on: bool) {
        self.auto_connect.store(on, Ordering::SeqCst);
    }
}

fn set_status(app: &AppHandle, status: Status) {
    let dev = app.state::<Device>();
    *lock(&dev.status) = status.clone();
    let _ = app.emit("status", status);
}

fn plugged_in() -> bool {
    usb::list().map(|l| !l.is_empty()).unwrap_or(false)
}

/// The open calculator, or a "no device" error.
pub fn calc(app: &AppHandle) -> CmdResult<Arc<Calculator>> {
    app.state::<Device>().current().ok_or_else(CmdError::no_device)
}

/// Run `f` on the calculator. If the connection turns out to be dead, forget it and say so.
pub fn run<T>(app: &AppHandle, f: impl FnOnce(&Calculator) -> calcbridge_link::Result<T>) -> CmdResult<T> {
    let calc = calc(app)?;
    f(&calc).map_err(|e| {
        if e.is_disconnect() {
            lost(app, &calc, &e.to_string());
        }
        e.into()
    })
}

/// Forget `calc` if it's still the current connection. Returns whether it was.
fn forget(dev: &Device, calc: &Arc<Calculator>) -> bool {
    let mut slot = dev.calc.write().unwrap_or_else(PoisonError::into_inner);
    if !slot.as_ref().is_some_and(|c| Arc::ptr_eq(c, calc)) {
        return false;
    }
    *slot = None;
    true
}

/// The connection `calc` stopped working.
fn lost(app: &AppHandle, calc: &Arc<Calculator>, why: &str) {
    let dev = app.state::<Device>();
    if !forget(&dev, calc) {
        return; // already replaced
    }
    if *lock(&dev.restart) != Restart::None {
        return; // expected: the handheld is restarting after an OS install
    }
    log::warn!("connection lost: {why}");
    let status = if !dev.is_simulated() && !plugged_in() {
        Status::Absent
    } else {
        Status::Failed { message: format!("The connection stopped working: {why}") }
    };
    set_status(app, status);
}

/// Connect, at the user's request (`manual`) or because a calculator was plugged in.
pub fn connect(app: &AppHandle, simulated: bool, manual: bool) -> CmdResult<DeviceInfo> {
    let dev = app.state::<Device>();
    let _switching = lock(&dev.switching);
    if let Some(calc) = dev.current() {
        if dev.is_simulated() == simulated {
            return Ok(calc.info()?);
        }
    }
    if manual {
        dev.hold_off.store(false, Ordering::SeqCst);
        *lock(&dev.restart) = Restart::None;
    }
    close_current(&dev);
    set_status(app, Status::Connecting { detail: "Connecting…".into() });
    let opened = (|| -> CmdResult<(Calculator, DeviceInfo)> {
        let calc = if simulated {
            let sim = lock(&dev.sim)
                .get_or_insert_with(|| Sim::new(SimConfig { idle_drop: None, ..SimConfig::default() }))
                .clone();
            Calculator::open(Box::new(sim.transport("sim")), Options { state_file: None, ..Options::default() }, None)?
        } else {
            let progress_app = app.clone();
            let mut progress = move |done: usize, total: usize| {
                set_status(&progress_app, Status::Connecting { detail: format!("Finding the connection ({done} of {total})…") });
            };
            Calculator::open_usb(None, Options::default(), Some(&mut progress))?
        };
        let info = calc.info()?;
        Ok((calc, info))
    })();
    match opened {
        Ok((calc, info)) => {
            log::info!(
                "connected to {} \"{}\" (OS {}){}",
                info.model(),
                info.name,
                info.os_version,
                if simulated { ", simulated" } else { "" }
            );
            *dev.calc.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(calc));
            dev.simulated.store(simulated, Ordering::SeqCst);
            set_status(app, Status::Connected { info: info.clone(), simulated });
            Ok(info)
        }
        Err(e) => {
            log::warn!("couldn't connect: {}", e.message);
            if !manual {
                // don't keep knocking on a calculator that won't answer; a re-plug tries again
                dev.hold_off.store(true, Ordering::SeqCst);
            }
            set_status(app, Status::Failed { message: e.message.clone() });
            Err(e)
        }
    }
}

fn close_current(dev: &Device) {
    let taken = dev.calc.write().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(calc) = taken {
        // A job still holding it closes it when it lets go.
        if let Ok(calc) = Arc::try_unwrap(calc) {
            calc.close();
        }
    }
}

pub fn disconnect(app: &AppHandle) {
    let dev = app.state::<Device>();
    let _switching = lock(&dev.switching);
    close_current(&dev);
    *lock(&dev.restart) = Restart::None;
    let present = plugged_in();
    // stay disconnected until the user connects again or re-plugs the calculator
    dev.hold_off.store(present, Ordering::SeqCst);
    log::info!("disconnected");
    set_status(app, if present { Status::Present } else { Status::Absent });
}

/// The handheld accepted a new OS and is restarting: let the connection go and pick it up again
/// once the handheld has left USB and come back.
pub fn os_restarting(app: &AppHandle) {
    let dev = app.state::<Device>();
    *lock(&dev.restart) = Restart::Leaving;
    if let Ok(calc) = calc(app) {
        forget(&dev, &calc);
    }
    dev.hold_off.store(false, Ordering::SeqCst);
    log::info!("the handheld is restarting with its new OS");
    set_status(app, Status::Waiting { detail: "The handheld is installing its new OS and restarting. CalcBridge reconnects when it's back.".into() });
}

/// Refresh the device details shown in the sidebar.
pub fn refresh_info(app: &AppHandle) -> CmdResult<DeviceInfo> {
    let info = run(app, |c| c.info())?;
    let simulated = app.state::<Device>().is_simulated();
    set_status(app, Status::Connected { info: info.clone(), simulated });
    Ok(info)
}

/// Look at what's plugged in now: connect to a new calculator, or notice one that left.
fn reconsider(app: &AppHandle) {
    let dev = app.state::<Device>();
    let present = plugged_in();
    if !present {
        dev.hold_off.store(false, Ordering::SeqCst); // a re-plug may auto-connect again
    }
    {
        let mut restart = lock(&dev.restart);
        match (*restart, present) {
            (Restart::Leaving, false) => *restart = Restart::Gone,
            (Restart::Leaving, true) => return, // still rebooting
            (Restart::Gone, false) => return,
            (Restart::Gone, true) => *restart = Restart::None,
            (Restart::None, _) => {}
        }
    }
    match (dev.status(), dev.current()) {
        (Status::Connecting { .. }, _) => {}
        (_, Some(calc)) => {
            if !dev.is_simulated() && !present {
                lost(app, &calc, "the calculator was unplugged");
            }
        }
        (_, None) => {
            let auto = dev.auto_connect.load(Ordering::SeqCst) && !dev.hold_off.load(Ordering::SeqCst);
            if present && auto {
                let _ = connect(app, false, false);
            } else if !matches!(dev.status(), Status::Failed { .. }) || !present {
                set_status(app, if present { Status::Present } else { Status::Absent });
            }
        }
    }
}

/// Watch USB for calculators coming and going, and look once at startup (or start the simulator).
pub fn start(app: AppHandle, simulated: bool) {
    let (tx, rx) = mpsc::channel::<()>();
    if simulated {
        let sim_app = app.clone();
        std::thread::spawn(move || {
            let _ = connect(&sim_app, true, true);
        });
    } else {
        let _ = tx.send(());
    }
    let watcher = tx.clone();
    if let Err(e) = usb::watch(move || {
        let _ = watcher.send(());
    }) {
        log::warn!("can't watch USB for calculators ({e}); checking every few seconds instead");
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(3));
                if tx.send(()).is_err() {
                    break;
                }
            }
        });
    }
    std::thread::Builder::new()
        .name("device-watch".into())
        .spawn(move || {
            while rx.recv().is_ok() {
                // Windows announces a device before its interface is ready, and events come in bursts.
                std::thread::sleep(Duration::from_millis(600));
                while rx.try_recv().is_ok() {}
                reconsider(&app);
            }
        })
        .expect("start the device watcher");
}
