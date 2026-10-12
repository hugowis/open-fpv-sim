"""MSP_STATUS decode for the fault tests: Betaflight's own failsafe flags (m3c debt closed).

A real RX-loss failsafe never sets the BOXFAILSAFE pilot-switch bits (failsafe.c only reads
IS_RC_MODE_ACTIVE(BOXFAILSAFE)), so the decoded observable is the arming-disable flags:
RX_FAILSAFE (1 << 2) from stage 1 on, FAILSAFE (1 << 1) once stage 2 engages
(Betaflight 2026.6.2: runtime_config.h:44-45; set at failsafe.c:195/214/231 and :490,
cleared on recovery at :204/:521).
"""
import socket

MSP_STATUS = 101
ARMING_DISABLED_FAILSAFE = 1 << 1
ARMING_DISABLED_RX_FAILSAFE = 1 << 2


class StatusClient:
    """One MSP_STATUS channel over the Configurator TCP port, pumped by simulated time."""

    def __init__(self, sim, timeout_s=2.0):
        port = int(sim.configurator_address.rsplit(":", 1)[1])
        self._sock = socket.create_connection(("127.0.0.1", port), timeout=timeout_s)
        self._sock.settimeout(timeout_s)
        self._sim = sim

    def _exchange(self, cmd):
        ck = cmd
        self._sock.sendall(b"$M<" + bytes([0, cmd]) + bytes([ck]))
        while True:
            self._sim.run(0.02)  # MSP is serviced only while simulated time advances
            b = self._sock.recv(4096)
            if b"$M>" in b:
                i = b.index(b"$M>")
                n, got = b[i + 3], b[i + 4]
                if got == cmd:
                    return b[i + 5:i + 5 + n]

    def arming_disable_flags(self):
        """One MSP_STATUS exchange: walks the pinned payload to the u32 LE arming-disable flags.

        Layout (Betaflight msp.c, case MSP_STATUS): [0..2] u16 cycle time, [2..4] u16 i2c errors,
        [4..6] u16 sensors, [6..10] u32 LE flight-mode flags, [10] u8 pid profile, [11..13] u16
        system load, [13..15] u16 gyro cycle time, [15] u8 extended-box byte count + that many
        bytes, then u8 arming-disable flag count, then the u32 LE flags.
        """
        payload = self._exchange(MSP_STATUS)
        i = 15
        i += payload[i] + 1  # skip the extended flight-mode-flag bytes
        count = payload[i]
        i += 1
        # The count byte names the flag-count width (30 in 2026.6.2), but msp.c's case MSP_STATUS
        # writes exactly the u32 LE flags after it (plus config-state bytes we must not pick up).
        return int.from_bytes(payload[i:i + 4], "little")

    def close(self):
        self._sock.close()
