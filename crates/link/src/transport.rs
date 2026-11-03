use std::time::Duration;

use crate::Result;

pub const TI_VID: u16 = 0x0451;
/// Product IDs that speak NNSE (the CX II family). The original CX/CX CAS (0xE012) uses plain NavNet
/// without the NNSE layer and isn't supported yet.
pub const CX2_PIDS: &[(u16, &str)] = &[(0xE022, "TI-Nspire CX II")];

pub fn product_name(pid: u16) -> Option<&'static str> {
    CX2_PIDS.iter().find(|(p, _)| *p == pid).map(|(_, name)| *name)
}

/// Raw bulk transfers: real USB, the simulator, or anything else with the same methods.
/// Timeouts passed in are never zero.
pub trait Transport: Send {
    /// A timeout is `Error::Link` (the link layer treats it like a lost packet and retries);
    /// a vanished device is `Error::NoDevice`.
    fn write(&mut self, data: &[u8], timeout: Duration) -> Result<()>;

    /// Up to `max` bytes, or `None` on timeout.
    fn read(&mut self, max: usize, timeout: Duration) -> Result<Option<Vec<u8>>>;

    /// Stable identifier for per-device state (sequence numbers).
    fn device_key(&self) -> String {
        "default".into()
    }

    fn close(&mut self) {}
}
