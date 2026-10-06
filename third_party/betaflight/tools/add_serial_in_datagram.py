"""Adds "UART bytes in the state datagram" to a Betaflight tree that already has ofs-sitl.patch applied.

The simulator appends blocks of [uart index][length lo][length hi][bytes] after the 184-byte state datagram;
SITL stages them with the packet and hands them to the UART on the tick that applies it, so receiver traffic
(CRSF) is deterministic in lockstep. See docs/research/sitl-interface.md §8.

Usage (Linux or WSL), from the repository root:
    bash scripts/build-sitl.sh                                   # tree at the pinned commit + current patch
    python3 third_party/betaflight/tools/add_serial_in_datagram.py ~/ofs/betaflight
    git -C ~/ofs/betaflight diff > third_party/betaflight/ofs-sitl.patch
    bash scripts/build-sitl.sh                                   # rebuild from the regenerated patch
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


edit("src/main/drivers/serial_tcp.c", [(
    r"""            s->port.rxCallback(ch, s->port.rxCallbackData);
        }
    }
}
""",
    r"""            s->port.rxCallback(ch, s->port.rxCallbackData);
        }
    }
}

// Lockstep builds: bytes for UART `id` (0-based) that the simulator sent inside the state datagram. They join
// the port's RX buffer like bytes from a TCP client and reach the driver at this tick's tcpSerialDispatchRx().
void tcpSerialInject(unsigned id, const uint8_t *data, int size)
{
    if (id >= ARRAYLEN(tcpSerialPorts) || !tcpPortInitialized[id] || size <= 0) {
        return;
    }
    tcpDataIn(&tcpSerialPorts[id], (uint8_t *)data, size);
}
""")])

edit("src/main/drivers/serial_tcp.h", [(
    "void tcpSerialDispatchRx(void);  // lockstep: deliver buffered RX bytes on the main thread\n",
    "void tcpSerialDispatchRx(void);  // lockstep: deliver buffered RX bytes on the main thread\n"
    "void tcpSerialInject(unsigned id, const uint8_t *data, int size);  // lockstep: UART bytes from the state datagram\n",
)])

edit("src/platform/SIMULATOR/sitl.c", [
    (
        "static uint16_t extFdmRcChannels[SIMULATOR_MAX_RC_CHANNELS];\n",
        r"""static uint16_t extFdmRcChannels[SIMULATOR_MAX_RC_CHANNELS];

// UART bytes carried in the state datagram after the rc_packet: blocks of [uart index][len lo][len hi][bytes].
// Staged with the packet and handed to the UARTs on the tick that applies it, so receiver traffic (CRSF) is
// deterministic in lockstep. FDM thread: extFdmSerial*; under extPacketMutex: extSerialPending*.
#define EXT_SERIAL_MAX 512
static uint8_t extFdmSerial[EXT_SERIAL_MAX];
static int extFdmSerialLen = 0;
static uint8_t extSerialPending[EXT_SERIAL_MAX];
static int extSerialPendingLen = 0;

static void extInjectSerial(const uint8_t *p, int len)
{
    int i = 0;
    while (i + 3 <= len) {
        const unsigned uart = p[i];
        const int n = p[i + 1] | (p[i + 2] << 8);
        i += 3;
        if (n > len - i) {
            break;
        }
        tcpSerialInject(uart, &p[i], n);
        i += n;
    }
}
""",
    ),
    (
        """    uint16_t rc[SIMULATOR_MAX_RC_CHANNELS];
    bool rcNew = false;
""",
        """    uint16_t rc[SIMULATOR_MAX_RC_CHANNELS];
    bool rcNew = false;
    uint8_t serial[EXT_SERIAL_MAX];
    int serialLen = 0;
""",
    ),
    (
        """            extRcPending = false;
        }
    }
    pthread_mutex_unlock(&extPacketMutex);
""",
        """            extRcPending = false;
        }
        serialLen = extSerialPendingLen;
        if (serialLen > 0) {
            memcpy(serial, extSerialPending, serialLen);
            extSerialPendingLen = 0;
        }
    }
    pthread_mutex_unlock(&extPacketMutex);
""",
    ),
    (
        """    tcpSerialDispatchRx();
    extReplyPending = true;
""",
        """    extInjectSerial(serial, serialLen);
    tcpSerialDispatchRx();
    extReplyPending = true;
""",
    ),
    (
        """        extFdmRcValid = false;
    }
    extGyroTicks++;
""",
        """        extFdmRcValid = false;
    }
    if (extFdmSerialLen > 0) {
        const int room = EXT_SERIAL_MAX - extSerialPendingLen;
        const int n = extFdmSerialLen < room ? extFdmSerialLen : room;
        memcpy(extSerialPending + extSerialPendingLen, extFdmSerial, n);
        extSerialPendingLen += n;
        extFdmSerialLen = 0;
    }
    extGyroTicks++;
""",
    ),
    (
        """        static struct { fdm_packet fdm; rc_packet rc; } __attribute__((packed)) fdmRcPkt;
        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);
        if (n == sizeof(fdmRcPkt)) {
            memcpy(extFdmRcChannels, fdmRcPkt.rc.channels, sizeof(extFdmRcChannels));
            extFdmRcValid = true;
        }
        if (n == sizeof(fdm_packet) || n == sizeof(fdmRcPkt)) {
""",
        """        // UART blocks may follow the rc_packet (see EXT_SERIAL_MAX).
        static struct { fdm_packet fdm; rc_packet rc; uint8_t serial[EXT_SERIAL_MAX]; } __attribute__((packed)) fdmRcPkt;
        const int fdmRcSize = (int)(sizeof(fdm_packet) + sizeof(rc_packet));
        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);
        if (n >= fdmRcSize) {
            memcpy(extFdmRcChannels, fdmRcPkt.rc.channels, sizeof(extFdmRcChannels));
            extFdmRcValid = true;
            extFdmSerialLen = n - fdmRcSize;
            memcpy(extFdmSerial, fdmRcPkt.serial, extFdmSerialLen);
        }
        if (n == (int)sizeof(fdm_packet) || n >= fdmRcSize) {
""",
    ),
])
print("serial-in-datagram applied to", ROOT)
