"""Runs the quad with Betaflight SITL in real time until Ctrl+C, so Betaflight Configurator can connect.

Usage (repo root): OFS_SITL_LAUNCH="<sitl launch command>" python python/examples/serve_realtime.py [quad.toml] [seconds]
Then in Betaflight Configurator (desktop app): enable manual connection, port tcp://127.0.0.1:5761, Connect.
Saving in the Configurator reboots Betaflight; the simulator relaunches it and the Configurator reconnects.
"""
import sys
import time

import ofs


def serve(quad: str, seconds: float = float("inf")) -> ofs.State:
    with ofs.launch() as sim:
        sim.load(quad, seed=1, mode="realtime")
        sim.start()
        print(f"running in real time; Betaflight Configurator: {sim.configurator_address} (Ctrl+C to stop)", flush=True)
        started = time.monotonic()
        s = sim.state()
        try:
            while time.monotonic() - started < seconds:
                time.sleep(1.0)
                s = sim.state()
                link = "up" if s.radio.link_up else "DOWN"
                print(f"t={s.time_s:7.1f} s  radio {link}  overruns={s.overruns}  betaflight restarts={s.fc_restarts}",
                      flush=True)
        except KeyboardInterrupt:
            pass
        return s


if __name__ == "__main__":
    quad = sys.argv[1] if len(sys.argv) > 1 else "quads/opendrone-5f-freestyle.toml"
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else float("inf")
    serve(quad, seconds)
