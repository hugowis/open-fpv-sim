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
MODE_UNSPECIFIED: Mode
MODE_LOCKSTEP: Mode

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
    __slots__ = ("quad_path", "seed", "mode", "open_loop_fc")
    QUAD_PATH_FIELD_NUMBER: _ClassVar[int]
    SEED_FIELD_NUMBER: _ClassVar[int]
    MODE_FIELD_NUMBER: _ClassVar[int]
    OPEN_LOOP_FC_FIELD_NUMBER: _ClassVar[int]
    quad_path: str
    seed: int
    mode: Mode
    open_loop_fc: bool
    def __init__(self, quad_path: _Optional[str] = ..., seed: _Optional[int] = ..., mode: _Optional[_Union[Mode, str]] = ..., open_loop_fc: _Optional[bool] = ...) -> None: ...

class LoadReply(_message.Message):
    __slots__ = ("quad_name", "base_hz")
    QUAD_NAME_FIELD_NUMBER: _ClassVar[int]
    BASE_HZ_FIELD_NUMBER: _ClassVar[int]
    quad_name: str
    base_hz: int
    def __init__(self, quad_name: _Optional[str] = ..., base_hz: _Optional[int] = ...) -> None: ...

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

class State(_message.Message):
    __slots__ = ("time_s", "position_ned_m", "velocity_ned_mps", "attitude", "rate_frd_radps", "battery_voltage_v", "battery_current_a", "motor_rpm", "motor_cmd")
    TIME_S_FIELD_NUMBER: _ClassVar[int]
    POSITION_NED_M_FIELD_NUMBER: _ClassVar[int]
    VELOCITY_NED_MPS_FIELD_NUMBER: _ClassVar[int]
    ATTITUDE_FIELD_NUMBER: _ClassVar[int]
    RATE_FRD_RADPS_FIELD_NUMBER: _ClassVar[int]
    BATTERY_VOLTAGE_V_FIELD_NUMBER: _ClassVar[int]
    BATTERY_CURRENT_A_FIELD_NUMBER: _ClassVar[int]
    MOTOR_RPM_FIELD_NUMBER: _ClassVar[int]
    MOTOR_CMD_FIELD_NUMBER: _ClassVar[int]
    time_s: float
    position_ned_m: Vec3
    velocity_ned_mps: Vec3
    attitude: Quat
    rate_frd_radps: Vec3
    battery_voltage_v: float
    battery_current_a: float
    motor_rpm: _containers.RepeatedScalarFieldContainer[float]
    motor_cmd: _containers.RepeatedScalarFieldContainer[float]
    def __init__(self, time_s: _Optional[float] = ..., position_ned_m: _Optional[_Union[Vec3, _Mapping]] = ..., velocity_ned_mps: _Optional[_Union[Vec3, _Mapping]] = ..., attitude: _Optional[_Union[Quat, _Mapping]] = ..., rate_frd_radps: _Optional[_Union[Vec3, _Mapping]] = ..., battery_voltage_v: _Optional[float] = ..., battery_current_a: _Optional[float] = ..., motor_rpm: _Optional[_Iterable[float]] = ..., motor_cmd: _Optional[_Iterable[float]] = ...) -> None: ...
