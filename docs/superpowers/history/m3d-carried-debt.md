> Retired at the v1 close-out (2026-10-13): every item here now lives in `docs/roadmap.md` or was fixed (see
> `CHANGELOG.md`); this file is kept as history.

# M3d carried debt

These items were deliberately left out of M3d, or surfaced while building it. They are listed so M4 planning and the maintainer can pick them up.

## Deferred features
- **The RPM filter's spectral proof.** The filter is structurally on (the stub's `useDshotTelemetry` gate, per-motor Hz fed from the datagram block); a Blackbox-based proof that the gyro's motor-frequency notch actually appears was skipped as too costly for the value (the M3d spec's §7 left it to the plan).
- **DShot extended telemetry and MSP-over-CRSF.** The CRSF telemetry carries battery, attitude, mode, baro, mag and vario; per-ESC voltage/current/temperature (EDT) and MSP-over-CRSF stay out with the DShot driver itself.
- **The telemetry slot ratio is config-less.** `TELEMETRY_SLOT_DIVISOR = 4` is a named constant with its estimate recorded in docs/research/elrs-link.md; expose it in the quad file only if a user misses it.

## Modelling simplifications (estimates, see docs/research/elrs-link.md)
- The telemetry slots are an estimated 1:4 ratio of the downlink; real ExpressLRS negotiates the ratio dynamically.
- The RC/telemetry UART is shared with no contention model: Betaflight's own scheduler interleaves its telemetry with the receiver's traffic, and the sim captures whatever it wrote.
- The simulator's pole pairs are the named constant 7 (`ofs-fc::sitl::esc_telemetry::POLE_PAIRS`, the shipped quad's 14-pole motors) while Betaflight's come from its `motor_poles` setting; a diff with motor_poles != 14 silently scales the RPM filter's input by 14/poles.

## Rulings made while planning
- **The motor protocol stays PWM** (M3d spec decision 2): the spike proved the RPM filter needs no DShot driver, so the M3c milestone table's "motor-protocol change" note is retired.
- **The FC telemetry is not part of the lockstep digest.** It crosses to clients through a shared handle (the OSD pattern); determinism is pinned by the ELRS byte-stream test and the live runs.
- **Battery values pass through Betaflight verbatim**, including the ESC-sensor consumption summed over motor slots (x4 on a 4-motor quad) and `getLegacyBatteryVoltage()`'s 0.1 V units.
- **A stale SITL binary is refused loudly but harmlessly**: it prints one `[SITL] … no such open UART` line per datagram for the eRPM block and flies without motor telemetry; `dev-setup.md` says to rebuild.
