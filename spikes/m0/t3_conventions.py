"""THROWAWAY M0 Task 3: verify gyro/accel/attitude conventions through motor responses.

Ground truth is physics, not displays: a rate on an axis must make the PID push
back against it, and a stick must push the matching motors.
Motor index: 0=M1 RR, 1=M2 FR, 2=M3 RL, 3=M4 FL. CW props: M1, M4.
"""
import os
from ofs_spike import *

RIGHT, LEFT, REAR, FRONT, CW, CCW = (0, 1), (2, 3), (0, 2), (1, 3), (0, 3), (1, 2)


def bias(m, group):
    other = [i for i in range(4) if i not in group]
    return sum(m[i] for i in group) - sum(m[i] for i in other)


MSP_CLIENT = None


def check(name, base, m, group):
    d = bias(m, group) - bias(base, group)
    ok = d > 0.01
    print(f"{'PASS' if ok else 'FAIL'}  {name:42s} delta={d:+.4f}  motors={['%.3f' % x for x in m]}")
    if not ok and MSP_CLIENT is not None:
        print("      arming disabled flags now:", msp_arming_disabled(MSP_CLIENT))
    return ok


work = os.path.abspath("runs/t3")
if not os.path.exists(os.path.join(work, "eeprom.bin")):
    apply_config(work, "spike.diff")
proc = launch_sitl(work)
link = SitlLink()
results = []
try:
    f = Flight(link)
    disarmed = dict(throttle=0.0, aux=(-1, -1, -1, -1))
    acro = dict(throttle=0.5, aux=(1, -1, -1, -1))
    angle = dict(throttle=0.5, aux=(1, 1, -1, -1))
    f.run(5.0, sticks=disarmed)                       # boot + gyro calibration, perfectly still
    f.run(1.0, sticks=dict(throttle=0.0, aux=(1, -1, -1, -1)))  # arm
    base = f.run(1.0, sticks=acro)
    print("armed baseline:", base, " (all zero => not armed: read runs/t3/sitl.log 'Arming disabled')")

    # Rate disturbances: PID must oppose them. Short pulses so I-term stays small.
    results.append(check("gyro +x FRD (rolling right) -> roll left", base, f.run(0.03, gyro=(1, 0, 0), sticks=acro), RIGHT))
    f.run(0.5, sticks=acro)
    results.append(check("gyro +y FRD (nose rising) -> pitch down", base, f.run(0.03, gyro=(0, 1, 0), sticks=acro), REAR))
    f.run(0.5, sticks=acro)
    results.append(check("gyro +z FRD (yawing right) -> yaw left", base, f.run(0.03, gyro=(0, 0, 1), sticks=acro), CW))
    f.run(0.5, sticks=acro)

    # Sticks.
    results.append(check("roll stick right -> left motors up", base, f.run(0.03, sticks=dict(acro, roll=0.3)), LEFT))
    f.run(0.5, sticks=acro)
    results.append(check("pitch stick forward -> rear motors up", base, f.run(0.03, sticks=dict(acro, pitch=0.3)), REAR))
    f.run(0.5, sticks=acro)
    results.append(check("yaw stick right -> CCW props up", base, f.run(0.03, sticks=dict(acro, yaw=0.3)), CCW))
    f.run(0.5, sticks=acro)

    # Angle mode: estimator must read tilt from accel and level the craft.
    base_angle = f.run(2.0, sticks=angle)
    results.append(check("angle: rolled right 20 deg -> roll left", base_angle,
                         f.run(1.5, q=attitude_ned(roll_deg=20), sticks=angle), RIGHT))
    f.run(2.0, sticks=angle)
    results.append(check("angle: nose up 20 deg -> pitch down", base_angle,
                         f.run(1.5, q=attitude_ned(pitch_deg=20), sticks=angle), REAR))

    msp = Msp(pump=f.hold)
    for r, p, y in ((0, 0, 0), (20, 0, 0), (0, 10, 0), (0, 0, 90), (20, 10, 45)):
        q = attitude_ned(roll_deg=r, pitch_deg=p, yaw_deg=y)
        f.run(6.0, q=q, sticks=disarmed)
        expect_acc = tuple(round(-a * s * 256 / G) for a, s in zip(specific_force_frd(q), ACCEL_SIGN))
        print(f"FRD roll={r:+3d} pitch={p:+3d} yaw={y:+3d}: MSP_ATTITUDE={msp_attitude(msp)} "
              f"raw acc={msp_raw_imu(msp)[0]} (sent, after SITL negation: {expect_acc})")
finally:
    link.close()
    stop_sitl(proc)
print(f"{sum(results)}/{len(results)} checks passed")
