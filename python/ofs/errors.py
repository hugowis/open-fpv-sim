"""Typed errors mapped from the server's `ofs-error-kind` metadata."""
import grpc


class OfsError(Exception):
    """Base class for Open FPV Sim errors."""


class ConfigError(OfsError):
    """The quad file is missing, unparsable, or invalid."""


class FirmwareCrashed(OfsError):
    """Betaflight SITL exited, hung, or sent garbage. The session is paused until the next load()."""


class NumericalError(OfsError):
    """A simulated signal became NaN or infinite. The session is paused until the next load()."""


class ProtocolMismatch(OfsError):
    """Client and server speak different protocol versions."""


class NotLoaded(OfsError):
    """No quad is loaded; call load() first."""


class InvalidArgument(OfsError, ValueError):
    """A request argument was out of range."""


class ServerUnavailable(OfsError):
    """The server could not be reached."""


_KINDS = {
    "config": ConfigError,
    "firmware": FirmwareCrashed,
    "numerical": NumericalError,
    "protocol": ProtocolMismatch,
    "not_loaded": NotLoaded,
    "invalid_argument": InvalidArgument,
}


def from_rpc_error(e: grpc.RpcError) -> OfsError:
    kind = dict(e.trailing_metadata() or ()).get("ofs-error-kind")
    if kind in _KINDS:
        return _KINDS[kind](e.details())
    if e.code() == grpc.StatusCode.UNAVAILABLE:
        return ServerUnavailable(e.details())
    return OfsError(f"{e.code().name}: {e.details()}")
