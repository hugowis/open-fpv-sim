"""THROWAWAY: Task 5 feature changes on top of the external-time patch (target.h, common_post.h, target.mk, sitl.c)."""
import sys

root = sys.argv[1]


def edit(rel, fn):
    p = f"{root}/{rel}"
    s = open(p).read()
    s2 = fn(s)
    assert s2 != s, f"no change in {rel}"
    open(p, "w").write(s2)


def target_h(s):
    for m in ["USE_SERIALRX", "USE_SERIALRX_CRSF", "USE_OSD", "USE_VTX_COMMON",
              "USE_VTX_CONTROL", "USE_VTX_SMARTAUDIO", "USE_VTX_TRAMP", "USE_CMS"]:
        line = f"#undef {m}\n"
        assert line in s, m
        s = s.replace(line, "", 1)
    anchor = "#undef USE_SPI\n"
    assert anchor in s
    return s.replace(anchor, anchor + """
// Open FPV Sim: peripherals simulated over TCP UARTs (CRSF receiver, ESC telemetry, MSP DisplayPort OSD, VTX control)
#define USE_SERIALRX
#define USE_SERIALRX_CRSF
// USE_TELEMETRY_CRSF stays undefined: telemetry/crsf.c needs an ATOMIC_BLOCK shim for SITL (future work)
#ifndef USE_ESC_SENSOR
#define USE_ESC_SENSOR
#endif
#define USE_OSD
#ifndef USE_MSP_DISPLAYPORT
#define USE_MSP_DISPLAYPORT
#endif
#define USE_CMS
#define USE_OSD_OVER_MSP_DISPLAYPORT
#define USE_VTX_COMMON
#define USE_VTX_CONTROL
#define USE_VTX_SMARTAUDIO
#define USE_VTX_TRAMP
""", 1)


def common_post(s):
    old = "#ifndef USE_DSHOT\n#undef USE_ESC_SENSOR\n#endif\n"
    assert old in s
    return s.replace(old, "#if !defined(USE_DSHOT) && !defined(SIMULATOR_BUILD)  // SITL: ESC telemetry is simulated over a TCP UART\n#undef USE_ESC_SENSOR\n#endif\n", 1)


SITL_STUBS = r"""uint32_t microsISR(void)  // used by serial RX drivers (e.g. CRSF frame timing)
{
    return micros();
}

#if defined(USE_ESC_SENSOR) && !defined(USE_DSHOT)
// ESC telemetry without DShot: the simulator streams KISS telemetry frames to the ESC sensor UART.
// These stand in for the helpers in drivers/dshot.c (compiled only with USE_DSHOT); same math.
bool useDshotTelemetry = false;
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
#endif

"""


def sitl_c(s):
    anchor = "uint32_t millis(void)\n{"
    assert anchor in s and "microsISR" not in s
    return s.replace(anchor, SITL_STUBS + anchor, 1)


def serial_tcp_c(s):
    old_vt = "static const struct serialPortVTable tcpVTable = {"
    assert old_vt in s
    s = s.replace(old_vt, r"""// Baud rate and mode are meaningless over TCP, but drivers such as SmartAudio call these
// unconditionally (a NULL entry crashed SITL), so accept and record them.
static void tcpSetBaudRate(serialPort_t *instance, uint32_t baudRate)
{
    instance->baudRate = baudRate;
}

static void tcpSetMode(serialPort_t *instance, portMode_e mode)
{
    instance->mode = mode;
}

""" + old_vt, 1)
    s = s.replace("        .serialSetBaudRate = NULL,\n", "        .serialSetBaudRate = tcpSetBaudRate,\n", 1)
    s = s.replace("        .setMode = NULL,\n", "        .setMode = tcpSetMode,\n", 1)
    assert "tcpSetBaudRate," in s and "tcpSetMode," in s
    # Interrupt-driven RX drivers (CRSF, ESC sensor, ...) register an rxCallback and never poll the
    # buffer; on hardware the UART ISR calls it per byte. Do the same from the TCP thread.
    old_in = """    pthread_mutex_lock(&s->rxLock);

    while (size--) {"""
    assert old_in in s
    s = s.replace(old_in, """    if (s->port.rxCallback) {
        while (size--) {
            s->port.rxCallback(*(ch++), s->port.rxCallbackData);
        }
        return;
    }

    pthread_mutex_lock(&s->rxLock);

    while (size--) {""", 1)
    return s


def osd_c(s):
    # Upstream bug: devices without a size (e.g. SITL's VIRTUAL blackbox) give storageTotal == 0 and the
    # percentage divides by zero; the compiler turns that path into a trap (SIGILL on the disarm stats screen).
    old = """    if (storageDeviceIsWorking) {
        const uint16_t storageUsedPercent = (storageUsed * 100) / storageTotal;"""
    assert old in s
    return s.replace(old, """    if (storageDeviceIsWorking && storageTotal == 0) {
        tfp_sprintf(buff, "OK");
    } else if (storageDeviceIsWorking) {
        const uint16_t storageUsedPercent = (storageUsed * 100) / storageTotal;""", 1)


def sitl_mk(s):
    old = "        telemetry/crsf.c \\\n"
    assert old in s
    return s.replace(old, "", 1)  # CRSF telemetry is simulated over a TCP UART


edit("src/main/osd/osd.c", osd_c)
edit("src/main/drivers/serial_tcp.c", serial_tcp_c)
edit("src/platform/SIMULATOR/target/SITL/target.h", target_h)
edit("src/main/target/common_post.h", common_post)
edit("src/platform/SIMULATOR/sitl.c", sitl_c)
print("features patched", root)
