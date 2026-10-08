from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class Mode(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    MODE_UNSPECIFIED: _ClassVar[Mode]
    MODE_LOCKSTEP: _ClassVar[Mode]
    MODE_REALTIME: _ClassVar[Mode]

class OverrunPolicy(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    OVERRUN_POLICY_UNSPECIFIED: _ClassVar[OverrunPolicy]
    OVERRUN_POLICY_WARN: _ClassVar[OverrunPolicy]
    OVERRUN_POLICY_SLOW: _ClassVar[OverrunPolicy]

class EventKind(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    EVENT_KIND_UNSPECIFIED: _ClassVar[EventKind]
    EVENT_KIND_OVERRUN: _ClassVar[EventKind]
    EVENT_KIND_FIRMWARE_RESTARTED: _ClassVar[EventKind]
    EVENT_KIND_SIM_ERROR: _ClassVar[EventKind]
    EVENT_KIND_LINK_DOWN: _ClassVar[EventKind]
    EVENT_KIND_LINK_UP: _ClassVar[EventKind]
    EVENT_KIND_PILOT_CONNECTED: _ClassVar[EventKind]
    EVENT_KIND_PILOT_DISCONNECTED: _ClassVar[EventKind]
    EVENT_KIND_SESSION_ENDED: _ClassVar[EventKind]
    EVENT_KIND_VTX_CHANGED: _ClassVar[EventKind]
    EVENT_KIND_SERIAL_OVERFLOW: _ClassVar[EventKind]
MODE_UNSPECIFIED: Mode
MODE_LOCKSTEP: Mode
MODE_REALTIME: Mode
OVERRUN_POLICY_UNSPECIFIED: OverrunPolicy
OVERRUN_POLICY_WARN: OverrunPolicy
OVERRUN_POLICY_SLOW: OverrunPolicy
EVENT_KIND_UNSPECIFIED: EventKind
EVENT_KIND_OVERRUN: EventKind
EVENT_KIND_FIRMWARE_RESTARTED: EventKind
EVENT_KIND_SIM_ERROR: EventKind
EVENT_KIND_LINK_DOWN: EventKind
EVENT_KIND_LINK_UP: EventKind
EVENT_KIND_PILOT_CONNECTED: EventKind
EVENT_KIND_PILOT_DISCONNECTED: EventKind
EVENT_KIND_SESSION_ENDED: EventKind
EVENT_KIND_VTX_CHANGED: EventKind
EVENT_KIND_SERIAL_OVERFLOW: EventKind

class Empty(_message.Message):
    __slots__ = ()
    def __init__(self) -> None: ...

class HandshakeRequest(_message.Message):
    __slots__ = ("protocol_version",)
    PROTOCOL_VERSION_FIELD_NUMBER: _ClassVar[int]
    protocol_version: int
    def __init__(self, protocol_version: _Optional[int] = ...) -> None: ...

class HandshakeReply(_message.Message):
    __slots__ = ("protocol_version", "server_version")
    PROTOCOL_VERSION_FIELD_NUMBER: _ClassVar[int]
    SERVER_VERSION_FIELD_NUMBER: _ClassVar[int]
    protocol_version: int
    server_version: str
    def __init__(self, protocol_version: _Optional[int] = ..., server_version: _Optional[str] = ...) -> None: ...

class LoadRequest(_message.Message):
    __slots__ = ("quad_path", "seed", "mode", "open_loop_fc", "overrun_policy", "keep_alive")
    QUAD_PATH_FIELD_NUMBER: _ClassVar[int]
    SEED_FIELD_NUMBER: _ClassVar[int]
    MODE_FIELD_NUMBER: _ClassVar[int]
    OPEN_LOOP_FC_FIELD_NUMBER: _ClassVar[int]
    OVERRUN_POLICY_FIELD_NUMBER: _ClassVar[int]
    KEEP_ALIVE_FIELD_NUMBER: _ClassVar[int]
    quad_path: str
    seed: int
    mode: Mode
    open_loop_fc: bool
    overrun_policy: OverrunPolicy
    keep_alive: bool
    def __init__(self, quad_path: _Optional[str] = ..., seed: _Optional[int] = ..., mode: _Optional[_Union[Mode, str]] = ..., open_loop_fc: _Optional[bool] = ..., overrun_policy: _Optional[_Union[OverrunPolicy, str]] = ..., keep_alive: _Optional[bool] = ...) -> None: ...

class LoadReply(_message.Message):
    __slots__ = ("quad_name", "base_hz", "configurator_address")
    QUAD_NAME_FIELD_NUMBER: _ClassVar[int]
    BASE_HZ_FIELD_NUMBER: _ClassVar[int]
    CONFIGURATOR_ADDRESS_FIELD_NUMBER: _ClassVar[int]
    quad_name: str
    base_hz: int
    configurator_address: str
    def __init__(self, quad_name: _Optional[str] = ..., base_hz: _Optional[int] = ..., configurator_address: _Optional[str] = ...) -> None: ...

class Sticks(_message.Message):
    __slots__ = ("roll", "pitch", "yaw", "throttle", "aux")
    ROLL_FIELD_NUMBER: _ClassVar[int]
    PITCH_FIELD_NUMBER: _ClassVar[int]
    YAW_FIELD_NUMBER: _ClassVar[int]
    THROTTLE_FIELD_NUMBER: _ClassVar[int]
    AUX_FIELD_NUMBER: _ClassVar[int]
    roll: float
    pitch: float
    yaw: float
    throttle: float
    aux: _containers.RepeatedScalarFieldContainer[float]
    def __init__(self, roll: _Optional[float] = ..., pitch: _Optional[float] = ..., yaw: _Optional[float] = ..., throttle: _Optional[float] = ..., aux: _Optional[_Iterable[float]] = ...) -> None: ...

class RunRequest(_message.Message):
    __slots__ = ("seconds",)
    SECONDS_FIELD_NUMBER: _ClassVar[int]
    seconds: float
    def __init__(self, seconds: _Optional[float] = ...) -> None: ...

class StreamRequest(_message.Message):
    __slots__ = ("rate_hz",)
    RATE_HZ_FIELD_NUMBER: _ClassVar[int]
    rate_hz: int
    def __init__(self, rate_hz: _Optional[int] = ...) -> None: ...

class PilotInput(_message.Message):
    __slots__ = ("sticks", "state_rate_hz")
    STICKS_FIELD_NUMBER: _ClassVar[int]
    STATE_RATE_HZ_FIELD_NUMBER: _ClassVar[int]
    sticks: Sticks
    state_rate_hz: int
    def __init__(self, sticks: _Optional[_Union[Sticks, _Mapping]] = ..., state_rate_hz: _Optional[int] = ...) -> None: ...

class Vec3(_message.Message):
    __slots__ = ("x", "y", "z")
    X_FIELD_NUMBER: _ClassVar[int]
    Y_FIELD_NUMBER: _ClassVar[int]
    Z_FIELD_NUMBER: _ClassVar[int]
    x: float
    y: float
    z: float
    def __init__(self, x: _Optional[float] = ..., y: _Optional[float] = ..., z: _Optional[float] = ...) -> None: ...

class Quat(_message.Message):
    __slots__ = ("w", "x", "y", "z")
    W_FIELD_NUMBER: _ClassVar[int]
    X_FIELD_NUMBER: _ClassVar[int]
    Y_FIELD_NUMBER: _ClassVar[int]
    Z_FIELD_NUMBER: _ClassVar[int]
    w: float
    x: float
    y: float
    z: float
    def __init__(self, w: _Optional[float] = ..., x: _Optional[float] = ..., y: _Optional[float] = ..., z: _Optional[float] = ...) -> None: ...

class RadioLink(_message.Message):
    __slots__ = ("tx_enabled", "link_up", "lq_pct", "rssi_dbm")
    TX_ENABLED_FIELD_NUMBER: _ClassVar[int]
    LINK_UP_FIELD_NUMBER: _ClassVar[int]
    LQ_PCT_FIELD_NUMBER: _ClassVar[int]
    RSSI_DBM_FIELD_NUMBER: _ClassVar[int]
    tx_enabled: bool
    link_up: bool
    lq_pct: float
    rssi_dbm: float
    def __init__(self, tx_enabled: _Optional[bool] = ..., link_up: _Optional[bool] = ..., lq_pct: _Optional[float] = ..., rssi_dbm: _Optional[float] = ...) -> None: ...

class OsdFrame(_message.Message):
    __slots__ = ("seq", "time_s", "present", "cols", "rows", "cells")
    SEQ_FIELD_NUMBER: _ClassVar[int]
    TIME_S_FIELD_NUMBER: _ClassVar[int]
    PRESENT_FIELD_NUMBER: _ClassVar[int]
    COLS_FIELD_NUMBER: _ClassVar[int]
    ROWS_FIELD_NUMBER: _ClassVar[int]
    CELLS_FIELD_NUMBER: _ClassVar[int]
    seq: int
    time_s: float
    present: bool
    cols: int
    rows: int
    cells: _containers.RepeatedScalarFieldContainer[int]
    def __init__(self, seq: _Optional[int] = ..., time_s: _Optional[float] = ..., present: _Optional[bool] = ..., cols: _Optional[int] = ..., rows: _Optional[int] = ..., cells: _Optional[_Iterable[int]] = ...) -> None: ...

class Vtx(_message.Message):
    __slots__ = ("present", "band", "channel", "freq_mhz", "power_mw", "pit_mode")
    PRESENT_FIELD_NUMBER: _ClassVar[int]
    BAND_FIELD_NUMBER: _ClassVar[int]
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    FREQ_MHZ_FIELD_NUMBER: _ClassVar[int]
    POWER_MW_FIELD_NUMBER: _ClassVar[int]
    PIT_MODE_FIELD_NUMBER: _ClassVar[int]
    present: bool
    band: int
    channel: int
    freq_mhz: int
    power_mw: int
    pit_mode: bool
    def __init__(self, present: _Optional[bool] = ..., band: _Optional[int] = ..., channel: _Optional[int] = ..., freq_mhz: _Optional[int] = ..., power_mw: _Optional[int] = ..., pit_mode: _Optional[bool] = ...) -> None: ...

class State(_message.Message):
    __slots__ = ("time_s", "position_ned_m", "velocity_ned_mps", "attitude", "rate_frd_radps", "battery_voltage_v", "battery_current_a", "motor_rpm", "motor_cmd", "radio", "running", "overruns", "fc_restarts", "vtx", "serial_dropped_bytes")
    TIME_S_FIELD_NUMBER: _ClassVar[int]
    POSITION_NED_M_FIELD_NUMBER: _ClassVar[int]
    VELOCITY_NED_MPS_FIELD_NUMBER: _ClassVar[int]
    ATTITUDE_FIELD_NUMBER: _ClassVar[int]
    RATE_FRD_RADPS_FIELD_NUMBER: _ClassVar[int]
    BATTERY_VOLTAGE_V_FIELD_NUMBER: _ClassVar[int]
    BATTERY_CURRENT_A_FIELD_NUMBER: _ClassVar[int]
    MOTOR_RPM_FIELD_NUMBER: _ClassVar[int]
    MOTOR_CMD_FIELD_NUMBER: _ClassVar[int]
    RADIO_FIELD_NUMBER: _ClassVar[int]
    RUNNING_FIELD_NUMBER: _ClassVar[int]
    OVERRUNS_FIELD_NUMBER: _ClassVar[int]
    FC_RESTARTS_FIELD_NUMBER: _ClassVar[int]
    VTX_FIELD_NUMBER: _ClassVar[int]
    SERIAL_DROPPED_BYTES_FIELD_NUMBER: _ClassVar[int]
    time_s: float
    position_ned_m: Vec3
    velocity_ned_mps: Vec3
    attitude: Quat
    rate_frd_radps: Vec3
    battery_voltage_v: float
    battery_current_a: float
    motor_rpm: _containers.RepeatedScalarFieldContainer[float]
    motor_cmd: _containers.RepeatedScalarFieldContainer[float]
    radio: RadioLink
    running: bool
    overruns: int
    fc_restarts: int
    vtx: Vtx
    serial_dropped_bytes: int
    def __init__(self, time_s: _Optional[float] = ..., position_ned_m: _Optional[_Union[Vec3, _Mapping]] = ..., velocity_ned_mps: _Optional[_Union[Vec3, _Mapping]] = ..., attitude: _Optional[_Union[Quat, _Mapping]] = ..., rate_frd_radps: _Optional[_Union[Vec3, _Mapping]] = ..., battery_voltage_v: _Optional[float] = ..., battery_current_a: _Optional[float] = ..., motor_rpm: _Optional[_Iterable[float]] = ..., motor_cmd: _Optional[_Iterable[float]] = ..., radio: _Optional[_Union[RadioLink, _Mapping]] = ..., running: _Optional[bool] = ..., overruns: _Optional[int] = ..., fc_restarts: _Optional[int] = ..., vtx: _Optional[_Union[Vtx, _Mapping]] = ..., serial_dropped_bytes: _Optional[int] = ...) -> None: ...

class Event(_message.Message):
    __slots__ = ("time_s", "kind", "message")
    TIME_S_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    time_s: float
    kind: EventKind
    message: str
    def __init__(self, time_s: _Optional[float] = ..., kind: _Optional[_Union[EventKind, str]] = ..., message: _Optional[str] = ...) -> None: ...

class RadioLinkLoss(_message.Message):
    __slots__ = ()
    def __init__(self) -> None: ...

class Fault(_message.Message):
    __slots__ = ("radio_link_loss",)
    RADIO_LINK_LOSS_FIELD_NUMBER: _ClassVar[int]
    radio_link_loss: RadioLinkLoss
    def __init__(self, radio_link_loss: _Optional[_Union[RadioLinkLoss, _Mapping]] = ...) -> None: ...
