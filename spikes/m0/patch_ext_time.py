"""THROWAWAY: apply the external-time change to a Betaflight tree's sitl.c (then `git diff` saves it)."""
import re, sys
path = sys.argv[1] + "/src/platform/SIMULATOR/sitl.c"
src = open(path).read()
assert "ENABLE_SIMULATOR_EXTERNAL_TIME" not in src, "already patched"

src = src.replace("static double simRate = 1.0;\n", """static double simRate = 1.0;

#ifndef ENABLE_SIMULATOR_EXTERNAL_TIME
#define ENABLE_SIMULATOR_EXTERNAL_TIME 0
#endif

#if ENABLE_SIMULATOR_EXTERNAL_TIME
// Open FPV Sim: simulator-owned time. After the first FDM packet, micros()/millis() follow the
// packet timestamps (offset so the clock stays monotonic) instead of wall time scaled by simRate.
// This makes SITL a deterministic-time participant in a lockstep simulation.
#define EXT_TIME_START_US 10000000LL
static volatile bool extTimeValid = false;
static volatile int64_t extTimeOffsetUs = 0;
static volatile uint64_t extTimeUs = 0;
static uint64_t wallScaledMicros64(void);
static volatile uint32_t extGyroTicks = 0;   // one per FDM packet whose sensors were applied
static uint32_t extGyroTicksTaken = 0;

// Called by the scheduler: true once per FDM packet, so gyro/filter/PID run exactly once per packet.
bool simulatorTakeGyroTick(void)
{
    const uint32_t ticks = extGyroTicks;
    if (ticks != extGyroTicksTaken) {
        extGyroTicksTaken = ticks;
        return true;
    }
    return false;
}
#endif
""", 1)

src = src.replace("""    const uint64_t realtime_now = micros64_real();
""", """    const uint64_t realtime_now = micros64_real();

#if ENABLE_SIMULATOR_EXTERNAL_TIME
    {
        const int64_t pktUs = (int64_t)(pkt->timestamp * 1e6);
        if (!extTimeValid) {
            // Fixed start (not wall time) so Betaflight's clock is a pure function of the packet
            // timestamps and runs are reproducible. Boot normally takes < 1 s of wall time.
            const int64_t wallUs = (int64_t)wallScaledMicros64();
            extTimeOffsetUs = (wallUs < EXT_TIME_START_US ? EXT_TIME_START_US : wallUs) - pktUs;
            extTimeValid = true;
        }
        if (pktUs + extTimeOffsetUs > (int64_t)extTimeUs) {
            extTimeUs = (uint64_t)(pktUs + extTimeOffsetUs);
        }
    }
#endif
""", 1)

src = src.replace("""uint64_t micros64(void)
{""", """#if ENABLE_SIMULATOR_EXTERNAL_TIME
uint64_t micros64(void)
{
    return extTimeValid ? extTimeUs : wallScaledMicros64();
}

static uint64_t wallScaledMicros64(void)
#else
uint64_t micros64(void)
#endif
{""", 1)

src = src.replace("""uint64_t millis64(void)
{""", """uint64_t millis64(void)
{
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    if (extTimeValid) {
        return extTimeUs / 1000;
    }
#endif""", 1)

src = src.replace("""void delayMicroseconds(uint32_t us)
{""", """void delayMicroseconds(uint32_t us)
{
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    if (extTimeValid) {  // simulated time only advances with FDM packets; never wait on it
        microsleep(us);
        return;
    }
#endif""", 1)

src = src.replace("""void delay(uint32_t ms)
{""", """void delay(uint32_t ms)
{
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    if (extTimeValid) {  // simulated time only advances with FDM packets; never wait on it
        microsleep(ms * 1000);
        return;
    }
#endif""", 1)

src = src.replace("""    static uint64_t lastDebugTimeUs = 0;
    if (realtime_now - lastDebugTimeUs >= 1000000) {
        lastDebugTimeUs = realtime_now;""", """    static uint64_t lastDebugTimeUs = 0;
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    const uint64_t debugNowUs = micros64();  // status once per simulated second
#else
    const uint64_t debugNowUs = realtime_now;
#endif
    if (debugNowUs - lastDebugTimeUs >= 1000000) {
        lastDebugTimeUs = debugNowUs;""", 1)

src = src.replace("""    pthread_mutex_unlock(&updateLock); // can send PWM output now
""", """    pthread_mutex_unlock(&updateLock); // can send PWM output now
    // External time: only now (sensors set, updateLock released) may the main loop run this tick.
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    extGyroTicks++;
#endif
""", 1)
src = src.replace("""        n = udpRecv(&stateLink, &fdmPkt, sizeof(fdm_packet), 100);
        if (n == sizeof(fdm_packet)) {""", """#if ENABLE_SIMULATOR_EXTERNAL_TIME
        // Optionally an rc_packet follows the fdm_packet in the same datagram; it is then applied
        // on this thread before the gyro tick, so stick input is deterministic in lockstep.
        static struct { fdm_packet fdm; rc_packet rc; } __attribute__((packed)) fdmRcPkt;
        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);
        if (n == sizeof(fdmRcPkt)) {
            extRcPending = true;
            memcpy(extRcChannels, fdmRcPkt.rc.channels, sizeof(extRcChannels));
        }
        if (n == sizeof(fdm_packet) || n == sizeof(fdmRcPkt)) {
            fdmPkt = fdmRcPkt.fdm;
#else
        n = udpRecv(&stateLink, &fdmPkt, sizeof(fdm_packet), 100);
        if (n == sizeof(fdm_packet)) {
#endif""", 1)
src = src.replace("""#if ENABLE_SIMULATOR_EXTERNAL_TIME
    extGyroTicks++;
#endif""", """#if ENABLE_SIMULATOR_EXTERNAL_TIME
    if (extRcPending) {
        extRcPending = false;
        rxUpdateUdpChannels(extRcChannels, SIMULATOR_MAX_RC_CHANNELS);
    }
    pthread_mutex_lock(&extPacketMutex);
    extGyroTicks++;
    pthread_cond_signal(&extPacketCond);
    pthread_mutex_unlock(&extPacketMutex);
#endif""", 1)
src = src.replace("""static volatile uint32_t extGyroTicks = 0;""", """static volatile uint32_t extGyroTicks = 0;
static bool extRcPending = false;                       // set and consumed on the FDM thread
static uint16_t extRcChannels[SIMULATOR_MAX_RC_CHANNELS];""", 1)
src = src.replace("""    if (pthread_mutex_trylock(&updateLock) != 0) return;
    udpSend(&pwmLink, &pwmPkt, sizeof(servo_packet));
    udpSend(&pwmRawLink, &pwmRawPkt, sizeof(servo_packet_raw));
}
""", """    if (pthread_mutex_trylock(&updateLock) != 0) return;
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    extMotorPending = true;  // sent by simulatorSchedulerIdle() once every task of this tick has run
#else
    udpSend(&pwmLink, &pwmPkt, sizeof(servo_packet));
    udpSend(&pwmRawLink, &pwmRawPkt, sizeof(servo_packet_raw));
#endif
}

#if ENABLE_SIMULATOR_EXTERNAL_TIME
// Called by the scheduler when nothing is due at the current (frozen) simulated instant. Replying
// only now guarantees the simulator's next packet cannot advance time in the middle of a tick.
void simulatorSchedulerIdle(void)
{
    if (extMotorPending) {
        extMotorPending = false;
        udpSend(&pwmLink, &pwmPkt, sizeof(servo_packet));
        udpSend(&pwmRawLink, &pwmRawPkt, sizeof(servo_packet_raw));
    }
    // Nothing can become due until the next packet advances time: sleep until it arrives (bounded,
    // so wall-time behaviour before the first packet and TCP serial traffic are still serviced).
    pthread_mutex_lock(&extPacketMutex);
    if (extGyroTicks == extGyroTicksTaken) {
        struct timespec until;
        clock_gettime(CLOCK_REALTIME, &until);
        until.tv_nsec += 1000000;
        if (until.tv_nsec >= 1000000000) {
            until.tv_sec += 1;
            until.tv_nsec -= 1000000000;
        }
        pthread_cond_timedwait(&extPacketCond, &extPacketMutex, &until);
    }
    pthread_mutex_unlock(&extPacketMutex);
}
#endif
""", 1)
src = src.replace("""static bool extRcPending = false;""", """static pthread_mutex_t extPacketMutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t extPacketCond = PTHREAD_COND_INITIALIZER;
static volatile bool extMotorPending = false;          // set by PID motor update, sent when idle
static bool extRcPending = false;""", 1)
src = src.replace(r"""int targetParseArgs(int argc, char * argv[])
{""", r"""int targetParseArgs(int argc, char * argv[])
{
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    // A supervising simulator reads our output through a pipe: keep it line-buffered.
    // (Must precede the first write to the stream.)
    setvbuf(stdout, NULL, _IOLBF, 0);
#endif""", 1)
assert src.count("ENABLE_SIMULATOR_EXTERNAL_TIME") == 14, src.count("ENABLE_SIMULATOR_EXTERNAL_TIME")

# 2) scheduler.c: realtime tasks once per packet; non-realtime tasks get a full gyro period of budget
spath = sys.argv[1] + "/src/main/scheduler/scheduler.c"
sch = open(spath).read()
assert "simulatorTakeGyroTick" not in sch
sch = sch.replace("""        // Tune out the time lost between completing the last task execution and re-entering the scheduler
""", """#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
        // Lockstep with an external simulator: time only advances with FDM packets, so a cycle
        // target can never be "waited for". Run the realtime tasks exactly once per packet instead.
        if (simulatorTakeGyroTick()) {
            nextTargetCycles = nowCycles;
            schedLoopRemainingCycles = 0;
        } else {
            schedLoopRemainingCycles = INT32_MAX;
        }
#endif
        // Tune out the time lost between completing the last task execution and re-entering the scheduler
""", 1)
sch = sch.replace("""    nowCycles = getCycleCounter();
    schedLoopRemainingCycles = cmpTimeCycles(nextTargetCycles, nowCycles);

    if (!gyroEnabled || (schedLoopRemainingCycles > (int32_t)clockMicrosToCycles(CHECK_GUARD_MARGIN_US))) {""", """    nowCycles = getCycleCounter();
    schedLoopRemainingCycles = cmpTimeCycles(nextTargetCycles, nowCycles);
#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
    schedLoopRemainingCycles = desiredPeriodCycles;  // frozen time: every non-realtime task fits
#endif

    if (!gyroEnabled || (schedLoopRemainingCycles > (int32_t)clockMicrosToCycles(CHECK_GUARD_MARGIN_US))) {""", 1)
sch = sch.replace("""static int32_t desiredPeriodCycles;
""", """static int32_t desiredPeriodCycles;
#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
bool simulatorTakeGyroTick(void);  // platform/SIMULATOR/sitl.c
void simulatorSchedulerIdle(void);  // platform/SIMULATOR/sitl.c
#endif
""", 1)
sch = sch.replace("""#if defined(UNIT_TEST)
    readSchedulerLocals(selectedTask, selectedTaskDynamicPriority);""", """#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
    if (!selectedTask && !firstSchedulingOpportunity) {
        simulatorSchedulerIdle();  // every task due at this simulated instant has run
    }
#endif

#if defined(UNIT_TEST)
    readSchedulerLocals(selectedTask, selectedTaskDynamicPriority);""", 1)
assert sch.count("ENABLE_SIMULATOR_EXTERNAL_TIME") == 8, sch.count("ENABLE_SIMULATOR_EXTERNAL_TIME")
open(spath, "w").write(sch)

# 3) platform.h: no fixed per-iteration sleep in lockstep (the idle hook waits for packets instead)
ppath = sys.argv[1] + "/src/platform/SIMULATOR/include/platform/platform.h"
plat = open(ppath).read()
old = "#define RUN_LOOP_DELAY_US 50 // max 20khz run loop frequency"
assert old in plat
plat = plat.replace(old, """#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
#define RUN_LOOP_DELAY_US 0  // lockstep: the scheduler idle hook waits for the next FDM packet
#else
#define RUN_LOOP_DELAY_US 50 // max 20khz run loop frequency
#endif""", 1)
open(ppath, "w").write(plat), src.count("ENABLE_SIMULATOR_EXTERNAL_TIME")
open(path, "w").write(src)
print("patched", path)
