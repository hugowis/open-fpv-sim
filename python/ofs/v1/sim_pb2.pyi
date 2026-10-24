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

class VideoSync(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    VIDEO_SYNC_UNSPECIFIED: _ClassVar[VideoSync]
    VIDEO_SYNC_LOCKED: _ClassVar[VideoSync]
    VIDEO_SYNC_UNSTABLE: _ClassVar[VideoSync]
    VIDEO_SYNC_LOST: _ClassVar[VideoSync]

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
    EVENT_KIND_VIDEO_LOST: _ClassVar[EventKind]
    EVENT_KIND_VIDEO_RESTORED: _ClassVar[EventKind]
    EVENT_KIND_COLLISION: _ClassVar[EventKind]
    EVENT_KIND_FAULT_INJECTED: _ClassVar[EventKind]
    EVENT_KIND_FAULT_CLEARED: _ClassVar[EventKind]
    EVENT_KIND_INPUTS_STARTED: _ClassVar[EventKind]
    EVENT_KIND_INPUTS_ENDED: _ClassVar[EventKind]
    EVENT_KIND_RECORDING_STARTED: _ClassVar[EventKind]
    EVENT_KIND_RECORDING_STOPPED: _ClassVar[EventKind]
    EVENT_KIND_INPUTS_RECORDING_STARTED: _ClassVar[EventKind]
    EVENT_KIND_INPUTS_RECORDING_STOPPED: _ClassVar[EventKind]
    EVENT_KIND_GATE_PASSED: _ClassVar[EventKind]
    EVENT_KIND_LAP_COMPLETED: _ClassVar[EventKind]
MODE_UNSPECIFIED: Mode
MODE_LOCKSTEP: Mode
MODE_REALTIME: Mode
OVERRUN_POLICY_UNSPECIFIED: OverrunPolicy
OVERRUN_POLICY_WARN: OverrunPolicy
OVERRUN_POLICY_SLOW: OverrunPolicy
VIDEO_SYNC_UNSPECIFIED: VideoSync
VIDEO_SYNC_LOCKED: VideoSync
VIDEO_SYNC_UNSTABLE: VideoSync
VIDEO_SYNC_LOST: VideoSync
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
EVENT_KIND_VIDEO_LOST: EventKind
EVENT_KIND_VIDEO_RESTORED: EventKind
EVENT_KIND_COLLISION: EventKind
EVENT_KIND_FAULT_INJECTED: EventKind
EVENT_KIND_FAULT_CLEARED: EventKind
EVENT_KIND_INPUTS_STARTED: EventKind
EVENT_KIND_INPUTS_ENDED: EventKind
EVENT_KIND_RECORDING_STARTED: EventKind
EVENT_KIND_RECORDING_STOPPED: EventKind
EVENT_KIND_INPUTS_RECORDING_STARTED: EventKind
EVENT_KIND_INPUTS_RECORDING_STOPPED: EventKind
EVENT_KIND_GATE_PASSED: EventKind
EVENT_KIND_LAP_COMPLETED: EventKind

class Empty(_message.Message):
    __slots__ = ()
    def __init__(self) -> None: ...

class HandshakeRequest(_message.Message):
    __slots__ = ("protocol_version",)
    PROTOCOL_VERSION_FIELD_NUMBER: _ClassVar[int]
    protocol_version: int
    def __init__(self, protocol_version: _Optional[int] = ...) -> None: ...

class HandshakeReply(_message.Message):
    __slots__ = ("protocol_version", "server_version", "logs_dir")
    PROTOCOL_VERSION_FIELD_NUMBER: _ClassVar[int]
    SERVER_VERSION_FIELD_NUMBER: _ClassVar[int]
    LOGS_DIR_FIELD_NUMBER: _ClassVar[int]
    protocol_version: int
    server_version: str
    logs_dir: str
    def __init__(self, protocol_version: _Optional[int] = ..., server_version: _Optional[str] = ..., logs_dir: _Optional[str] = ...) -> None: ...

class LoadRequest(_message.Message):
    __slots__ = ("quad_path", "seed", "mode", "open_loop_fc", "overrun_policy", "keep_alive", "world_path", "stabilize")
    QUAD_PATH_FIELD_NUMBER: _ClassVar[int]
    SEED_FIELD_NUMBER: _ClassVar[int]
    MODE_FIELD_NUMBER: _ClassVar[int]
    OPEN_LOOP_FC_FIELD_NUMBER: _ClassVar[int]
    OVERRUN_POLICY_FIELD_NUMBER: _ClassVar[int]
    KEEP_ALIVE_FIELD_NUMBER: _ClassVar[int]
    WORLD_PATH_FIELD_NUMBER: _ClassVar[int]
    STABILIZE_FIELD_NUMBER: _ClassVar[int]
    quad_path: str
    seed: int
    mode: Mode
    open_loop_fc: bool
    overrun_policy: OverrunPolicy
    keep_alive: bool
    world_path: str
    stabilize: bool
    def __init__(self, quad_path: _Optional[str] = ..., seed: _Optional[int] = ..., mode: _Optional[_Union[Mode, str]] = ..., open_loop_fc: _Optional[bool] = ..., overrun_policy: _Optional[_Union[OverrunPolicy, str]] = ..., keep_alive: _Optional[bool] = ..., world_path: _Optional[str] = ..., stabilize: _Optional[bool] = ...) -> None: ...

class LoadReply(_message.Message):
    __slots__ = ("quad_name", "base_hz", "configurator_address", "parts")
    QUAD_NAME_FIELD_NUMBER: _ClassVar[int]
    BASE_HZ_FIELD_NUMBER: _ClassVar[int]
    CONFIGURATOR_ADDRESS_FIELD_NUMBER: _ClassVar[int]
    PARTS_FIELD_NUMBER: _ClassVar[int]
    quad_name: str
    base_hz: int
    configurator_address: str
    parts: _containers.RepeatedCompositeFieldContainer[PartInfo]
    def __init__(self, quad_name: _Optional[str] = ..., base_hz: _Optional[int] = ..., configurator_address: _Optional[str] = ..., parts: _Optional[_Iterable[_Union[PartInfo, _Mapping]]] = ...) -> None: ...

class PartInfo(_message.Message):
    __slots__ = ("kind", "id", "name", "provenance")
    KIND_FIELD_NUMBER: _ClassVar[int]
    ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    PROVENANCE_FIELD_NUMBER: _ClassVar[int]
    kind: str
    id: str
    name: str
    provenance: _containers.RepeatedCompositeFieldContainer[ParamProvenance]
    def __init__(self, kind: _Optional[str] = ..., id: _Optional[str] = ..., name: _Optional[str] = ..., provenance: _Optional[_Iterable[_Union[ParamProvenance, _Mapping]]] = ...) -> None: ...

class ParamProvenance(_message.Message):
    __slots__ = ("path", "source", "link", "note")
    PATH_FIELD_NUMBER: _ClassVar[int]
    SOURCE_FIELD_NUMBER: _ClassVar[int]
    LINK_FIELD_NUMBER: _ClassVar[int]
    NOTE_FIELD_NUMBER: _ClassVar[int]
    path: str
    source: str
    link: str
    note: str
    def __init__(self, path: _Optional[str] = ..., source: _Optional[str] = ..., link: _Optional[str] = ..., note: _Optional[str] = ...) -> None: ...

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
    __slots__ = ("tx_enabled", "link_up", "lq_pct", "rssi_dbm", "snr_db", "active_antenna", "downlink_lq_pct")
    TX_ENABLED_FIELD_NUMBER: _ClassVar[int]
    LINK_UP_FIELD_NUMBER: _ClassVar[int]
    LQ_PCT_FIELD_NUMBER: _ClassVar[int]
    RSSI_DBM_FIELD_NUMBER: _ClassVar[int]
    SNR_DB_FIELD_NUMBER: _ClassVar[int]
    ACTIVE_ANTENNA_FIELD_NUMBER: _ClassVar[int]
    DOWNLINK_LQ_PCT_FIELD_NUMBER: _ClassVar[int]
    tx_enabled: bool
    link_up: bool
    lq_pct: float
    rssi_dbm: float
    snr_db: float
    active_antenna: str
    downlink_lq_pct: float
    def __init__(self, tx_enabled: _Optional[bool] = ..., link_up: _Optional[bool] = ..., lq_pct: _Optional[float] = ..., rssi_dbm: _Optional[float] = ..., snr_db: _Optional[float] = ..., active_antenna: _Optional[str] = ..., downlink_lq_pct: _Optional[float] = ...) -> None: ...

class FcTelemetry(_message.Message):
    __slots__ = ("battery_v", "battery_a", "battery_mah", "roll_deg", "pitch_deg", "yaw_deg", "flight_mode", "frame_count", "telemetry_age_s")
    BATTERY_V_FIELD_NUMBER: _ClassVar[int]
    BATTERY_A_FIELD_NUMBER: _ClassVar[int]
    BATTERY_MAH_FIELD_NUMBER: _ClassVar[int]
    ROLL_DEG_FIELD_NUMBER: _ClassVar[int]
    PITCH_DEG_FIELD_NUMBER: _ClassVar[int]
    YAW_DEG_FIELD_NUMBER: _ClassVar[int]
    FLIGHT_MODE_FIELD_NUMBER: _ClassVar[int]
    FRAME_COUNT_FIELD_NUMBER: _ClassVar[int]
    TELEMETRY_AGE_S_FIELD_NUMBER: _ClassVar[int]
    battery_v: float
    battery_a: float
    battery_mah: float
    roll_deg: float
    pitch_deg: float
    yaw_deg: float
    flight_mode: str
    frame_count: int
    telemetry_age_s: float
    def __init__(self, battery_v: _Optional[float] = ..., battery_a: _Optional[float] = ..., battery_mah: _Optional[float] = ..., roll_deg: _Optional[float] = ..., pitch_deg: _Optional[float] = ..., yaw_deg: _Optional[float] = ..., flight_mode: _Optional[str] = ..., frame_count: _Optional[int] = ..., telemetry_age_s: _Optional[float] = ...) -> None: ...

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

class AntennaRssi(_message.Message):
    __slots__ = ("name", "rssi_dbm")
    NAME_FIELD_NUMBER: _ClassVar[int]
    RSSI_DBM_FIELD_NUMBER: _ClassVar[int]
    name: str
    rssi_dbm: float
    def __init__(self, name: _Optional[str] = ..., rssi_dbm: _Optional[float] = ...) -> None: ...

class VideoLink(_message.Message):
    __slots__ = ("present", "snr_db", "interference_dbm", "rssi", "active_antenna", "noise", "sparkles", "chroma", "sync", "field_valid", "latency_ms")
    PRESENT_FIELD_NUMBER: _ClassVar[int]
    SNR_DB_FIELD_NUMBER: _ClassVar[int]
    INTERFERENCE_DBM_FIELD_NUMBER: _ClassVar[int]
    RSSI_FIELD_NUMBER: _ClassVar[int]
    ACTIVE_ANTENNA_FIELD_NUMBER: _ClassVar[int]
    NOISE_FIELD_NUMBER: _ClassVar[int]
    SPARKLES_FIELD_NUMBER: _ClassVar[int]
    CHROMA_FIELD_NUMBER: _ClassVar[int]
    SYNC_FIELD_NUMBER: _ClassVar[int]
    FIELD_VALID_FIELD_NUMBER: _ClassVar[int]
    LATENCY_MS_FIELD_NUMBER: _ClassVar[int]
    present: bool
    snr_db: float
    interference_dbm: float
    rssi: _containers.RepeatedCompositeFieldContainer[AntennaRssi]
    active_antenna: str
    noise: float
    sparkles: float
    chroma: float
    sync: VideoSync
    field_valid: bool
    latency_ms: int
    def __init__(self, present: _Optional[bool] = ..., snr_db: _Optional[float] = ..., interference_dbm: _Optional[float] = ..., rssi: _Optional[_Iterable[_Union[AntennaRssi, _Mapping]]] = ..., active_antenna: _Optional[str] = ..., noise: _Optional[float] = ..., sparkles: _Optional[float] = ..., chroma: _Optional[float] = ..., sync: _Optional[_Union[VideoSync, str]] = ..., field_valid: _Optional[bool] = ..., latency_ms: _Optional[int] = ...) -> None: ...

class State(_message.Message):
    __slots__ = ("time_s", "position_ned_m", "velocity_ned_mps", "attitude", "rate_frd_radps", "battery_voltage_v", "battery_current_a", "motor_rpm", "motor_cmd", "radio", "running", "overruns", "fc_restarts", "vtx", "serial_dropped_bytes", "video", "collision_speed_mps", "fc_telemetry", "battery_cell_voltages", "race")
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
    VIDEO_FIELD_NUMBER: _ClassVar[int]
    COLLISION_SPEED_MPS_FIELD_NUMBER: _ClassVar[int]
    FC_TELEMETRY_FIELD_NUMBER: _ClassVar[int]
    BATTERY_CELL_VOLTAGES_FIELD_NUMBER: _ClassVar[int]
    RACE_FIELD_NUMBER: _ClassVar[int]
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
    video: VideoLink
    collision_speed_mps: float
    fc_telemetry: FcTelemetry
    battery_cell_voltages: _containers.RepeatedScalarFieldContainer[float]
    race: RaceState
    def __init__(self, time_s: _Optional[float] = ..., position_ned_m: _Optional[_Union[Vec3, _Mapping]] = ..., velocity_ned_mps: _Optional[_Union[Vec3, _Mapping]] = ..., attitude: _Optional[_Union[Quat, _Mapping]] = ..., rate_frd_radps: _Optional[_Union[Vec3, _Mapping]] = ..., battery_voltage_v: _Optional[float] = ..., battery_current_a: _Optional[float] = ..., motor_rpm: _Optional[_Iterable[float]] = ..., motor_cmd: _Optional[_Iterable[float]] = ..., radio: _Optional[_Union[RadioLink, _Mapping]] = ..., running: _Optional[bool] = ..., overruns: _Optional[int] = ..., fc_restarts: _Optional[int] = ..., vtx: _Optional[_Union[Vtx, _Mapping]] = ..., serial_dropped_bytes: _Optional[int] = ..., video: _Optional[_Union[VideoLink, _Mapping]] = ..., collision_speed_mps: _Optional[float] = ..., fc_telemetry: _Optional[_Union[FcTelemetry, _Mapping]] = ..., battery_cell_voltages: _Optional[_Iterable[float]] = ..., race: _Optional[_Union[RaceState, _Mapping]] = ...) -> None: ...

class RaceState(_message.Message):
    __slots__ = ("present", "running", "next_gate", "gate_count", "laps", "current_lap_s", "last_lap_s", "best_lap_s")
    PRESENT_FIELD_NUMBER: _ClassVar[int]
    RUNNING_FIELD_NUMBER: _ClassVar[int]
    NEXT_GATE_FIELD_NUMBER: _ClassVar[int]
    GATE_COUNT_FIELD_NUMBER: _ClassVar[int]
    LAPS_FIELD_NUMBER: _ClassVar[int]
    CURRENT_LAP_S_FIELD_NUMBER: _ClassVar[int]
    LAST_LAP_S_FIELD_NUMBER: _ClassVar[int]
    BEST_LAP_S_FIELD_NUMBER: _ClassVar[int]
    present: bool
    running: bool
    next_gate: int
    gate_count: int
    laps: int
    current_lap_s: float
    last_lap_s: float
    best_lap_s: float
    def __init__(self, present: _Optional[bool] = ..., running: _Optional[bool] = ..., next_gate: _Optional[int] = ..., gate_count: _Optional[int] = ..., laps: _Optional[int] = ..., current_lap_s: _Optional[float] = ..., last_lap_s: _Optional[float] = ..., best_lap_s: _Optional[float] = ...) -> None: ...

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

class RadioLoss(_message.Message):
    __slots__ = ("fraction",)
    FRACTION_FIELD_NUMBER: _ClassVar[int]
    fraction: float
    def __init__(self, fraction: _Optional[float] = ...) -> None: ...

class DownlinkLoss(_message.Message):
    __slots__ = ("fraction",)
    FRACTION_FIELD_NUMBER: _ClassVar[int]
    fraction: float
    def __init__(self, fraction: _Optional[float] = ...) -> None: ...

class BurstLoss(_message.Message):
    __slots__ = ("on_ms", "off_ms")
    ON_MS_FIELD_NUMBER: _ClassVar[int]
    OFF_MS_FIELD_NUMBER: _ClassVar[int]
    on_ms: int
    off_ms: int
    def __init__(self, on_ms: _Optional[int] = ..., off_ms: _Optional[int] = ...) -> None: ...

class MotorFailure(_message.Message):
    __slots__ = ("motor", "scale")
    MOTOR_FIELD_NUMBER: _ClassVar[int]
    SCALE_FIELD_NUMBER: _ClassVar[int]
    motor: int
    scale: float
    def __init__(self, motor: _Optional[int] = ..., scale: _Optional[float] = ...) -> None: ...

class PropDamage(_message.Message):
    __slots__ = ("prop", "thrust_scale", "imbalance_kgm")
    PROP_FIELD_NUMBER: _ClassVar[int]
    THRUST_SCALE_FIELD_NUMBER: _ClassVar[int]
    IMBALANCE_KGM_FIELD_NUMBER: _ClassVar[int]
    prop: int
    thrust_scale: float
    imbalance_kgm: float
    def __init__(self, prop: _Optional[int] = ..., thrust_scale: _Optional[float] = ..., imbalance_kgm: _Optional[float] = ...) -> None: ...

class WeakCell(_message.Message):
    __slots__ = ("cell", "r_offset_ohm", "v_offset_v")
    CELL_FIELD_NUMBER: _ClassVar[int]
    R_OFFSET_OHM_FIELD_NUMBER: _ClassVar[int]
    V_OFFSET_V_FIELD_NUMBER: _ClassVar[int]
    cell: int
    r_offset_ohm: float
    v_offset_v: float
    def __init__(self, cell: _Optional[int] = ..., r_offset_ohm: _Optional[float] = ..., v_offset_v: _Optional[float] = ...) -> None: ...

class GyroFault(_message.Message):
    __slots__ = ("bias_radps", "noise_scale")
    BIAS_RADPS_FIELD_NUMBER: _ClassVar[int]
    NOISE_SCALE_FIELD_NUMBER: _ClassVar[int]
    bias_radps: Vec3
    noise_scale: float
    def __init__(self, bias_radps: _Optional[_Union[Vec3, _Mapping]] = ..., noise_scale: _Optional[float] = ...) -> None: ...

class VtxInterference(_message.Message):
    __slots__ = ("freq_mhz", "power_mw", "position_ned_m")
    FREQ_MHZ_FIELD_NUMBER: _ClassVar[int]
    POWER_MW_FIELD_NUMBER: _ClassVar[int]
    POSITION_NED_M_FIELD_NUMBER: _ClassVar[int]
    freq_mhz: float
    power_mw: float
    position_ned_m: Vec3
    def __init__(self, freq_mhz: _Optional[float] = ..., power_mw: _Optional[float] = ..., position_ned_m: _Optional[_Union[Vec3, _Mapping]] = ...) -> None: ...

class Fault(_message.Message):
    __slots__ = ("radio_link_loss", "radio_loss", "burst_loss", "motor_failure", "prop_damage", "weak_cell", "gyro_fault", "vtx_interference", "downlink_loss", "at_s")
    RADIO_LINK_LOSS_FIELD_NUMBER: _ClassVar[int]
    RADIO_LOSS_FIELD_NUMBER: _ClassVar[int]
    BURST_LOSS_FIELD_NUMBER: _ClassVar[int]
    MOTOR_FAILURE_FIELD_NUMBER: _ClassVar[int]
    PROP_DAMAGE_FIELD_NUMBER: _ClassVar[int]
    WEAK_CELL_FIELD_NUMBER: _ClassVar[int]
    GYRO_FAULT_FIELD_NUMBER: _ClassVar[int]
    VTX_INTERFERENCE_FIELD_NUMBER: _ClassVar[int]
    DOWNLINK_LOSS_FIELD_NUMBER: _ClassVar[int]
    AT_S_FIELD_NUMBER: _ClassVar[int]
    radio_link_loss: RadioLinkLoss
    radio_loss: RadioLoss
    burst_loss: BurstLoss
    motor_failure: MotorFailure
    prop_damage: PropDamage
    weak_cell: WeakCell
    gyro_fault: GyroFault
    vtx_interference: VtxInterference
    downlink_loss: DownlinkLoss
    at_s: float
    def __init__(self, radio_link_loss: _Optional[_Union[RadioLinkLoss, _Mapping]] = ..., radio_loss: _Optional[_Union[RadioLoss, _Mapping]] = ..., burst_loss: _Optional[_Union[BurstLoss, _Mapping]] = ..., motor_failure: _Optional[_Union[MotorFailure, _Mapping]] = ..., prop_damage: _Optional[_Union[PropDamage, _Mapping]] = ..., weak_cell: _Optional[_Union[WeakCell, _Mapping]] = ..., gyro_fault: _Optional[_Union[GyroFault, _Mapping]] = ..., vtx_interference: _Optional[_Union[VtxInterference, _Mapping]] = ..., downlink_loss: _Optional[_Union[DownlinkLoss, _Mapping]] = ..., at_s: _Optional[float] = ...) -> None: ...

class RecordingSignal(_message.Message):
    __slots__ = ("pattern", "rate_hz")
    PATTERN_FIELD_NUMBER: _ClassVar[int]
    RATE_HZ_FIELD_NUMBER: _ClassVar[int]
    pattern: str
    rate_hz: float
    def __init__(self, pattern: _Optional[str] = ..., rate_hz: _Optional[float] = ...) -> None: ...

class StartRecordingRequest(_message.Message):
    __slots__ = ("signals",)
    SIGNALS_FIELD_NUMBER: _ClassVar[int]
    signals: _containers.RepeatedCompositeFieldContainer[RecordingSignal]
    def __init__(self, signals: _Optional[_Iterable[_Union[RecordingSignal, _Mapping]]] = ...) -> None: ...

class RecordingStarted(_message.Message):
    __slots__ = ("path", "signals")
    PATH_FIELD_NUMBER: _ClassVar[int]
    SIGNALS_FIELD_NUMBER: _ClassVar[int]
    path: str
    signals: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, path: _Optional[str] = ..., signals: _Optional[_Iterable[str]] = ...) -> None: ...

class RecordingSummary(_message.Message):
    __slots__ = ("path", "messages", "first_s", "last_s")
    PATH_FIELD_NUMBER: _ClassVar[int]
    MESSAGES_FIELD_NUMBER: _ClassVar[int]
    FIRST_S_FIELD_NUMBER: _ClassVar[int]
    LAST_S_FIELD_NUMBER: _ClassVar[int]
    path: str
    messages: int
    first_s: float
    last_s: float
    def __init__(self, path: _Optional[str] = ..., messages: _Optional[int] = ..., first_s: _Optional[float] = ..., last_s: _Optional[float] = ...) -> None: ...

class InputsFile(_message.Message):
    __slots__ = ("name", "csv")
    NAME_FIELD_NUMBER: _ClassVar[int]
    CSV_FIELD_NUMBER: _ClassVar[int]
    name: str
    csv: str
    def __init__(self, name: _Optional[str] = ..., csv: _Optional[str] = ...) -> None: ...

class InputsRecordingStarted(_message.Message):
    __slots__ = ("path",)
    PATH_FIELD_NUMBER: _ClassVar[int]
    path: str
    def __init__(self, path: _Optional[str] = ...) -> None: ...

class InputsRecordingSummary(_message.Message):
    __slots__ = ("path", "rows", "duration_s")
    PATH_FIELD_NUMBER: _ClassVar[int]
    ROWS_FIELD_NUMBER: _ClassVar[int]
    DURATION_S_FIELD_NUMBER: _ClassVar[int]
    path: str
    rows: int
    duration_s: float
    def __init__(self, path: _Optional[str] = ..., rows: _Optional[int] = ..., duration_s: _Optional[float] = ...) -> None: ...

class ReceiverAntenna(_message.Message):
    __slots__ = ("name", "kind", "gain_dbi", "beamwidth_deg", "polarization", "aim_az_deg", "aim_el_deg")
    NAME_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    GAIN_DBI_FIELD_NUMBER: _ClassVar[int]
    BEAMWIDTH_DEG_FIELD_NUMBER: _ClassVar[int]
    POLARIZATION_FIELD_NUMBER: _ClassVar[int]
    AIM_AZ_DEG_FIELD_NUMBER: _ClassVar[int]
    AIM_EL_DEG_FIELD_NUMBER: _ClassVar[int]
    name: str
    kind: str
    gain_dbi: float
    beamwidth_deg: float
    polarization: str
    aim_az_deg: float
    aim_el_deg: float
    def __init__(self, name: _Optional[str] = ..., kind: _Optional[str] = ..., gain_dbi: _Optional[float] = ..., beamwidth_deg: _Optional[float] = ..., polarization: _Optional[str] = ..., aim_az_deg: _Optional[float] = ..., aim_el_deg: _Optional[float] = ...) -> None: ...

class WorldObject(_message.Message):
    __slots__ = ("name", "shape", "center_ned_m", "size_m", "radius_m", "height_m", "color", "rf_loss_db")
    NAME_FIELD_NUMBER: _ClassVar[int]
    SHAPE_FIELD_NUMBER: _ClassVar[int]
    CENTER_NED_M_FIELD_NUMBER: _ClassVar[int]
    SIZE_M_FIELD_NUMBER: _ClassVar[int]
    RADIUS_M_FIELD_NUMBER: _ClassVar[int]
    HEIGHT_M_FIELD_NUMBER: _ClassVar[int]
    COLOR_FIELD_NUMBER: _ClassVar[int]
    RF_LOSS_DB_FIELD_NUMBER: _ClassVar[int]
    name: str
    shape: str
    center_ned_m: Vec3
    size_m: Vec3
    radius_m: float
    height_m: float
    color: Vec3
    rf_loss_db: float
    def __init__(self, name: _Optional[str] = ..., shape: _Optional[str] = ..., center_ned_m: _Optional[_Union[Vec3, _Mapping]] = ..., size_m: _Optional[_Union[Vec3, _Mapping]] = ..., radius_m: _Optional[float] = ..., height_m: _Optional[float] = ..., color: _Optional[_Union[Vec3, _Mapping]] = ..., rf_loss_db: _Optional[float] = ...) -> None: ...

class Gate(_message.Message):
    __slots__ = ("name", "center_ned_m", "heading_deg", "width_m", "height_m")
    NAME_FIELD_NUMBER: _ClassVar[int]
    CENTER_NED_M_FIELD_NUMBER: _ClassVar[int]
    HEADING_DEG_FIELD_NUMBER: _ClassVar[int]
    WIDTH_M_FIELD_NUMBER: _ClassVar[int]
    HEIGHT_M_FIELD_NUMBER: _ClassVar[int]
    name: str
    center_ned_m: Vec3
    heading_deg: float
    width_m: float
    height_m: float
    def __init__(self, name: _Optional[str] = ..., center_ned_m: _Optional[_Union[Vec3, _Mapping]] = ..., heading_deg: _Optional[float] = ..., width_m: _Optional[float] = ..., height_m: _Optional[float] = ...) -> None: ...

class Emitter(_message.Message):
    __slots__ = ("name", "position_ned_m", "freq_mhz", "power_mw")
    NAME_FIELD_NUMBER: _ClassVar[int]
    POSITION_NED_M_FIELD_NUMBER: _ClassVar[int]
    FREQ_MHZ_FIELD_NUMBER: _ClassVar[int]
    POWER_MW_FIELD_NUMBER: _ClassVar[int]
    name: str
    position_ned_m: Vec3
    freq_mhz: float
    power_mw: float
    def __init__(self, name: _Optional[str] = ..., position_ned_m: _Optional[_Union[Vec3, _Mapping]] = ..., freq_mhz: _Optional[float] = ..., power_mw: _Optional[float] = ...) -> None: ...

class Handset(_message.Message):
    __slots__ = ("position_ned_m", "antennas")
    POSITION_NED_M_FIELD_NUMBER: _ClassVar[int]
    ANTENNAS_FIELD_NUMBER: _ClassVar[int]
    position_ned_m: Vec3
    antennas: _containers.RepeatedCompositeFieldContainer[ReceiverAntenna]
    def __init__(self, position_ned_m: _Optional[_Union[Vec3, _Mapping]] = ..., antennas: _Optional[_Iterable[_Union[ReceiverAntenna, _Mapping]]] = ...) -> None: ...

class World(_message.Message):
    __slots__ = ("name", "pilot_position_ned_m", "pilot_facing_deg", "antennas", "objects", "emitters", "handset", "gates")
    NAME_FIELD_NUMBER: _ClassVar[int]
    PILOT_POSITION_NED_M_FIELD_NUMBER: _ClassVar[int]
    PILOT_FACING_DEG_FIELD_NUMBER: _ClassVar[int]
    ANTENNAS_FIELD_NUMBER: _ClassVar[int]
    OBJECTS_FIELD_NUMBER: _ClassVar[int]
    EMITTERS_FIELD_NUMBER: _ClassVar[int]
    HANDSET_FIELD_NUMBER: _ClassVar[int]
    GATES_FIELD_NUMBER: _ClassVar[int]
    name: str
    pilot_position_ned_m: Vec3
    pilot_facing_deg: float
    antennas: _containers.RepeatedCompositeFieldContainer[ReceiverAntenna]
    objects: _containers.RepeatedCompositeFieldContainer[WorldObject]
    emitters: _containers.RepeatedCompositeFieldContainer[Emitter]
    handset: Handset
    gates: _containers.RepeatedCompositeFieldContainer[Gate]
    def __init__(self, name: _Optional[str] = ..., pilot_position_ned_m: _Optional[_Union[Vec3, _Mapping]] = ..., pilot_facing_deg: _Optional[float] = ..., antennas: _Optional[_Iterable[_Union[ReceiverAntenna, _Mapping]]] = ..., objects: _Optional[_Iterable[_Union[WorldObject, _Mapping]]] = ..., emitters: _Optional[_Iterable[_Union[Emitter, _Mapping]]] = ..., handset: _Optional[_Union[Handset, _Mapping]] = ..., gates: _Optional[_Iterable[_Union[Gate, _Mapping]]] = ...) -> None: ...
