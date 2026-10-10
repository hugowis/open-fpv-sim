"""Adds flight-controller telemetry to a Betaflight tree that already has ofs-sitl.patch applied:
Betaflight's CRSF telemetry (an atomic-block shim; telemetry/crsf.c compiles again), and per-motor
eRPM for the RPM filter through an emulated DShot telemetry (no USE_DSHOT, no DShot driver). The
eRPM itself arrives in Task 2's state-datagram block; until then the stub feeds synthetic values so
the chain is provable standalone. See docs/research/sitl-interface.md §10 and the M3d spec.

The base tree is the committed ofs-sitl.patch applied to the pinned Betaflight checkout. The
generators are NOT idempotent: running one on a tree that already has the current patch inserts its
changes a second time.

Usage (Linux or WSL), from the repository root:
    bash scripts/build-sitl.sh                                   # pinned commit + the committed patch
    python3 third_party/betaflight/tools/add_fc_telemetry.py ~/ofs/betaflight
    git -C ~/ofs/betaflight diff > third_party/betaflight/ofs-sitl.patch
    bash scripts/build-sitl.sh                                   # rebuild from the regenerated patch
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


# 1. atomic.h: the software-BASEPRI branch (UNIT_TEST's) also covers simulator builds. Upstreamable.
edit("src/main/build/atomic.h", [
    (
        "#if !defined(UNIT_TEST)\n// BASEPRI manipulation functions",
        "#if !defined(UNIT_TEST) && !defined(SIMULATOR_BUILD)\n// BASEPRI manipulation functions",
    ),
    (
        "#if defined(UNIT_TEST)\n// atomic related functions for unittest.",
        "#if defined(UNIT_TEST) || defined(SIMULATOR_BUILD)\n"
        "// atomic related functions for unittest and simulator builds (software BASEPRI).",
    ),
])

# 2. SITL.mk: telemetry/crsf.c compiles again (its ATOMIC_BLOCK is emulated above).
edit("src/platform/SIMULATOR/mk/SITL.mk", [
    (
        "        telemetry/crsf.c \\\n        telemetry/ghst.c \\",
        "        telemetry/ghst.c \\",
    ),
])

# 3. target.h: CRSF telemetry (the master USE_TELEMETRY was never defined for SITL) and the emulated
#    DShot telemetry (no USE_DSHOT: dshot.c stays out, the sitl.c stub owns the API).
edit("src/platform/SIMULATOR/target/SITL/target.h", [
    (
        "#undef USE_TELEMETRY_CRSF\n",
        "// M3d: CRSF telemetry enabled; atomic.h emulates BASEPRI in software for SIMULATOR_BUILD.\n"
        "#define USE_TELEMETRY\n"
        "#define USE_TELEMETRY_CRSF\n",
    ),
    (
        "// USE_TELEMETRY_CRSF stays undefined: telemetry/crsf.c needs an ATOMIC_BLOCK shim for SITL (future work)\n",
        "",
    ),
    (
        "#define USE_PWM_OUTPUT\n",
        "#define USE_PWM_OUTPUT\n"
        "// M3d: DShot telemetry emulated in sitl.c (no DShot driver), so the RPM filter builds.\n"
        "#define USE_DSHOT_TELEMETRY\n"
        "#define USE_DSHOT_TELEMETRY_STATS\n"
        "#define USE_RPM_FILTER\n",
    ),
])

# 4. common_post.h: keep USE_DSHOT_TELEMETRY (and with it USE_RPM_FILTER) without USE_DSHOT under
#    SIMULATOR_BUILD — the same exception USE_ESC_SENSOR already has.
edit("src/main/target/common_post.h", [
    (
        "#ifndef USE_DSHOT\n#undef USE_DSHOT_TELEMETRY\n#undef USE_DSHOT_BITBANG\n#endif",
        "#if !defined(USE_DSHOT) && !defined(SIMULATOR_BUILD)  // SITL: DShot telemetry is emulated without the driver\n"
        "#undef USE_DSHOT_TELEMETRY\n"
        "#undef USE_DSHOT_BITBANG\n"
        "#endif",
    ),
])

# 5. sitl.c: the atomic_BASEPRI storage (atomic.c is UNIT_TEST-only), the includes the stub needs,
#    the DShot-telemetry stub (synthetic per-motor eRPM until Task 2's datagram block lands), and the
#    feed hook in pwmCompleteMotorUpdate.
edit("src/platform/SIMULATOR/sitl.c", [
    (
        '#include "drivers/pwm_output_impl.h"\n',
        '#include "drivers/pwm_output_impl.h"\n'
        '#include "drivers/dshot.h"\n'
        '#include "sensors/esc_sensor.h"\n',
    ),
    (
        "uint32_t microsISR(void)  // used by serial RX drivers (e.g. CRSF frame timing)\n{",
        "uint8_t atomic_BASEPRI;  // software BASEPRI for ATOMIC_BLOCK (atomic.h); atomic.c is UNIT_TEST-only\n"
        "\n"
        "uint32_t microsISR(void)  // used by serial RX drivers (e.g. CRSF frame timing)\n{",
    ),
    (
        "#if defined(USE_ESC_SENSOR) && !defined(USE_DSHOT)\n"
        "// ESC telemetry without DShot: the simulator streams KISS telemetry frames to the ESC sensor UART.\n"
        "// These stand in for the helpers in drivers/dshot.c (compiled only with USE_DSHOT); same math.\n"
        "bool useDshotTelemetry = false;\n"
        "static float sitlErpmToHz = 0.0f;\n"
        "\n"
        "void initDshotTelemetry(const timeUs_t looptimeUs)\n"
        "{\n"
        "    UNUSED(looptimeUs);\n"
        "    sitlErpmToHz = 100.0f / SECONDS_PER_MINUTE / (motorConfig()->motorPoleCount / 2.0f);  // ERPM_PER_LSB = 100\n"
        "}\n"
        "\n"
        "float erpmToRpm(uint32_t erpm)\n"
        "{\n"
        "    return erpm * sitlErpmToHz * SECONDS_PER_MINUTE;\n"
        "}\n"
        "#endif",
        "#if defined(USE_ESC_SENSOR) && !defined(USE_DSHOT)\n"
        "// ESC telemetry without DShot: the simulator streams KISS telemetry frames to the ESC sensor UART.\n"
        "// These stand in for the helpers in drivers/dshot.c (compiled only with USE_DSHOT); same math.\n"
        "static float sitlErpmToHz = 0.0f;\n"
        "\n"
        "void initDshotTelemetry(const timeUs_t looptimeUs)\n"
        "{\n"
        "    UNUSED(looptimeUs);\n"
        "    sitlErpmToHz = 100.0f / SECONDS_PER_MINUTE / (motorConfig()->motorPoleCount / 2.0f);  // ERPM_PER_LSB = 100\n"
        "}\n"
        "\n"
        "float erpmToRpm(uint32_t erpm)\n"
        "{\n"
        "    return erpm * sitlErpmToHz * SECONDS_PER_MINUTE;\n"
        "}\n"
        "\n"
        "#ifdef USE_DSHOT_TELEMETRY\n"
        "// SITL DShot-telemetry emulation (dshot.c compiles only with USE_DSHOT): per-motor eRPM provided by\n"
        "// the simulator. Task 1 feeds synthetic values so the chain is provable standalone; Task 2's state-\n"
        "// datagram block replaces them with the simulator's motor models.\n"
        "bool useDshotTelemetry = true;\n"
        "dshotTelemetryState_t dshotTelemetryState;\n"
        "dshotTelemetryCycleCounters_t dshotDMAHandlerCycleCounters;\n"
        "static uint32_t sitlDshotErpm[MAX_SUPPORTED_MOTORS];\n"
        "static float sitlMotorFrequencyHz[MAX_SUPPORTED_MOTORS];\n"
        "\n"
        "static void sitlDshotTelemetryFeed(void)\n"
        "{\n"
        "    for (int i = 0; i < 4; i++) {  // servo_packet carries 4 motor values\n"
        "        const float throttle = pwmPkt.motor_speed[i];  // throttle 0..1 ((us - 1000) / 1000)\n"
        "        const uint32_t erpm = (uint32_t)(throttle * 120000.0f * (1.0f + 0.25f * i));\n"
        "        sitlDshotErpm[i] = erpm;\n"
        "        sitlMotorFrequencyHz[i] = erpmToRpm(erpm / 100U) / 60.0f;\n"
        "    }\n"
        "    dshotTelemetryState.readCount++;\n"
        "}\n"
        "\n"
        "uint16_t getDshotErpm(uint8_t motorIndex) { return (uint16_t)(sitlDshotErpm[motorIndex] / 100U); }  // ERPM_PER_LSB\n"
        "float getDshotRpm(uint8_t motorIndex) { return erpmToRpm(sitlDshotErpm[motorIndex] / 100U); }\n"
        "float getDshotRpmAverage(void)\n"
        "{\n"
        "    float sum = 0.0f;\n"
        "    for (int i = 0; i < 4; i++) {\n"
        "        sum += getDshotRpm(i);\n"
        "    }\n"
        "    return sum / 4.0f;\n"
        "}\n"
        "float getMotorFrequencyHz(uint8_t motorIndex) { return sitlMotorFrequencyHz[motorIndex]; }\n"
        "float getMinMotorFrequencyHz(void)\n"
        "{\n"
        "    float m = sitlMotorFrequencyHz[0];\n"
        "    for (int i = 1; i < 4; i++) {\n"
        "        if (sitlMotorFrequencyHz[i] < m) m = sitlMotorFrequencyHz[i];\n"
        "    }\n"
        "    return m;\n"
        "}\n"
        "bool isDshotMotorTelemetryActive(uint8_t motorIndex) { return sitlDshotErpm[motorIndex] > 0; }\n"
        "bool isDshotTelemetryActive(void) { return true; }\n"
        "void dshotCleanTelemetryData(void) { memset(sitlDshotErpm, 0, sizeof(sitlDshotErpm)); }\n"
        "int16_t getDshotTelemetryMotorInvalidPercent(uint8_t motorIndex) { UNUSED(motorIndex); return 0; }\n"
        "void updateDshotTelemetry(void) {}\n"
        "bool getDshotSensorData(escSensorData_t *dest, int motorIndex)\n"
        "{\n"
        "    memset(dest, 0, sizeof(*dest));\n"
        "    dest->dataAge = 0;\n"
        "    dest->temperature = 25;\n"
        "    dest->rpm = (int16_t)constrain(sitlDshotErpm[motorIndex] / 100U, 0, 32767);  // 0.01 erpm units (escSensorData_t contract)\n"
        "    return true;\n"
        "}\n"
        "#else\n"
        "bool useDshotTelemetry = false;\n"
        "#endif\n"
        "#endif",
    ),
    (
        "    // pwmPkt now holds the latest motor values; simulatorSchedulerIdle() sends one reply per tick.\n"
        "    return;",
        "    // pwmPkt now holds the latest motor values; simulatorSchedulerIdle() sends one reply per tick.\n"
        "#if defined(USE_ESC_SENSOR) && defined(USE_DSHOT_TELEMETRY) && !defined(USE_DSHOT)\n"
        "    sitlDshotTelemetryFeed();  // M3d: per-motor eRPM (synthetic until Task 2's datagram block)\n"
        "#endif\n"
        "    return;",
    ),
])

# 6. Two upstream NULL-deref bugs: osdEscDataCombined is assigned only in osdProcessStats2 (osd.c),
#    but with USE_ESC_SENSOR and USE_DSHOT_TELEMETRY both compiled it is read during flight — at the
#    ESC-temperature warning blink and in the post-flight stats. Un-guarded, the first arm SIGSEGVs
#    (kernel trap IP + objdump; §10). Worth sending upstream with the storageTotal fix (M0 §5).
edit("src/main/osd/osd_elements.c", [
    (
        "        blink = osdConfig()->esc_temp_alarm != ESC_TEMP_ALARM_OFF && osdEscDataCombined->temperature >= osdConfig()->esc_temp_alarm;",
        "        blink = osdEscDataCombined != NULL && osdConfig()->esc_temp_alarm != ESC_TEMP_ALARM_OFF && osdEscDataCombined->temperature >= osdConfig()->esc_temp_alarm;",
    ),
])
edit("src/main/osd/osd.c", [
    (
        "    if (featureIsEnabled(FEATURE_ESC_SENSOR)) {\n"
        "        return lrintf(erpmToRpm(osdEscDataCombined->rpm));\n"
        "    }",
        "    if (featureIsEnabled(FEATURE_ESC_SENSOR)) {\n"
        "        if (osdEscDataCombined == NULL) return 0;  // OFS: assigned only in osdProcessStats2\n"
        "        return lrintf(erpmToRpm(osdEscDataCombined->rpm));\n"
        "    }",
    ),
    (
        "    if (featureIsEnabled(FEATURE_ESC_SENSOR)) {\n"
        "        value = osdEscDataCombined->temperature;\n"
        "        if (stats.max_esc_temp < value) {\n"
        "            stats.max_esc_temp = value;\n"
        "        }\n"
        "    } else",
        "    if (featureIsEnabled(FEATURE_ESC_SENSOR)) {\n"
        "        if (osdEscDataCombined != NULL) {  // OFS: assigned only in osdProcessStats2\n"
        "            value = osdEscDataCombined->temperature;\n"
        "            if (stats.max_esc_temp < value) {\n"
        "                stats.max_esc_temp = value;\n"
        "            }\n"
        "        }\n"
        "    } else",
    ),
])

print("add_fc_telemetry: edits applied")
