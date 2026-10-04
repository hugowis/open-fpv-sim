"""THROWAWAY M0 Task 4: reply ratio, real-time factor, and reproducibility.

Usage: python t4_lockstep.py <label> <exchange_hz> [rc_first]
OFS_SITL_LAUNCH selects the build under test. With external time, exchange_hz must equal the
build's VIRTUAL_GYRO_SAMPLE_RATE_HZ (one packet = one gyro sample).
"""
import json
import math
import os
import shutil
import sys
import time
from ofs_spike import *

label, hz = sys.argv[1], int(sys.argv[2])
mode = sys.argv[3] if len(sys.argv) > 3 else "fdm_first"   # fdm_first | rc_first | combined
rc_first = mode == "rc_first"
seed_dir = os.path.abspath("runs/t3")  # eeprom.bin with the working arming config
jitter_ms = float(os.environ.get("JITTER_MS", "0"))   # random host pause before each send (review finding #1)
seed_diff = os.environ.get("SEED_DIFF")               # build the seed eeprom from this CLI diff instead
if seed_diff:
    seed_dir = os.path.abspath(f"runs/t4/seed-{os.path.basename(seed_diff)}")
    shutil.rmtree(seed_dir, ignore_errors=True)
    apply_config(seed_dir, seed_diff)
import random
reply_timeout = float(os.environ.get("REPLY_TIMEOUT", "0.5"))


class OrderedLink(SitlLink):
    def send(self, fdm_pkt, rc_pkt):
        if jitter_ms:
            time.sleep(random.uniform(0, jitter_ms) / 1000.0)
        if mode == "combined":
            self.tx.sendto(fdm_pkt + rc_pkt, (self.host, PORT_STATE))
            return
        pairs = ((rc_pkt, PORT_RC), (fdm_pkt, PORT_STATE)) if rc_first else ((fdm_pkt, PORT_STATE), (rc_pkt, PORT_RC))
        for pkt, port in pairs:
            self.tx.sendto(pkt, (self.host, port))


traces, stats = [], []
for run in range(2):
    work = os.path.abspath(f"runs/t4/{label}-{hz}-{mode}-{run}")
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    shutil.copy(os.path.join(seed_dir, "eeprom.bin"), work)
    proc = launch_sitl(work)
    link = OrderedLink()
    try:
        f = Flight(link, dt=1.0 / hz)
        f.run(5.5, timeout=reply_timeout, sticks=dict(throttle=0.0, aux=(-1, -1, -1, -1)))
        f.run(1.0, timeout=reply_timeout, sticks=dict(throttle=0.0, aux=(1, -1, -1, -1)))
        trace = []
        t0, sent0, rep0 = time.time(), f.sent, f.replies
        for k in range(int(3.0 * hz)):
            g = 0.5 * math.sin(2 * math.pi * 3.0 * k / hz)
            roll = 0.0 if os.environ.get("CONST_RC") else 0.3 * math.sin(2 * math.pi * 1.0 * k / hz)
            f.run(1.0 / hz, timeout=reply_timeout, gyro=(g, 0.0, 0.0), sticks=dict(throttle=0.5, roll=roll, aux=(1, -1, -1, -1)))
            trace.append(list(f.last))
        wall = time.time() - t0
        traces.append(trace)
        stats.append({"reply_ratio": (f.replies - rep0) / (f.sent - sent0), "real_time_factor": 3.0 / wall,
                      "armed": max(max(m) for m in trace) > 0.06})
    finally:
        link.close()
        stop_sitl(proc)
if os.environ.get("TRACE_DUMP"):
    for i in range(0, 14):
        print(i, ["%.4f" % v for v in traces[0][i]], ["%.4f" % v for v in traces[1][i]])
identical = sum(a == b for a, b in zip(*traces))
max_diff = max(max(abs(x - y) for x, y in zip(a, b)) for a, b in zip(*traces))
first_diff = next((i for i, (a, b) in enumerate(zip(*traces)) if a != b), None)
result = {"jitter_ms": jitter_ms, "seed_diff": seed_diff, "label": label, "hz": hz, "mode": mode, "runs": stats,
          "identical_steps": identical, "steps": len(traces[0]), "max_abs_diff": max_diff, "first_diff_step": first_diff}
print(json.dumps(result))
os.makedirs("runs/t4", exist_ok=True)
with open("runs/t4/results.jsonl", "a") as fh:
    fh.write(json.dumps(result) + "\n")
