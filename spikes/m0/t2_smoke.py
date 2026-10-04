"""THROWAWAY M0 Task 2: SITL boots, answers MSP over TCP, replies to FDM packets."""
import os
import time
from ofs_spike import *

work = os.path.abspath("runs/t2")
if not os.path.exists(os.path.join(work, "eeprom.bin")):
    apply_config(work, "spike.diff")
proc = launch_sitl(work)
link = SitlLink()
try:
    f = Flight(link)
    t0 = time.time()
    f.run(3.0)
    wall = time.time() - t0
    print(f"replies {f.replies}/{f.sent}, sim 3.0 s in {wall:.2f} s wall")
    msp = Msp()
    print("MSP_API_VERSION:", msp.request(MSP_API_VERSION).hex())
    print("attitude (roll, pitch, yaw):", msp_attitude(msp))
finally:
    link.close()
    stop_sitl(proc)
