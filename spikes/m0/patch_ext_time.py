"""THROWAWAY generator: apply the Open FPV Sim external-time (lockstep) change to a Betaflight tree.

Edits src/platform/SIMULATOR/sitl.c, src/main/scheduler/scheduler.c and the SIMULATOR platform.h.
Run on a clean tree, then `git diff` saves the patch.

Threading model (after review):
- FDM (UDP) thread: applies sensors, then *stages* the packet's time and RC under extPacketMutex and
  counts a tick. It never changes the time or RC the main loop sees.
- Main thread: simulatorTakeGyroTick() applies the staged time and RC and returns true once per packet;
  the scheduler then runs gyro/filter/PID; simulatorSchedulerIdle() replies once per taken tick when
  every task due at that instant has run, then waits (bounded) for the next packet.
"""
import sys

root = sys.argv[1]
path = root + "/src/platform/SIMULATOR/sitl.c"
src = open(path).read()
assert "ENABLE_SIMULATOR_EXTERNAL_TIME" not in src, "already patched"


def sub(old, new):
    global src
    assert old in src, old[:60]
    src = src.replace(old, new, 1)


sub("static double simRate = 1.0;\n", """static double simRate = 1.0;

#ifndef ENABLE_SIMULATOR_EXTERNAL_TIME
#define ENABLE_SIMULATOR_EXTERNAL_TIME 0
#endif

#if ENABLE_SIMULATOR_EXTERNAL_TIME
// Open FPV Sim: simulator-owned time. After the first FDM packet, micros()/millis() follow the
// packet timestamps (from a fixed base) instead of wall time scaled by simRate, and the main loop
// runs gyro/filter/PID exactly once per packet. This makes SITL a deterministic lockstep participant.
#define EXT_TIME_START_US 10000000LL

// Main loop's view of simulated time: written only by the main thread when it takes a tick.
static uint64_t extTimeUs = 0;
static bool extTimeValid = false;  // published with release, read with acquire (other threads read micros())
static uint64_t wallScaledMicros64(void);

static inline bool extTimeIsValid(void)
{
    return __atomic_load_n(&extTimeValid, __ATOMIC_ACQUIRE);
}

// Staged by the FDM thread, consumed by the main thread (all under extPacketMutex).
static pthread_mutex_t extPacketMutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t extPacketCond = PTHREAD_COND_INITIALIZER;
static uint32_t extGyroTicks = 0;        // one per FDM packet whose sensors were applied
static uint32_t extGyroTicksTaken = 0;
static uint64_t extPendingTimeUs = 0;
static bool extRcPending = false;
static uint16_t extRcChannels[SIMULATOR_MAX_RC_CHANNELS];

// FDM thread only.
static int64_t extTimeOffsetUs = 0;
static bool extOffsetSet = false;
static uint64_t extFdmNowUs = 0;
static bool extFdmRcValid = false;
static uint16_t extFdmRcChannels[SIMULATOR_MAX_RC_CHANNELS];

// Main thread only: a tick was taken and has not been answered yet.
#define EXT_MAX_BUSY_PASSES 100000
static bool extReplyPending = false;
static uint32_t extBusyPasses = 0;

// Called by the scheduler: true once per FDM packet. Applies that packet's time and RC first, so the
// main loop never sees a new instant before the tick that belongs to it.
bool simulatorTakeGyroTick(void)
{
    uint16_t rc[SIMULATOR_MAX_RC_CHANNELS];
    bool rcNew = false;
    pthread_mutex_lock(&extPacketMutex);
    const bool tick = extGyroTicks != extGyroTicksTaken;
    if (tick) {
        extGyroTicksTaken = extGyroTicks;
        if (extPendingTimeUs > extTimeUs) {
            __atomic_store_n(&extTimeUs, extPendingTimeUs, __ATOMIC_RELAXED);
        }
        rcNew = extRcPending;
        if (rcNew) {
            memcpy(rc, extRcChannels, sizeof(rc));
            extRcPending = false;
        }
    }
    pthread_mutex_unlock(&extPacketMutex);
    if (!tick) {
        return false;
    }
    __atomic_store_n(&extTimeValid, true, __ATOMIC_RELEASE);
    if (rcNew) {
        rxUpdateUdpChannels(rc, SIMULATOR_MAX_RC_CHANNELS);
    }
    extReplyPending = true;
    return true;
}
#endif
""")

sub("""    const uint64_t realtime_now = micros64_real();
""", """    const uint64_t realtime_now = micros64_real();

#if ENABLE_SIMULATOR_EXTERNAL_TIME
    {
        const int64_t pktUs = (int64_t)(pkt->timestamp * 1e6);
        if (!extOffsetSet) {
            // Fixed start (not wall time) so Betaflight's clock is a pure function of the packet
            // timestamps and runs are reproducible. Boot normally takes < 1 s of wall time.
            const int64_t wallUs = (int64_t)wallScaledMicros64();
            if (wallUs >= EXT_TIME_START_US) {
                fprintf(stderr, "[SITL] warning: first FDM packet after %lld us; time base is wall-derived, runs are not reproducible\\n", (long long)wallUs);
            }
            extTimeOffsetUs = (wallUs < EXT_TIME_START_US ? EXT_TIME_START_US : wallUs) - pktUs;
            extOffsetSet = true;
        }
        extFdmNowUs = (uint64_t)(pktUs + extTimeOffsetUs);
    }
#endif
""")

sub("""uint64_t micros64(void)
{""", """#if ENABLE_SIMULATOR_EXTERNAL_TIME
uint64_t micros64(void)
{
    return extTimeIsValid() ? __atomic_load_n(&extTimeUs, __ATOMIC_RELAXED) : wallScaledMicros64();
}

static uint64_t wallScaledMicros64(void)
#else
uint64_t micros64(void)
#endif
{""")

sub("""uint64_t millis64(void)
{""", """uint64_t millis64(void)
{
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    if (extTimeIsValid()) {
        return __atomic_load_n(&extTimeUs, __ATOMIC_RELAXED) / 1000;
    }
#endif""")

sub("""void delayMicroseconds(uint32_t us)
{""", """void delayMicroseconds(uint32_t us)
{
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    if (extTimeIsValid()) {  // simulated time only advances with FDM packets; never wait on it
        microsleep(us);
        return;
    }
#endif""")

sub("""void delay(uint32_t ms)
{""", """void delay(uint32_t ms)
{
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    if (extTimeIsValid()) {  // simulated time only advances with FDM packets; never wait on it
        microsleep(ms * 1000);
        return;
    }
#endif""")

sub("""    static uint64_t lastDebugTimeUs = 0;
    if (realtime_now - lastDebugTimeUs >= 1000000) {
        lastDebugTimeUs = realtime_now;""", """    static uint64_t lastDebugTimeUs = 0;
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    const uint64_t debugNowUs = extFdmNowUs;  // status once per simulated second (this packet's time)
#else
    const uint64_t debugNowUs = realtime_now;
#endif
    if (debugNowUs - lastDebugTimeUs >= 1000000) {
        lastDebugTimeUs = debugNowUs;""")

sub("""    pthread_mutex_unlock(&updateLock); // can send PWM output now
""", """    pthread_mutex_unlock(&updateLock); // can send PWM output now
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    // Sensors are set: stage this packet's time and RC and count the tick. The main loop applies them
    // in simulatorTakeGyroTick(), so it can never observe the new instant before its tick.
    pthread_mutex_lock(&extPacketMutex);
    extPendingTimeUs = extFdmNowUs;
    if (extFdmRcValid) {
        memcpy(extRcChannels, extFdmRcChannels, sizeof(extRcChannels));
        extRcPending = true;
        extFdmRcValid = false;
    }
    extGyroTicks++;
    pthread_cond_signal(&extPacketCond);
    pthread_mutex_unlock(&extPacketMutex);
#endif
""")

sub("""        n = udpRecv(&stateLink, &fdmPkt, sizeof(fdm_packet), 100);
        if (n == sizeof(fdm_packet)) {""", """#if ENABLE_SIMULATOR_EXTERNAL_TIME
        // Optionally an rc_packet follows the fdm_packet in the same datagram; it is applied with that
        // packet's tick, so stick input is deterministic in lockstep.
        static struct { fdm_packet fdm; rc_packet rc; } __attribute__((packed)) fdmRcPkt;
        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);
        if (n == sizeof(fdmRcPkt)) {
            memcpy(extFdmRcChannels, fdmRcPkt.rc.channels, sizeof(extFdmRcChannels));
            extFdmRcValid = true;
        }
        if (n == sizeof(fdm_packet) || n == sizeof(fdmRcPkt)) {
            fdmPkt = fdmRcPkt.fdm;
#else
        n = udpRecv(&stateLink, &fdmPkt, sizeof(fdm_packet), 100);
        if (n == sizeof(fdm_packet)) {
#endif""")

sub("""    if (pthread_mutex_trylock(&updateLock) != 0) return;
    udpSend(&pwmLink, &pwmPkt, sizeof(servo_packet));
    udpSend(&pwmRawLink, &pwmRawPkt, sizeof(servo_packet_raw));
}
""", """#if ENABLE_SIMULATOR_EXTERNAL_TIME
    // pwmPkt now holds the latest motor values; simulatorSchedulerIdle() sends one reply per tick.
    return;
#endif
    if (pthread_mutex_trylock(&updateLock) != 0) return;
    udpSend(&pwmLink, &pwmPkt, sizeof(servo_packet));
    udpSend(&pwmRawLink, &pwmRawPkt, sizeof(servo_packet_raw));
}

#if ENABLE_SIMULATOR_EXTERNAL_TIME
// Called by the scheduler when nothing is due at the current (frozen) simulated instant. Replying
// only now guarantees the simulator's next packet cannot advance time in the middle of a tick. Every
// taken tick gets exactly one reply (the latest motor values), even when PID did not run on it
// (pid_process_denom > 1).
static void extSendReply(void)
{
    extReplyPending = false;
    extBusyPasses = 0;
    udpSend(&pwmLink, &pwmPkt, sizeof(servo_packet));
    udpSend(&pwmRawLink, &pwmRawPkt, sizeof(servo_packet_raw));
}

void simulatorSchedulerIdle(void)
{
    extBusyPasses = 0;
    if (extReplyPending) {
        extSendReply();
    }
    // Nothing can become due until the next packet advances time: sleep until it arrives (bounded,
    // so wall-time behaviour before the first packet and TCP serial traffic are still serviced).
    pthread_mutex_lock(&extPacketMutex);
    if (extGyroTicks == extGyroTicksTaken) {
        struct timespec until;
        clock_gettime(CLOCK_MONOTONIC, &until);  // extPacketCond uses CLOCK_MONOTONIC (systemInit)
        until.tv_nsec += 1000000;
        if (until.tv_nsec >= 1000000000) {
            until.tv_sec += 1;
            until.tv_nsec -= 1000000000;
        }
        pthread_cond_timedwait(&extPacketCond, &extPacketMutex, &until);
    }
    pthread_mutex_unlock(&extPacketMutex);
}

// Called by the scheduler after a pass that ran a task. Liveness guard: if tasks keep being selected
// at one frozen instant (e.g. an event task whose check never clears), reply anyway rather than stall.
void simulatorSchedulerBusy(void)
{
    if (extReplyPending && ++extBusyPasses > EXT_MAX_BUSY_PASSES) {
        static bool warned = false;
        if (!warned) {
            fprintf(stderr, "[SITL] warning: tasks still due after %u scheduler passes at one simulated instant; replying anyway\\n", (unsigned)EXT_MAX_BUSY_PASSES);
            warned = true;
        }
        extSendReply();
    }
}
#endif
""")

sub("""    clock_gettime(CLOCK_MONOTONIC, &start_time);
""", """    clock_gettime(CLOCK_MONOTONIC, &start_time);
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    {
        // The idle wait must not jump with wall-clock steps (WSL2 resyncs its clock after host sleep).
        pthread_condattr_t attr;
        pthread_condattr_init(&attr);
        pthread_condattr_setclock(&attr, CLOCK_MONOTONIC);
        pthread_cond_init(&extPacketCond, &attr);
        pthread_condattr_destroy(&attr);
    }
#endif
""")

sub(r"""int targetParseArgs(int argc, char * argv[])
{""", r"""int targetParseArgs(int argc, char * argv[])
{
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    // A supervising simulator reads our output through a pipe: keep it line-buffered.
    // (Must precede the first write to the stream.)
    setvbuf(stdout, NULL, _IOLBF, 0);
#endif""")

open(path, "w").write(src)

# scheduler.c: realtime tasks once per packet; non-realtime tasks get a full gyro period of budget
spath = root + "/src/main/scheduler/scheduler.c"
sch = open(spath).read()
assert "simulatorTakeGyroTick" not in sch


def ssub(old, new):
    global sch
    assert old in sch, old[:60]
    sch = sch.replace(old, new, 1)


ssub("""        // Tune out the time lost between completing the last task execution and re-entering the scheduler
""", """#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
        // Lockstep with an external simulator: time only advances with FDM packets, so a cycle
        // target can never be "waited for". Run the realtime tasks exactly once per packet instead.
        if (simulatorTakeGyroTick()) {
            nowCycles = getCycleCounter();  // the tick just advanced simulated time
            nextTargetCycles = nowCycles;
            schedLoopRemainingCycles = 0;
        } else {
            schedLoopRemainingCycles = INT32_MAX;
        }
#endif
        // Tune out the time lost between completing the last task execution and re-entering the scheduler
""")
ssub("""    nowCycles = getCycleCounter();
    schedLoopRemainingCycles = cmpTimeCycles(nextTargetCycles, nowCycles);

    if (!gyroEnabled || (schedLoopRemainingCycles > (int32_t)clockMicrosToCycles(CHECK_GUARD_MARGIN_US))) {""", """    nowCycles = getCycleCounter();
    schedLoopRemainingCycles = cmpTimeCycles(nextTargetCycles, nowCycles);
#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
    schedLoopRemainingCycles = desiredPeriodCycles;  // frozen time: every non-realtime task fits
#endif

    if (!gyroEnabled || (schedLoopRemainingCycles > (int32_t)clockMicrosToCycles(CHECK_GUARD_MARGIN_US))) {""")
ssub("""#if defined(UNIT_TEST)
    readSchedulerLocals(selectedTask, selectedTaskDynamicPriority);""", """#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
    if (!selectedTask && !firstSchedulingOpportunity) {
        simulatorSchedulerIdle();  // every task due at this simulated instant has run
    } else {
        simulatorSchedulerBusy();
    }
#endif

#if defined(UNIT_TEST)
    readSchedulerLocals(selectedTask, selectedTaskDynamicPriority);""")
open(spath, "w").write(sch)

# target.h: declare the lockstep hooks the scheduler calls
tpath = root + "/src/platform/SIMULATOR/target/SITL/target.h"
tgt = open(tpath).read()
anchor = "int lockMainPID(void);\n"
assert anchor in tgt
tgt = tgt.replace(anchor, anchor + """
#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
#include <stdbool.h>
// Open FPV Sim lockstep hooks, called by the scheduler (implemented in sitl.c)
bool simulatorTakeGyroTick(void);
void simulatorSchedulerIdle(void);
void simulatorSchedulerBusy(void);
#endif
""", 1)
open(tpath, "w").write(tgt)

# platform.h: no fixed per-iteration sleep in lockstep (the idle hook waits for packets instead)
ppath = root + "/src/platform/SIMULATOR/include/platform/platform.h"
plat = open(ppath).read()
old = "#define RUN_LOOP_DELAY_US 50 // max 20khz run loop frequency"
assert old in plat
plat = plat.replace(old, """#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
#define RUN_LOOP_DELAY_US 0  // lockstep: the scheduler idle hook waits for the next FDM packet
#else
#define RUN_LOOP_DELAY_US 50 // max 20khz run loop frequency
#endif""", 1)
open(ppath, "w").write(plat)
print("patched", path)
