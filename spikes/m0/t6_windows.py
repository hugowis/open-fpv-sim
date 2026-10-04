"""THROWAWAY M0 Task 6: Windows-side harness against SITL inside WSL2 (default NAT networking).

Run from Windows (not WSL):  python t6_windows.py [wsl-path-to-betaflight_SITL.elf]
Discovers the WSL VM IP and the Windows host IP as seen from WSL, launches SITL with --ip <host IP>,
sends combined fdm+rc datagrams to the VM IP and binds the motor socket on the host-side vEthernet IP.
"""
import os
import socket
import statistics
import subprocess
import sys
import time

from ofs_spike import PORT_PWM, PORT_STATE, attitude_ned, fdm_legacy, rc, specific_force_frd, stop_sitl, launch_sitl

DISTRO = "Ubuntu"
SITL = sys.argv[1] if len(sys.argv) > 1 else "/home/hugow/ofs/betaflight/obj/main/betaflight_SITL.elf"


def wsl(*argv):
    return subprocess.run(["wsl.exe", "-d", DISTRO, "-e", *argv], capture_output=True, text=True).stdout.strip()


vm_ip = wsl("hostname", "-I").split()[0]
host_ip = wsl("sh", "-c", "ip route | awk '/default/ {print $3}'")
print(f"WSL VM IP {vm_ip}, Windows host IP as seen from WSL {host_ip}")

subprocess.run(["wsl.exe", "-d", DISTRO, "-e", "pkill", "-x", "betaflight_SITL"], capture_output=True)  # required cleanup
os.environ["OFS_SITL_LAUNCH"] = f"wsl.exe -d {DISTRO} -e {SITL} --ip {host_ip}"
work = os.path.abspath("runs/t6")
os.makedirs(work, exist_ok=True)
if not os.path.exists(os.path.join(work, "eeprom.bin")):
    sys.exit("seed runs/t6/eeprom.bin first (copy runs/t3/eeprom.bin)")
proc = launch_sitl(work)
pwm = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
pwm.bind((host_ip, PORT_PWM))  # host side of the WSL virtual switch
pwm.settimeout(2.0)            # generous: the first reply may come before the main loop is warm
tx = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
q = attitude_ned()
t, lat, misses = 0.0, [], []
try:
    t0 = time.time()
    for i in range(5000):
        t += 0.001
        a = time.perf_counter()
        tx.sendto(fdm_legacy(t, (0, 0, 0), specific_force_frd(q), q) + rc(t), (vm_ip, PORT_STATE))
        try:
            pwm.recvfrom(64)
            lat.append((time.perf_counter() - a) * 1000)
        except (socket.timeout, ConnectionResetError):
            misses.append(i)
        if i == 0:
            pwm.settimeout(0.5)
    wall = time.time() - t0
    lat.sort()
    print(f"replies {len(lat)}/5000, misses at {misses}, 5.0 s sim in {wall:.2f} s (RTF {5.0 / wall:.2f}), "
          f"p50 {statistics.median(lat):.3f} ms, p99 {lat[int(len(lat) * .99)]:.3f} ms")
finally:
    pwm.close()
    tx.close()
    stop_sitl(proc)
