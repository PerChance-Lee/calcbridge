"""Raw bulk-transfer transports: real USB through pyusb/libusb, or anything with the same three methods."""
from __future__ import annotations

import logging
from dataclasses import dataclass

from .errors import DeviceBusyError, LinkError, NoDeviceError

log = logging.getLogger(__name__)

TI_VID = 0x0451
# Product IDs that speak NNSE (the CX II family). The original CX/CX CAS (0xE012) uses plain NavNet
# without the NNSE layer and isn't supported yet.
CX2_PIDS = {0xE022: "TI-Nspire CX II"}


class Transport:
    """Interface the link layer needs. read() returns None on timeout."""

    def write(self, data: bytes, timeout_ms: int = 1000) -> None:
        raise NotImplementedError

    def read(self, size: int, timeout_ms: int) -> bytes | None:
        raise NotImplementedError

    def close(self) -> None:
        pass

    @property
    def device_key(self) -> str:
        """Stable identifier for per-device state (sequence numbers)."""
        return "default"


def _backend():
    try:
        import libusb_package

        be = libusb_package.get_libusb1_backend()
        if be is not None:
            return be
    except Exception as e:  # pragma: no cover - depends on the platform wheel
        log.debug("libusb-package unavailable: %s", e)
    import usb.backend.libusb1

    return usb.backend.libusb1.get_backend()  # system libusb (e.g. Homebrew on macOS)


@dataclass
class UsbDeviceInfo:
    product: str
    bus: int
    address: int
    serial: str | None


def list_usb_devices() -> list[UsbDeviceInfo]:
    """Connected CX II calculators. Cheap enough to poll for hotplug."""
    import usb.core

    be = _backend()
    if be is None:
        raise NoDeviceError("libusb is not available on this computer")
    found = []
    for dev in usb.core.find(find_all=True, idVendor=TI_VID, backend=be) or []:
        if dev.idProduct in CX2_PIDS:
            serial = None
            try:
                serial = dev.serial_number if dev.iSerialNumber else None
            except Exception:  # reading strings needs the device open on some platforms
                pass
            found.append(UsbDeviceInfo(CX2_PIDS[dev.idProduct], dev.bus or 0, dev.address or 0, serial))
    return found


class UsbTransport(Transport):
    def __init__(self, serial: str | None = None):
        import usb.core
        import usb.util

        self._usb_core, self._usb_util = usb.core, usb.util
        be = _backend()
        if be is None:
            raise NoDeviceError("libusb is not available on this computer")
        devs = [d for d in (usb.core.find(find_all=True, idVendor=TI_VID, backend=be) or [])
                if d.idProduct in CX2_PIDS]
        if serial is not None:
            devs = [d for d in devs if _serial(d) == serial]
        if not devs:
            raise NoDeviceError("No TI-Nspire CX II is connected")
        self.dev = devs[0]
        self._serial = _serial(self.dev)
        try:
            try:
                self.dev.set_configuration(1)
            except usb.core.USBError:
                pass  # already configured; WinUSB refuses a redundant SET_CONFIGURATION
            usb.util.claim_interface(self.dev, 0)
        except usb.core.USBError as e:
            raise DeviceBusyError(
                f"Can't open the calculator ({e}). Close other programs using it (TI software, another "
                "CalcBridge window) and try again.") from e
        cfg = self.dev.get_active_configuration()
        intf = cfg[(0, 0)]
        self.ep_in = self.ep_out = None
        for ep in intf:
            if usb.util.endpoint_direction(ep.bEndpointAddress) == usb.util.ENDPOINT_IN:
                self.ep_in = self.ep_in or ep.bEndpointAddress
            else:
                self.ep_out = self.ep_out or ep.bEndpointAddress
        self.ep_in, self.ep_out = self.ep_in or 0x81, self.ep_out or 0x01

    @property
    def device_key(self) -> str:
        return self._serial or "default"

    def write(self, data: bytes, timeout_ms: int = 1000) -> None:
        try:
            n = self.dev.write(self.ep_out, data, timeout_ms)
        except self._usb_core.USBTimeoutError as e:
            raise LinkError("USB write timed out") from e
        except self._usb_core.USBError as e:
            raise NoDeviceError(f"USB write failed: {e}") from e
        if n != len(data):
            raise LinkError(f"short USB write ({n} of {len(data)} bytes)")

    def read(self, size: int, timeout_ms: int) -> bytes | None:
        try:
            return bytes(self.dev.read(self.ep_in, size, timeout_ms))
        except self._usb_core.USBTimeoutError:
            return None
        except self._usb_core.USBError as e:
            if getattr(e, "errno", None) in (60, 110, 10060) or "timed out" in str(e).lower():
                return None
            raise NoDeviceError(f"USB read failed: {e}") from e

    def close(self) -> None:
        try:
            self._usb_util.release_interface(self.dev, 0)
        except Exception:
            pass
        self._usb_util.dispose_resources(self.dev)


def _serial(dev) -> str | None:
    try:
        return dev.serial_number if dev.iSerialNumber else None
    except Exception:
        return None
