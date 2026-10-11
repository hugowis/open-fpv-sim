"""Adds the simulator's per-motor eRPM block to a Betaflight tree that already has ofs-sitl.patch
and add_fc_telemetry.py applied. The state datagram may carry one reserved block after the UART
blocks — the same [index][len lo][len hi][bytes] format with index 250 — carrying motor 0..3's
eRPM/100 as u16 little-endian. SITL stages it with the packet and the DShot-telemetry stub feeds it
to the RPM filter each PID loop, replacing Task 1's synthetic feed. See the M3d spec §3.

Run AFTER add_fc_telemetry.py; the generators are NOT idempotent.
Usage: python3 third_party/betaflight/tools/add_erpm_block.py ~/ofs/betaflight
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


# 1. The staging statics, next to the serial staging.
edit("src/platform/SIMULATOR/sitl.c", [
    (
        "static uint8_t extSerialPending[EXT_SERIAL_MAX];\nstatic int extSerialPendingLen = 0;",
        "static uint8_t extSerialPending[EXT_SERIAL_MAX];\nstatic int extSerialPendingLen = 0;\n"
        "// The simulator's per-motor eRPM block (index 250), staged with the packet like the serial\n"
        "// bytes. Main thread only (extInjectSerial runs there, as does the consumer in pwmCompleteMotorUpdate).\n"
        "static uint16_t extErpm[4];\nstatic bool extErpmValid = false;",
    ),
])

# 2. extInjectSerial: validity is per datagram (a datagram without the block leaves the stub at
#    zero, per the spec), and the reserved index is intercepted before the UART lookup (a legacy
#    UART id of 250 does not exist; the block must not be dropped as "no such open UART").
edit("src/platform/SIMULATOR/sitl.c", [
    (
        "static void extInjectSerial(const uint8_t *p, int len)\n{\n    int i = 0;",
        "static void extInjectSerial(const uint8_t *p, int len)\n{\n    int i = 0;\n"
        "    extErpmValid = false;  // the eRPM block is per datagram; the bridge sends it every datagram",
    ),
    (
        "        if (n > len - i) {\n"
        "            printf(\"[SITL] malformed serial block for UART%u: %d bytes announced, %d left; rest dropped\\n\", uart + 1, n, len - i);\n"
        "            return;\n"
        "        }\n"
        "        if (!tcpSerialInject(uart, &p[i], n)) {",
        "        if (n > len - i) {\n"
        "            printf(\"[SITL] malformed serial block for UART%u: %d bytes announced, %d left; rest dropped\\n\", uart + 1, n, len - i);\n"
        "            return;\n"
        "        }\n"
        "        if (uart == 250) {  // M3d: the simulator's per-motor eRPM block\n"
        "            if (n != 8) {\n"
        "                printf(\"[SITL] malformed eRPM block: %d bytes, expected 8; dropped\\n\", n);\n"
        "            } else {\n"
        "                memcpy(extErpm, &p[i], sizeof(extErpm));\n"
        "                extErpmValid = true;\n"
        "            }\n"
        "            i += n;\n"
        "            continue;\n"
        "        }\n"
        "        if (!tcpSerialInject(uart, &p[i], n)) {",
    ),
])

# 3. The stub consumes the staged values instead of the synthetic feed.
edit("src/platform/SIMULATOR/sitl.c", [
    (
        "static void sitlDshotTelemetryFeed(void)\n"
        "{\n"
        "    for (int i = 0; i < 4; i++) {  // servo_packet carries 4 motor values\n"
        "        const float throttle = pwmPkt.motor_speed[i];  // throttle 0..1 ((us - 1000) / 1000)\n"
        "        const uint32_t erpm = (uint32_t)(throttle * 120000.0f * (1.0f + 0.25f * i));\n"
        "        sitlDshotErpm[i] = erpm;\n"
        "        sitlMotorFrequencyHz[i] = erpmToRpm(erpm / 100U) / 60.0f;\n"
        "    }\n"
        "    dshotTelemetryState.readCount++;\n"
        "}",
        "static void sitlDshotTelemetryFeed(void)\n"
        "{\n"
        "    for (int i = 0; i < 4; i++) {  // the datagram block's eRPM/100 (zeros until one arrives)\n"
        "        sitlDshotErpm[i] = extErpmValid ? (uint32_t)extErpm[i] * 100U : 0U;\n"
        "        sitlMotorFrequencyHz[i] = erpmToRpm(sitlDshotErpm[i] / 100U) / 60.0f;\n"
        "    }\n"
        "    dshotTelemetryState.readCount++;\n"
        "}",
    ),
    (
        "bool isDshotMotorTelemetryActive(uint8_t motorIndex) { return sitlDshotErpm[motorIndex] > 0; }",
        "bool isDshotMotorTelemetryActive(uint8_t motorIndex) { UNUSED(motorIndex); return extErpmValid; }",
    ),
])

print("add_erpm_block: edits applied")
