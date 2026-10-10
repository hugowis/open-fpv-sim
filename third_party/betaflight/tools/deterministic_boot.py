"""Makes lockstep SITL boot the same way every time.

Two races made runs differ (found in M2 with the CRSF receiver; docs/research/sitl-interface.md §8):
- SITL's UDP thread starts early in systemInit(), so a state packet could land while init still ran, and init
  read the fake sensors either before or after the packet's values arrived;
- before the first packet the scheduler ran tasks on wall time (gyro calibration, attitude estimate).

Now state packets are ignored until init has finished and the scheduler runs (which SITL announces with
"[SITL] ready for the simulator"), and the scheduler runs no task until the first packet.

Usage (Linux or WSL): run it second, on the tree that add_serial_in_datagram.py already changed. That tree is the
M1 patch (third_party/betaflight/ofs-sitl.patch as of commit c1514e2) applied to the pinned Betaflight checkout,
plus add_serial_in_datagram.py; see that script's docstring for the whole sequence. The generators are NOT
idempotent: running this one on a tree that already has the current patch inserts its changes a second time.
    python3 third_party/betaflight/tools/deterministic_boot.py ~/ofs/betaflight
"""
import sys

if len(sys.argv) != 2:
    sys.exit(f"usage: python3 {sys.argv[0]} <betaflight checkout>  (see the docstring above)")
ROOT = sys.argv[1]


def edit(rel, replacements):
    path = f"{ROOT}/{rel}"
    with open(path, newline="") as f:
        text = f.read()
    for old, new in replacements:
        assert text.count(old) == 1, f"{rel}: anchor not found exactly once:\n{old}"
        text = text.replace(old, new, 1)
    with open(path, "w", newline="") as f:
        f.write(text)


edit("src/platform/SIMULATOR/target/SITL/target.h", [(
    "bool simulatorTakeGyroTick(void);\n",
    "bool simulatorTakeGyroTick(void);\nbool simulatorTimeStarted(void);\n",
)])

edit("src/platform/SIMULATOR/sitl.c", [
    (
        "// Called by the scheduler: true once per FDM packet.",
        r"""// Set when the scheduler first runs (init has finished). State packets before that are ignored, so init
// never sees a packet's sensor values or time: every boot is the same.
static bool extSchedulerRunning = false;

// False until the first FDM packet is staged: until then the scheduler runs no task, so nothing in boot
// (gyro calibration, attitude estimate) depends on wall time.
bool simulatorTimeStarted(void)
{
    if (!__atomic_load_n(&extSchedulerRunning, __ATOMIC_ACQUIRE)) {
        __atomic_store_n(&extSchedulerRunning, true, __ATOMIC_RELEASE);
        printf("[SITL] ready for the simulator\n");
    }
    pthread_mutex_lock(&extPacketMutex);
    const bool started = extGyroTicks != 0;
    pthread_mutex_unlock(&extPacketMutex);
    return started;
}

// Called by the scheduler: true once per FDM packet.""",
    ),
    (
        "        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);\n",
        """        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);
        if (n > 0 && !__atomic_load_n(&extSchedulerRunning, __ATOMIC_ACQUIRE)) {
            continue;  // still initialising: the simulator resends (see extSchedulerRunning)
        }
""",
    ),
])

edit("src/main/scheduler/scheduler.c", [(
    "FAST_CODE void scheduler(void)\n{\n",
    """FAST_CODE void scheduler(void)
{
#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
    if (!simulatorTimeStarted()) {
        delayMicroseconds(1000);  // no simulated time yet: run no task on wall time
        return;
    }
#endif
""",
)])
print("deterministic boot applied:", ROOT)
