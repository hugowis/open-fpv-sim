"""A live view of a session in the Rerun viewer (spec §4): the quad's pose in the world, battery, motor, link and
video curves, and the session's events as text.

Needs the `rerun` extra (`pip install open-fpv-sim[rerun]`). It follows the state stream (at most 240 Hz), not the
full bus: for the 8 kHz signals, read the session's MCAP recording.
"""
from __future__ import annotations

_WARN_KINDS = {"link_down", "video_lost", "collision", "fault_injected", "overrun", "serial_overflow"}
_ERROR_KINDS = {"sim_error", "session_ended", "watch_ended"}


def _rr():
    try:
        import rerun as rr
    except ImportError as e:
        raise ImportError("the live view needs the rerun-sdk package; pip install open-fpv-sim[rerun]") from e
    return rr


def log_world(rec, world) -> None:
    """The world as static entities: NED axes (north forward, east right, down), its objects as boxes, the pilot."""
    rr = _rr()
    rec.log("world", rr.ViewCoordinates.FRD, static=True)
    boxes = [o for o in world.objects if o.shape == "box"]
    if boxes:
        # A box's center is mid-height; size is (north, east, height).
        rec.log("world/objects", rr.Boxes3D(centers=[o.center_ned_m for o in boxes],
                                            half_sizes=[[s / 2 for s in o.size_m] for o in boxes],
                                            colors=[o.color for o in boxes], labels=[o.name for o in boxes]), static=True)
    cylinders = [o for o in world.objects if o.shape == "cylinder"]
    if cylinders:
        rec.log("world/poles", rr.Boxes3D(centers=[o.center_ned_m for o in cylinders],
                                          half_sizes=[[o.radius_m, o.radius_m, o.height_m / 2] for o in cylinders],
                                          colors=[o.color for o in cylinders], labels=[o.name for o in cylinders]),
                static=True)
    rec.log("world/pilot", rr.Points3D([world.pilot_position_ned_m], radii=0.3, labels=["pilot"]), static=True)
    rec.log("world/quad/frame", rr.Boxes3D(half_sizes=[[0.12, 0.12, 0.03]]), static=True)


def log_state(rec, state) -> None:
    """One state: the quad's pose (`world/quad`) and its curves, at the state's simulated time."""
    rr = _rr()
    rec.set_time("sim", duration=state.time_s)
    w, x, y, z = state.attitude_wxyz
    rec.log("world/quad", rr.Transform3D(translation=list(state.position_ned_m), quaternion=rr.Quaternion(xyzw=[x, y, z, w])))
    rec.log("battery/voltage_v", rr.Scalars(state.battery_voltage_v))
    rec.log("battery/current_a", rr.Scalars(state.battery_current_a))
    if state.motor_rpm:
        rec.log("motors/rpm", rr.Scalars(list(state.motor_rpm)))
    rec.log("radio/lq_pct", rr.Scalars(state.radio.lq_pct))
    rec.log("radio/rssi_dbm", rr.Scalars(state.radio.rssi_dbm))
    if state.video.present:
        rec.log("video/snr_db", rr.Scalars(state.video.snr_db))


def log_event(rec, event) -> None:
    """One event as a line of text, at its simulated time (events without one, like `watch_ended`, keep the last)."""
    rr = _rr()
    if event.time_s == event.time_s:  # not NaN
        rec.set_time("sim", duration=event.time_s)
    level = rr.TextLogLevel.ERROR if event.kind in _ERROR_KINDS else (
        rr.TextLogLevel.WARN if event.kind in _WARN_KINDS else rr.TextLogLevel.INFO)
    rec.log("events", rr.TextLog(f"{event.kind}: {event.message}", level=level))


def view(sim, rate_hz: int = 30, spawn: bool = True, recording=None, until_s: float | None = None):
    """Streams `sim`'s session into Rerun until the session ends (or its simulated time reaches `until_s`).

    `spawn` starts a viewer; pass `recording` (a `rerun.RecordingStream`, e.g. one that saves to a file) to send
    elsewhere. It takes this `Sim`'s events (`sim.events()`): to keep yours, view through a second connection,
    `ofs.rerun.view(ofs.connect(sim.address))`. Returns the recording stream.
    """
    rr = _rr()
    rec = recording if recording is not None else rr.RecordingStream("open-fpv-sim")
    if spawn:
        rec.spawn()
    log_world(rec, sim.get_world())
    sim._ensure_watch()
    for state in sim.stream_states(rate_hz):
        log_state(rec, state)
        for event in sim.events():
            log_event(rec, event)
        if until_s is not None and state.time_s >= until_s:
            break
    rec.flush()
    return rec
