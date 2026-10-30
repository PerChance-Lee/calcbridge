class CalcError(Exception):
    """Base class for everything the protocol stack raises."""


class NoDeviceError(CalcError):
    """No calculator is connected (or it was unplugged)."""


class DeviceBusyError(CalcError):
    """The USB interface is held by another program."""


class LinkError(CalcError):
    """The calculator stopped answering on the link layer."""


class ProtocolError(CalcError):
    """The calculator sent something we don't understand."""


class RemoteError(CalcError):
    """The calculator refused a request (missing file, folder not empty, ...)."""

    def __init__(self, message, code=None):
        super().__init__(message)
        self.code = code


class NotFoundError(RemoteError):
    pass


class ExistsError(RemoteError):
    pass
