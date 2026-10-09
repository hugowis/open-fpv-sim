"""Adds "UART TX bytes in the reply datagram" to a Betaflight tree that already has ofs-sitl.patch applied.

In lockstep builds, what Betaflight writes to UART2 and up (OSD over MSP DisplayPort, SmartAudio requests, ...) is
captured into per-port buffers instead of waiting for a TCP client. When SITL replies to a state datagram
(extSendReply, after every task due at that simulated instant has run) the reply carries:
    [servo_packet (16 bytes)][dropped: u16 LE][blocks: [uart index (0-based)][length u16 LE][bytes] ...]
with at most EXT_SERIAL_MAX (512) bytes of blocks. Bytes that do not fit wait for the next reply; bytes that do
not fit in the 4096-byte capture buffer are dropped and counted in `dropped`. UART1 (MSP, Configurator) is never
captured. It also defines USE_OSD_SD for SITL: without it displayPortMspInit() falls through both of its SD/HD
fallbacks and always leaves vcd_video_system at AUTO (a 13-row NTSC OSD) whatever the quad's diff sets. See docs/superpowers/specs/2026-10-08-m3a-osd-vtx-design.md.

Usage (Linux or WSL), from the repository root, on a tree that has the CURRENT ofs-sitl.patch applied:
    bash scripts/build-sitl.sh                                   # applies the current patch to ~/ofs/betaflight
    python3 third_party/betaflight/tools/add_serial_out_datagram.py ~/ofs/betaflight
    git -C ~/ofs/betaflight diff > third_party/betaflight/ofs-sitl.patch
    bash scripts/build-sitl.sh                                   # rebuild from the regenerated patch
The generator is NOT idempotent: running it on a tree that already has this change fails on its anchors.
"""
import sys

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


edit("src/main/drivers/serial_tcp.c", [
    (
        '#include "io/serial.h"\n#include "serial_tcp.h"\n',
        '#include <string.h>\n\n#include "io/serial.h"\n#include "serial_tcp.h"\n',
    ),
    (
        r"""void tcpDataOut(tcpPort_t *instance)
{
    tcpPort_t *s = (tcpPort_t *)instance;
    if (s->conn == NULL) return;
""",
        r"""#if ENABLE_SIMULATOR_EXTERNAL_TIME
// Lockstep builds: what Betaflight writes to UART2 and up is captured for the simulator instead of waiting for a TCP
// client (UART1 stays the real MSP port for Configurator). tcpSerialTakeTx() hands the bytes to the reply datagram.
#define TX_CAPTURE_SIZE 4096
static uint8_t txCapture[SERIAL_PORT_COUNT][TX_CAPTURE_SIZE];
static unsigned txCaptureLen[SERIAL_PORT_COUNT];
static unsigned txCaptureDropped;
static pthread_mutex_t txCaptureLock = PTHREAD_MUTEX_INITIALIZER;

static void tcpCaptureTx(tcpPort_t *s)
{
    const unsigned id = s->id;
    if (id >= SERIAL_PORT_COUNT) {
        return;
    }
    pthread_mutex_lock(&s->txLock);
    pthread_mutex_lock(&txCaptureLock);
    while (s->port.txBufferTail != s->port.txBufferHead) {
        if (txCaptureLen[id] < TX_CAPTURE_SIZE) {
            txCapture[id][txCaptureLen[id]++] = s->port.txBuffer[s->port.txBufferTail];
        } else if (txCaptureDropped < 0xFFFF) {
            txCaptureDropped++;
        }
        if (++s->port.txBufferTail >= s->port.txBufferSize) {
            s->port.txBufferTail = 0;
        }
    }
    pthread_mutex_unlock(&txCaptureLock);
    pthread_mutex_unlock(&s->txLock);
}

// Fills `out` (at most `room` bytes) with blocks [uart index][len lo][len hi][bytes] for UART2 and up, oldest bytes
// first, and returns their size. `*dropped` receives the bytes dropped since the previous call.
int tcpSerialTakeTx(uint8_t *out, int room, uint16_t *dropped)
{
    int used = 0;
    pthread_mutex_lock(&txCaptureLock);
    *dropped = (uint16_t)txCaptureDropped;
    txCaptureDropped = 0;
    for (unsigned id = 1; id < SERIAL_PORT_COUNT; id++) {
        const int header = 3;
        if (txCaptureLen[id] == 0 || room - used <= header) {
            continue;
        }
        int n = (int)txCaptureLen[id];
        if (n > room - used - header) {
            n = room - used - header;
        }
        out[used] = (uint8_t)id;
        out[used + 1] = (uint8_t)(n & 0xFF);
        out[used + 2] = (uint8_t)(n >> 8);
        memcpy(out + used + header, txCapture[id], n);
        used += header + n;
        txCaptureLen[id] -= (unsigned)n;
        memmove(txCapture[id], txCapture[id] + n, txCaptureLen[id]);
    }
    pthread_mutex_unlock(&txCaptureLock);
    return used;
}
#endif

void tcpDataOut(tcpPort_t *instance)
{
    tcpPort_t *s = (tcpPort_t *)instance;
#if ENABLE_SIMULATOR_EXTERNAL_TIME
    if (s->id != 0) {
        tcpCaptureTx(s);
        return;
    }
#endif
    if (s->conn == NULL) return;
""",
    ),
])

edit("src/main/drivers/serial_tcp.h", [(
    "void tcpSerialInject(unsigned id, const uint8_t *data, int size);  // lockstep: UART bytes from the state datagram\n",
    "void tcpSerialInject(unsigned id, const uint8_t *data, int size);  // lockstep: UART bytes from the state datagram\n"
    "int tcpSerialTakeTx(uint8_t *out, int room, uint16_t *dropped);  // lockstep: UART2+ TX blocks for the reply datagram\n",
)])

edit("src/platform/SIMULATOR/sitl.c", [(
    r"""    extBusyPasses = 0;
    udpSend(&pwmLink, &pwmPkt, sizeof(servo_packet));
""",
    r"""    extBusyPasses = 0;
    // The reply is the servo packet, a dropped-bytes count, then the TX blocks of UART2 and up (tcpSerialTakeTx).
    static struct { servo_packet servo; uint16_t dropped; uint8_t serial[EXT_SERIAL_MAX]; } __attribute__((packed)) reply;
    reply.servo = pwmPkt;
    uint16_t dropped = 0;
    const int used = tcpSerialTakeTx(reply.serial, EXT_SERIAL_MAX, &dropped);
    reply.dropped = dropped;
    udpSend(&pwmLink, &reply, sizeof(servo_packet) + sizeof(uint16_t) + used);
""",
)])
edit("src/platform/SIMULATOR/target/SITL/target.h", [(
    "#define USE_OSD\n#ifndef USE_MSP_DISPLAYPORT\n",
    "#define USE_OSD\n#define USE_OSD_SD\n#ifndef USE_MSP_DISPLAYPORT\n",
)])
print("ok")
