"""USB protocol stack for the TI-Nspire CX II.

Layers, bottom to top:
    transport  raw bulk transfers (pyusb/libusb, or the in-process simulator)
    nnse       "NavNet SE" link framing used by the CX II: handshake, sequence numbers, acks, retransmits
    navnet     NavNet packets and services (the protocol older Nspire models speak directly over USB)
    services   device info, file system, screenshots, OS install

The protocol knowledge comes from libnspire (https://github.com/Vogtinator/libnspire, GPL-3.0) and
hackspire.org; quirks marked "observed" were found on a real CX II running OS 5.4.
"""
