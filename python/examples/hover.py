"""Arms the quad in Betaflight SITL (angle mode) and holds 1 m with a throttle PID.

Usage (repo root): OFS_SITL_LAUNCH="<sitl launch command>" python python/examples/hover.py
"""
import sys

import ofs

ARM_AND_ANGLE = (1.0, 1.0, -1.0, -1.0)  # AUX1 high = ARM, AUX2 high = ANGLE (see the quad's betaflight.diff)


def fly_hover(sim, target_alt=1.0, seconds=8.0):
    sim.set_sticks(throttle=0.0, aux=(-1.0, -1.0, -1.0, -1.0))
    sim.run(4.0)  # boot and gyro calibration, perfectly still
    sim.set_sticks(throttle=0.0, aux=ARM_AND_ANGLE)
    sim.run(1.0)

    hover_guess, kp, ki, kd, dt = 0.35, 0.15, 0.10, 0.10, 0.02
    integral, log = 0.0, []
    s = sim.state()
    t_end = s.time_s + seconds
    while s.time_s < t_end:
        err = target_alt - s.altitude_m
        integral = max(-2.0, min(2.0, integral + err * dt))
        climb = -s.velocity_ned_mps[2]
        throttle = max(0.0, min(1.0, hover_guess + kp * err + ki * integral - kd * climb))
        sim.set_sticks(throttle=throttle, aux=ARM_AND_ANGLE)
        s = sim.run(dt)
        roll, pitch, _ = s.euler_deg()
        log.append((s.time_s, s.altitude_m, roll, pitch, max(s.motor_cmd)))

    tail = [r for r in log if r[0] >= t_end - 5.0]
    return {
        "armed": max(r[4] for r in log) > 0.05,
        "max_alt_err_m": max(abs(r[1] - target_alt) for r in tail),
        "max_tilt_deg": max(max(abs(r[2]), abs(r[3])) for r in tail),
    }


if __name__ == "__main__":
    quad = sys.argv[1] if len(sys.argv) > 1 else "quads/opendrone-5f-freestyle.toml"
    with ofs.launch() as sim:
        sim.load(quad, seed=1)
        print(fly_hover(sim))
