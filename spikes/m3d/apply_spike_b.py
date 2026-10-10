"""THROWAWAY M3d spike B: DShot telemetry without the DShot driver (no USE_DSHOT).

Enables USE_DSHOT_TELEMETRY + USE_RPM_FILTER for SIMULATOR_BUILD, extends the sitl.c stub with the
dshot.c API those features reference, and feeds synthetic per-motor eRPM from the motor outputs
(offset per motor) in pwmCompleteMotorUpdate. Run inside WSL against ~/ofs/betaflight.
"""
import pathlib

root = pathlib.Path.home() / "ofs/betaflight"

# 1. common_post.h: SITL keeps USE_DSHOT_TELEMETRY (and thus USE_RPM_FILTER) without USE_DSHOT.
p = root / "src/main/target/common_post.h"
s = p.read_text()
old = """#ifndef USE_DSHOT
#undef USE_DSHOT_TELEMETRY
#undef USE_DSHOT_BITBANG
#endif"""
new = """#if !defined(USE_DSHOT) && !defined(SIMULATOR_BUILD)  // SITL: DShot telemetry is emulated without the driver
#undef USE_DSHOT_TELEMETRY
#undef USE_DSHOT_BITBANG
#endif"""
assert old in s, "common_post.h USE_DSHOT block not found"
s = s.replace(old, new)
p.write_text(s)

# 2. target.h: define both features for SITL.
p = root / "src/platform/SIMULATOR/target/SITL/target.h"
s = p.read_text()
old = "#define USE_PWM_OUTPUT\n"
new = ("#define USE_PWM_OUTPUT\n"
       "// M3d spike: DShot telemetry emulated in sitl.c (no DShot driver), so the RPM filter builds.\n"
       "#define USE_DSHOT_TELEMETRY\n"
       "#define USE_RPM_FILTER\n")
assert old in s, "USE_PWM_OUTPUT not found"
s = s.replace(old, new)
p.write_text(s)

# 3. sitl.c: the stub gains the dshot-telemetry API + a synthetic per-motor eRPM feed.
p = root / "src/platform/SIMULATOR/sitl.c"
s = p.read_text()
old = """bool useDshotTelemetry = false;
static float sitlErpmToHz = 0.0f;

void initDshotTelemetry(const timeUs_t looptimeUs)
{
    UNUSED(looptimeUs);
    sitlErpmToHz = 100.0f / SECONDS_PER_MINUTE / (motorConfig()->motorPoleCount / 2.0f);  // ERPM_PER_LSB = 100
}

float erpmToRpm(uint32_t erpm)
{
    return erpm * sitlErpmToHz * SECONDS_PER_MINUTE;
}
#endif"""
new = """static float sitlErpmToHz = 0.0f;

void initDshotTelemetry(const timeUs_t looptimeUs)
{
    UNUSED(looptimeUs);
    sitlErpmToHz = 100.0f / SECONDS_PER_MINUTE / (motorConfig()->motorPoleCount / 2.0f);  // ERPM_PER_LSB = 100
}

float erpmToRpm(uint32_t erpm)
{
    return erpm * sitlErpmToHz * SECONDS_PER_MINUTE;
}

#ifdef USE_DSHOT_TELEMETRY
// SITL DShot-telemetry emulation (dshot.c compiles only with USE_DSHOT): per-motor eRPM provided by
// the simulator. Spike feed: synthetic, from this loop's motor outputs, offset per motor to prove
// attribution; the real design feeds the simulator's motor models.
bool useDshotTelemetry = true;
dshotTelemetryState_t dshotTelemetryState;
static uint32_t sitlDshotErpm[MAX_SUPPORTED_MOTORS];
static float sitlMotorFrequencyHz[MAX_SUPPORTED_MOTORS];

static void sitlDshotTelemetryFeed(void)
{
    for (int i = 0; i < 4; i++) {  // servo_packet carries 4 motor_speed values
        const float throttle = (pwmPkt.motor_speed[i] - 1000.0f) / 1000.0f;
        const uint32_t erpm = (uint32_t)(throttle * 120000.0f * (1.0f + 0.25f * i));
        sitlDshotErpm[i] = erpm;
        sitlMotorFrequencyHz[i] = erpmToRpm(erpm) / 60.0f;
    }
    dshotTelemetryState.readCount++;
}

uint16_t getDshotErpm(uint8_t motorIndex) { return (uint16_t)(sitlDshotErpm[motorIndex] / 100U); }
float getDshotRpm(uint8_t motorIndex) { return erpmToRpm(sitlDshotErpm[motorIndex]); }
float getDshotRpmAverage(void)
{
    float sum = 0.0f;
    for (int i = 0; i < 4; i++) {
        sum += getDshotRpm(i);
    }
    return sum / 4.0f;
}
float getMotorFrequencyHz(uint8_t motorIndex) { return sitlMotorFrequencyHz[motorIndex]; }
float getMinMotorFrequencyHz(void)
{
    float m = sitlMotorFrequencyHz[0];
    for (int i = 1; i < 4; i++) {
        if (sitlMotorFrequencyHz[i] < m) m = sitlMotorFrequencyHz[i];
    }
    return m;
}
bool isDshotMotorTelemetryActive(uint8_t motorIndex) { return sitlDshotErpm[motorIndex] > 0; }
bool isDshotTelemetryActive(void) { return true; }
void dshotCleanTelemetryData(void) { memset(sitlDshotErpm, 0, sizeof(sitlDshotErpm)); }
int16_t getDshotTelemetryMotorInvalidPercent(uint8_t motorIndex) { UNUSED(motorIndex); return 0; }
void updateDshotTelemetry(void) {}
bool getDshotSensorData(escSensorData_t *dest, int motorIndex)
{
    memset(dest, 0, sizeof(*dest));
    dest->rpm = getDshotErpm(motorIndex);
    dest->temperature = 25;
    dest->valid = true;
    return true;
}
#else
bool useDshotTelemetry = false;
#endif
#endif"""
assert old in s, "sitl.c stub block not found"
s = s.replace(old, new)

# 4. pwmCompleteMotorUpdate: feed once per motor update.
old = """    // pwmPkt now holds the latest motor values; simulatorSchedulerIdle() sends one reply per tick.
    return;"""
new = """    // pwmPkt now holds the latest motor values; simulatorSchedulerIdle() sends one reply per tick.
#if defined(USE_ESC_SENSOR) && defined(USE_DSHOT_TELEMETRY) && !defined(USE_DSHOT)
    sitlDshotTelemetryFeed();  // M3d spike: synthetic per-motor eRPM (see the stub above)
#endif
    return;"""
assert old in s, "pwmCompleteMotorUpdate return not found"
s = s.replace(old, new)
p.write_text(s)

# 5. sitl.c needs esc_sensor.h for escSensorData_t (dshot.h already included for the rest).
p = root / "src/platform/SIMULATOR/sitl.c"
s = p.read_text()
marker = '#include "platform.h"'
assert marker in s
if 'sensors/esc_sensor.h' not in s:
    s = s.replace(marker, marker + '\n#include "sensors/esc_sensor.h"', 1)
    p.write_text(s)

print("spike B edits applied")
