//! Real USB through nusb (pure Rust: WinUSB on Windows, IOKit on macOS, usbfs on Linux).
//!
//! The CX II binds to WinUSB on its own (Microsoft OS descriptors), so Windows needs no driver
//! install; Linux needs a udev rule for access as a normal user.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use futures_core::Stream;
use nusb::descriptors::TransferType;
use nusb::hotplug::HotplugEvent;
use nusb::transfer::{Buffer, Bulk, In, Out, TransferError};
use nusb::{Endpoint, Interface, MaybeFuture};
use serde::Serialize;

use crate::nnse::MAX_MESSAGE;
use crate::transport::{TI_VID, Transport, product_name};
use crate::{Error, Result};

/// A connected CX II.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsbDevice {
    pub product: String,
    pub serial: Option<String>,
    pub bus: String,
    pub address: u8,
}

fn is_cx2(d: &nusb::DeviceInfo) -> bool {
    d.vendor_id() == TI_VID && product_name(d.product_id()).is_some()
}

fn usb_error(what: &str, e: nusb::Error) -> Error {
    use nusb::ErrorKind;
    match e.kind() {
        ErrorKind::Busy | ErrorKind::PermissionDenied => Error::Busy(format!(
            "Can't open the calculator ({e}). Close other programs using it (TI software, another CalcBridge \
             window) and try again."
        )),
        _ => Error::NoDevice(format!("{what}: {e}")),
    }
}

/// Connected CX II calculators. Cheap enough to call on every hotplug event.
pub fn list() -> Result<Vec<UsbDevice>> {
    let devices = nusb::list_devices().wait().map_err(|e| usb_error("Can't list USB devices", e))?;
    Ok(devices
        .filter(is_cx2)
        .map(|d| UsbDevice {
            product: product_name(d.product_id()).unwrap_or_default().to_owned(),
            serial: d.serial_number().map(str::to_owned),
            bus: d.bus_id().to_owned(),
            address: d.device_address(),
        })
        .collect())
}

/// Call `on_change` from a background thread whenever a USB device comes or goes (then call
/// [`list`]). Disconnect events don't say which device left, so they all count.
pub fn watch(on_change: impl Fn() + Send + 'static) -> Result<()> {
    let mut watch = nusb::watch_devices().map_err(|e| usb_error("Can't watch for USB devices", e))?;
    std::thread::Builder::new().name("usb-hotplug".into()).spawn(move || {
        while let Some(event) = next_blocking(&mut watch) {
            match event {
                HotplugEvent::Connected(info) if !is_cx2(&info) => {}
                _ => on_change(),
            }
        }
    })?;
    Ok(())
}

/// Block this thread until the stream yields.
fn next_blocking<S: Stream + Unpin>(stream: &mut S) -> Option<S::Item> {
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match Pin::new(&mut *stream).poll_next(&mut cx) {
            Poll::Ready(item) => return item,
            Poll::Pending => std::thread::park(),
        }
    }
}

pub struct UsbTransport {
    ep_in: Endpoint<Bulk, In>,
    ep_out: Endpoint<Bulk, Out>,
    _interface: Interface,
    /// Bytes received but not yet handed out.
    fifo: VecDeque<u8>,
    /// IN request size: a whole message, rounded up to whole packets as nusb requires.
    in_len: usize,
    key: String,
    errors_in_a_row: u32,
    debug: bool,
}

impl UsbTransport {
    /// The first connected CX II, or the one with this serial number.
    pub fn open(serial: Option<&str>) -> Result<UsbTransport> {
        let info = nusb::list_devices()
            .wait()
            .map_err(|e| usb_error("Can't list USB devices", e))?
            .filter(is_cx2)
            .find(|d| serial.is_none_or(|s| d.serial_number() == Some(s)))
            .ok_or_else(|| Error::NoDevice("No TI-Nspire CX II is connected".into()))?;
        let key = info.serial_number().map_or_else(|| "default".to_owned(), str::to_owned);
        let device = info.open().wait().map_err(|e| usb_error("Can't open the calculator", e))?;
        // A vendor-class device may come up unconfigured on macOS/Linux. (Windows configures it
        // itself, and nusb can't set a configuration there.)
        if !cfg!(windows) && device.active_configuration().is_err() {
            let _ = device.set_configuration(1).wait();
        }
        let interface = device.claim_interface(0).wait().map_err(|e| usb_error("Can't claim the calculator", e))?;
        let (mut in_addr, mut out_addr) = (0x81, 0x01);
        if let Some(desc) = interface.descriptor() {
            for ep in desc.endpoints().filter(|ep| ep.transfer_type() == TransferType::Bulk) {
                if ep.address() & 0x80 != 0 {
                    in_addr = ep.address();
                } else {
                    out_addr = ep.address();
                }
            }
        }
        let ep_in = interface.endpoint::<Bulk, In>(in_addr).map_err(|e| usb_error("No bulk IN endpoint", e))?;
        let ep_out = interface.endpoint::<Bulk, Out>(out_addr).map_err(|e| usb_error("No bulk OUT endpoint", e))?;
        let packet = ep_in.max_packet_size().max(1);
        log::info!("opened {} (serial {key}), max packet {packet}", product_name(info.product_id()).unwrap_or("?"));
        if std::env::var_os("CALCBRIDGE_USB_DEBUG").is_some() {
            eprintln!("usb: speed {:?}, in {in_addr:02x} out {out_addr:02x}, max packet {packet}", device.speed());
        }
        Ok(UsbTransport {
            ep_in,
            ep_out,
            _interface: interface,
            fifo: VecDeque::new(),
            in_len: MAX_MESSAGE.div_ceil(packet) * packet,
            key,
            errors_in_a_row: 0,
            debug: std::env::var_os("CALCBRIDGE_USB_DEBUG").is_some(),
        })
    }

    fn take(&mut self, max: usize) -> Vec<u8> {
        let n = max.min(self.fifo.len());
        self.fifo.drain(..n).collect()
    }
}

fn gone(e: TransferError) -> Error {
    Error::NoDevice(match e {
        TransferError::Disconnected => "The calculator was unplugged".into(),
        e => format!("USB transfer failed: {e}"),
    })
}

impl Transport for UsbTransport {
    fn write(&mut self, data: &[u8], timeout: Duration) -> Result<()> {
        let c = self.ep_out.transfer_blocking(Buffer::from(data), timeout);
        match c.status {
            Ok(()) if c.actual_len == data.len() => Ok(()),
            Ok(()) => Err(Error::Link(format!("short USB write ({} of {} bytes)", c.actual_len, data.len()))),
            Err(TransferError::Cancelled) => Err(Error::Link("USB write timed out".into())),
            Err(e) => Err(gone(e)),
        }
    }

    fn read(&mut self, max: usize, timeout: Duration) -> Result<Option<Vec<u8>>> {
        if !self.fifo.is_empty() {
            return Ok(Some(self.take(max)));
        }
        // Keep one IN transfer pending across calls: a timeout doesn't cancel it, so a reply that
        // arrives late (say, the ack of a packet we already sent again) is still caught.
        if self.ep_in.pending() == 0 {
            self.ep_in.submit(Buffer::new(self.in_len));
        }
        let Some(c) = self.ep_in.wait_next_complete(timeout) else { return Ok(None) };
        let status = c.status;
        if self.debug {
            eprintln!("usb: in {} bytes {:?} {:02x?}", c.actual_len, status, &c.buffer[..c.buffer.len().min(16)]);
        }
        self.fifo.extend(&c.buffer[..c.actual_len.min(c.buffer.len())]);
        let mut buf = c.buffer;
        buf.clear();
        buf.set_requested_len(self.in_len);
        match status {
            Ok(()) => {
                self.errors_in_a_row = 0;
                self.ep_in.submit(buf);
            }
            Err(TransferError::Disconnected) => return Err(gone(TransferError::Disconnected)),
            Err(e) => {
                // Windows reports some transient conditions as stalls or faults; only a run of them
                // means the calculator is gone.
                self.errors_in_a_row += 1;
                log::debug!("USB read error {e} ({} in a row)", self.errors_in_a_row);
                if self.errors_in_a_row > 3 {
                    return Err(gone(e));
                }
                if e == TransferError::Stall {
                    let _ = self.ep_in.clear_halt().wait();
                }
                self.ep_in.submit(buf);
            }
        }
        if self.fifo.is_empty() { Ok(None) } else { Ok(Some(self.take(max))) }
    }

    fn device_key(&self) -> String {
        self.key.clone()
    }

    fn close(&mut self) {
        self.ep_in.cancel_all();
        while self.ep_in.pending() > 0 {
            if self.ep_in.wait_next_complete(Duration::from_millis(500)).is_none() {
                break;
            }
        }
    }
}
