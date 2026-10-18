"""Flies the closed circuit in worlds/circuit.toml through its four gates with a scripted pilot.

The pilot steers with the sticks, like a person: an altitude loop on the throttle, and a position loop that turns the
distance to the next waypoint into a speed, then a lean (roll east, pitch north; the heading is held at north). The
same script flies two backends: the built-in open-loop stabilizer (fast, no Betaflight) and real Betaflight SITL in
angle mode. It reads the quad's state, never the gate events, to steer; the events only report what the simulator saw.

Usage (repo root):
    python python/examples/circuit.py                 # the stabilizer
    OFS_SITL_LAUNCH="<sitl launch command>" python python/examples/circuit.py --betaflight
"""
import math
import sys

import ofs

# AUX1 high = ARM, AUX2 high = ANGLE (see the quad's betaflight.diff). The stabilizer ignores them.
ARM_AND_ANGLE = (1.0, 1.0, -1.0, -1.0)
DISARMED = (-1.0, -1.0, -1.0, -1.0)

ALTITUDE_M = 2.0
# Take off from home, then loop the course clockwise: the first pass of Gate0 (eastwards, at (20, 0)) starts the clock
# and the last pass of it completes lap 1. North, east in metres from home.
WAYPOINTS = [(0.0, 0.0), (20.0, -20.0), (20.0, 20.0), (60.0, 20.0), (60.0, -20.0), (20.0, -20.0), (20.0, 20.0)]
CAPTURE_M = 2.5
DT = 0.02
G = 9.81

# Per backend: horizontal speed cap (m/s), lean cap (m/s^2 of horizontal acceleration), and the throttle that hovers.
# "lean" is the lean in degrees one full stick gives: the stabilizer's MAX_ANGLE_DEG is 60; the shipped Betaflight tune
# leans about 7 degrees at a 0.3 stick (hold the stick for 2 s and read the attitude), so the same stick asks for less.
TUNING = {
    "stabilizer": {"speed": 5.0, "accel": 3.0, "hover": 0.35, "lean": 60.0},
    "betaflight": {"speed": 5.0, "accel": 3.0, "hover": 0.35, "lean": 24.0},
}
KP_POSITION = 0.8  # 1/s: distance to speed
KP_VELOCITY = 2.0  # 1/s: speed error to acceleration


def _clamp(x, low, high):
    return max(low, min(high, x))


def _sticks(state, target, tuning, altitude, yaw_deg_target=0.0):
    """(roll, pitch, yaw, throttle) that head for `target` = (north, east) at the altitude loop's request."""
    north, east, _ = state.position_ned_m
    vn, ve, _ = state.velocity_ned_mps
    to_n, to_e = target[0] - north, target[1] - east
    distance = math.hypot(to_n, to_e)
    scale = min(1.0, tuning["speed"] / (KP_POSITION * distance)) if distance > 1e-6 else 0.0
    want_vn, want_ve = KP_POSITION * to_n * scale, KP_POSITION * to_e * scale
    an = _clamp(KP_VELOCITY * (want_vn - vn), -tuning["accel"], tuning["accel"])
    ae = _clamp(KP_VELOCITY * (want_ve - ve), -tuning["accel"], tuning["accel"])
    # Heading north, a forward lean (pitch +) accelerates north and a right lean (roll +) accelerates east.
    pitch = math.degrees(math.atan2(an, G)) / tuning["lean"]
    roll = math.degrees(math.atan2(ae, G)) / tuning["lean"]
    _, _, yaw = state.euler_deg()
    yaw_error = (yaw_deg_target - yaw + 180.0) % 360.0 - 180.0
    yaw_stick = _clamp(0.02 * yaw_error, -1.0, 1.0)
    return _clamp(roll, -1.0, 1.0), _clamp(pitch, -1.0, 1.0), yaw_stick, altitude.throttle(state, tuning["hover"])


class _Altitude:
    """The altitude loop of hover.py, with the throttle raised for the lean (the vertical thrust shrinks by cos)."""

    def __init__(self, target_m):
        self.target_m = target_m
        self.integral = 0.0

    def throttle(self, state, hover):
        err = self.target_m - state.altitude_m
        self.integral = _clamp(self.integral + err * DT, -2.0, 2.0)
        climb = -state.velocity_ned_mps[2]
        roll, pitch, _ = (math.radians(a) for a in state.euler_deg())
        lean = max(0.5, math.cos(roll) * math.cos(pitch))
        return _clamp(hover / lean + 0.15 * err + 0.10 * self.integral - 0.10 * climb, 0.0, 1.0)


def fly_circuit(sim, waypoints=None, betaflight=False, max_seconds=120.0, per_waypoint_s=40.0):
    """Flies `waypoints` (default WAYPOINTS) on a session loaded with the circuit world and returns a summary:
    `events` (every (kind, message) the simulator raised), `laps` (completed), `seconds` (simulated flight time, from
    take-off), `collisions`, and `final` (the last State). Raises RuntimeError naming the waypoint when one is not reached
    within `per_waypoint_s`, or the flight outlasts `max_seconds`.

    The session must be loaded to match the `betaflight` flag (`open_loop_fc=not betaflight, stabilize=not betaflight`,
    the circuit world, seed=1, as `__main__` does): a mismatch is not an error here, it shows up as waypoint timeouts."""
    waypoints = list(waypoints if waypoints is not None else WAYPOINTS)
    tuning = TUNING["betaflight" if betaflight else "stabilizer"]
    aux = ARM_AND_ANGLE if betaflight else DISARMED
    events = []

    def collect():
        events.extend((e.kind, e.message) for e in sim.events())

    sim.events()  # drop what an earlier session left
    if betaflight:
        sim.set_sticks(throttle=0.0, aux=DISARMED)
        sim.run(4.0)  # boot and gyro calibration, perfectly still
        sim.set_sticks(throttle=0.0, aux=ARM_AND_ANGLE)  # arm with the throttle down: Betaflight refuses otherwise
        sim.run(1.0)
    altitude = _Altitude(ALTITUDE_M)
    state = sim.state()
    t_start = state.time_s
    for index, target in enumerate(waypoints):
        t_leg = state.time_s
        while True:
            north, east, _ = state.position_ned_m
            if math.hypot(target[0] - north, target[1] - east) <= CAPTURE_M and (index > 0 or state.altitude_m > 0.8 * ALTITUDE_M):
                break
            if state.time_s - t_leg > per_waypoint_s:
                raise RuntimeError(f"waypoint {index} {target} not reached in {per_waypoint_s:.0f} s (at {state.position_ned_m})")
            if state.time_s - t_start > max_seconds:
                raise RuntimeError(f"the flight took more than {max_seconds:.0f} s (at waypoint {index} {target})")
            roll, pitch, yaw, throttle = _sticks(state, target, tuning, altitude)
            sim.set_sticks(roll=roll, pitch=pitch, yaw=yaw, throttle=throttle, aux=aux)
            state = sim.run(DT)
            collect()
    # The last gate event can still be in flight on the event stream: look for a moment.
    for _ in range(50):
        collect()
        if any(kind == "lap_completed" for kind, _ in events):
            break
        state = sim.run(DT)
    collect()
    return {
        "events": events,
        "laps": state.race.laps,
        "seconds": state.time_s - t_start,
        "collisions": sum(1 for kind, _ in events if kind == "collision"),
        "final": state,
    }


if __name__ == "__main__":
    use_betaflight = "--betaflight" in sys.argv[1:]
    quad = "quads/opendrone-5f-freestyle.toml"
    with ofs.launch() as sim:
        sim.load(quad, seed=1, open_loop_fc=not use_betaflight, stabilize=not use_betaflight, world="worlds/circuit.toml")
        result = fly_circuit(sim, betaflight=use_betaflight)
        for kind, message in result["events"]:
            if kind in ("gate_passed", "lap_completed", "collision"):
                print(f"{kind:14} {message}")
        print({k: v for k, v in result.items() if k not in ("events", "final")})
