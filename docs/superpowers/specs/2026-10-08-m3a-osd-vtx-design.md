# M3a: OSD and VTX control — design

Status: design approved in conversation 2026-10-08, awaiting written-spec review. Parent spec: `2026-10-04-open-fpv-sim-design.md` (§4.2 `ofs-video`, §5.4 return paths, §9 M3).

## 1. Purpose and scope

M3 of the parent spec (VTX model controlled via SmartAudio, real Betaflight OSD rendered, analog degradation driven by a link model) is split in two:

- **M3a (this spec): everything Betaflight-facing.** Betaflight's real OSD reaches the FPV view, the VTX is controlled by Betaflight over SmartAudio, and the battery reaches Betaflight as ESC telemetry so that the OSD shows real battery values.
- **M3b (later spec): the analog link model and the degradation shader**, driven by the VTX state that M3a publishes.

**Success looks like:** flying the Godot client against real Betaflight, the pilot sees Betaflight's own OSD (real font, live battery voltage, current and mAh, flight time, link quality, altitude, armed state, warnings, VTX channel and power) inside the FPV picture. The OSD stick menu works with the sticks. Changing the VTX channel or power from the OSD menu or from Betaflight Configurator's VTX tab changes the simulated VTX, and the HUD shows it. A low battery raises Betaflight's low-battery warning on the OSD.

### Decisions made in the design conversation

| # | Decision | Alternative rejected |
|---|---|---|
| 1 | Split M3 into M3a (this) and M3b. | One M3 covering all three pieces. |
| 2 | The battery is fed to Betaflight as simulated KISS ESC telemetry, so the OSD battery elements and the low-battery warning are live. | OSD without battery values (0.0 V). |
| 3 | UART TX bytes from Betaflight come back **inside the SITL reply datagram** (patch extension), deterministic and tick-bound. | TCP on the UART ports (wall-clock timing, no bit-identical lockstep OSD). Kept as the fallback for the OSD only if the reply hook fails (§7, step 1). |
| 4 | VTX protocol: **SmartAudio v2.1 only**. | Also Tramp (can be added later behind the same VTX model). |
| 5 | The OSD uses **Betaflight's real font** (converted from the official `.mcm`, GPL-3.0, license notice committed). | Placeholder monospace font. |
| 6 | The OSD is drawn **inside the FPV render path**: scene, lens, OSD, (M3b degradation slot), then HUD and menus. | A plain HUD layer on top (could never degrade). |

### Out of scope

Tramp; the link model and degradation shader (M3b); the RPM filter and DShot telemetry (still needs per-motor eRPM, as in M0); CRSF telemetry back to the radio (the HUD keeps reading the battery from the simulator's models); HD/digital OSD; VTX faults (M4); a thermal model; an OSD aspect-ratio setting.

## 2. Data flow

Per exchange (1 kHz in lockstep and real time):

- **Simulator to Betaflight** (the existing serial blocks in the state datagram, `EXT_SERIAL_MAX` 512 bytes shared by all UARTs):
  - UART2: CRSF (unchanged).
  - UART3: ESC telemetry (KISS frames) from the battery model.
  - UART5: SmartAudio replies from the VTX model.
- **Betaflight to simulator** (new, SITL patch): bytes Betaflight writes to its UARTs are buffered per UART inside SITL and returned in the reply datagram:
  - UART4: MSP DisplayPort (OSD frames).
  - UART5: SmartAudio requests.
  - Reply layout after the servo packet: `u16` bytes dropped since the previous reply (normally 0), then blocks `[uart index][len lo][len hi][bytes]`, at most 512 bytes in total. SITL drains its per-UART TX buffers across exchanges; nothing is dropped unless a buffer overflows, which is counted in the `u16`.
  - A reply that cannot be parsed is a firmware error that stops the simulator loudly.
- **Crates:**
  - `ofs-video` (new): `VtxModel`, the SmartAudio codec, the MSP DisplayPort decoder and the `OsdGrid`.
  - `ofs-fc`: splits the reply into UARTs; holds the KISS ESC-telemetry encoder.
  - `ofs-config`: schema 3 sections.
  - `ofs-sim`: wires the above into the vehicle and serves the new stream.
  - `ofs-proto`, `ofs-client`, `ofs-godot`, `godot/`, `python/ofs`: protocol 3 and the client side.

UART numbering: SITL binds UART1 to the MSP port (TCP 5761, Configurator); UART2 to UART5 are free. Config validation rejects a UART used twice and any UART outside 2–5.

## 3. OSD

**Decoder** (`ofs-video::osd`):
- A stream parser reads the UART4 bytes as MSP v1 frames (reusing the frame handling of `ofs-fc::msp` where it fits).
- It handles MSP DisplayPort (command 182): heartbeat, release, clear screen, write string (row, column, attribute, characters), draw screen, set options.
- Cells carry a character code, a font page and a blink flag. Clear and write modify a back buffer; **draw** publishes a snapshot, so a half-drawn screen is never shown.
- The grid is 30×16 (analog PAL) by default, settable in the quad file.
- A bad checksum drops the frame and increments a diagnostic counter.

**Stream:** `OsdFrame { seq, sim_time_s, cols, rows, cells, present }`.
- Published only when the cells changed, at most 60 Hz. A new subscriber receives the current frame immediately.
- `present = false` is sent when no draw has arrived for 1 s (OSD off, or the FC is rebooting).
- Lockstep runs produce identical frames.

**Godot:**
- The font is converted once from the Betaflight `.mcm` into a glyph atlas (12×18 px) by a committed script, with the license notice beside it.
- A `Control` draws the cells from the atlas. It redraws when `seq` changes or the blink phase flips.
- Layer order: 3D scene, lens distortion, **OSD**, (M3b degradation slot), then the HUD and menus. The lens is below the OSD because a real OSD is overlaid after the camera optics.
- The OSD sits in a centred 4:3 box fitted to the window height with nearest-neighbour scaling. It is hidden in the chase view and when `present` is false.

**Quad config:** the Betaflight diff enables the OSD elements: battery voltage, mAh used, current, flight time, link quality, altitude, craft name, armed state, warnings, VTX channel and power. The stick menu needs no new code (CRSF sticks already reach Betaflight).

## 4. VTX and SmartAudio

**`VtxModel`** (`ofs-video`): band (A, B, E, F, R, L), channel 1–8, frequency in MHz (48-entry table, checked against Betaflight's), power level with the power table (mW and dBm) from the quad file, pit mode, lock state, and power-up defaults.

**SmartAudio v2.1 codec:**
- Frames `AA 55 <cmd> <len> <payload> <CRC-8>`; the M0 request `aa 55 03 00 9f` is a get-settings.
- Answers get-settings, set-power, set-channel, set-frequency and set-mode; the v2.1 power table is part of the settings reply.
- A bad CRC or an unknown command is ignored (as a real VTX would) and counted.
- A short, configurable reply latency (default a few ms) stands in for the real VTX.
- Replies go into UART5 through the state datagram, so the exchange is deterministic.

**State and lifetime:**
- Volatile: `Load` resets the model to its defaults; a Betaflight reboot does not (a real VTX stays powered). At boot Betaflight pushes its saved band, channel and power, so the model follows the firmware's EEPROM.
- Frequency, power and pit mode are published as bus signals (input for M3b's link model) and in the state stream (`Vtx` message).
- A `vtx_changed` event ("R5 5806 MHz 200 mW") is emitted on every change. The HUD shows a VTX line and a toast.

**Absent VTX:** without a `[vtx]` section nothing answers on UART5 and Betaflight sees no VTX, as on hardware. A VTX that never answers is not an error.

## 5. Battery as ESC telemetry

- KISS frame, 10 bytes: temperature, voltage (0.01 V), current (0.01 A), consumed charge (mAh), eRPM, CRC-8 (polynomial 0x07).
- An encoder in `ofs-fc` writes one frame at a configurable rate (default 100 Hz) into UART3's serial link, deterministically via the state datagram.
- Betaflight's ESC sensor **sums** current and consumption over its four motor slots and **averages** voltage and RPM, and it files each frame under the motor it is currently polling, which the simulator cannot see. Every frame therefore carries **a quarter of the pack's current and consumed charge** and the pack's terminal voltage, so the sums are right whichever slot a frame lands in. The first reading is low until each slot has seen a frame (about a quarter of a second).
- Temperature is a constant 25 °C (no thermal model). RPM is the mean motor speed, for the OSD and `MSP_MOTOR_TELEMETRY`; the per-motor RPM filter stays out of scope.
- The diff gets `feature ESC_SENSOR`, UART3 with the ESC-sensor function, and the voltage and current meter sources set to ESC.
- The values come from the battery model's terminal voltage, pack current and consumed-charge signals.

## 6. Config, API and clients

**Quad file (schema 3):** optional `[vtx]` (UART, band, channel, power table, defaults, reply latency), `[osd]` (UART, grid size) and `[esc_telemetry]` (UART, rate). Schema-2 files still load and have no VTX, OSD or ESC telemetry. The shipped quad file and its Betaflight diff are updated: serial ports (UART3 ESC sensor, UART4 DisplayPort, UART5 SmartAudio), `feature OSD`, the OSD element positions, `vcd_video_system = PAL` and `force_battery_cell_count`. The SITL build has no `vtxtable`: Betaflight uses the factory bands and builds its power list from the dBm values the VTX reports, so the diff has no `vtxtable` lines (plan, "Rulings and facts verified in advance"). As today, the diff is applied on first boot only; an existing `eeprom.bin` must be deleted.

**Protocol 3:**
- New stream `StreamOsd` sending `OsdFrame`.
- State gains a `Vtx` message: band, channel, frequency, power, pit mode, present.
- New events `vtx_changed` and `serial_overflow`.
- All clients move to protocol 3 together.

**Clients:**
- Python: `get_osd()` returns the grid as text rows; `state.vtx`.
- `ofs-client` and the `OfsClient` node: `get_osd()` and the VTX fields, polled by sequence number.
- Godot HUD: a VTX line and a toast on change.

## 7. Order of work, failure handling and tests

**Order, riskiest first:**
1. **SITL patch and live probes.** Extend the patch so UART TX bytes ride in the reply (generator script like the earlier ones). Live probes decide go or no-go: (a) the reply hook carries DisplayPort frames within the 512-byte cap, with overflow counted; (b) Betaflight's SmartAudio driver accepts a reply from a TCP UART (half-duplex handling); (c) the ESC sensor with ESC meter sources produces the expected voltage, current and mAh. If (a) fails, fall back to TCP for the OSD only and record a ruling.
2. Config schema 3, the quad file and the Betaflight diff.
3. `ofs-fc`: reply demux and the KISS encoder.
4. `ofs-video`: SmartAudio codec, `VtxModel`, OSD decoder.
5. `ofs-sim` wiring, protocol 3, Python.
6. `ofs-client`, `ofs-godot`, the Godot OSD layer, the font atlas script, the HUD VTX line, docs, the manual check.

**Failure handling:** a malformed reply datagram is a firmware error (simulator failure, stops loudly). A corrupted OSD or SmartAudio frame is skipped and counted. Overflow raises `serial_overflow`. A bad VTX power list, an unknown band, a duplicate UART, or a diff that enables SmartAudio without a `[vtx]` section (Betaflight SITL crashes when its OSD shows the VTX channel and no VTX answers) is a config error at load.

**Confirmed against the pinned Betaflight source and live SITL while the plan was written (results in the plan's "Rulings and facts verified in advance"; the original list is kept for the record):** the DisplayPort attribute bits (blink, font page) and grid size; the exact SmartAudio frame bytes (`vtx_smartaudio.c`) and v2.1 power-table fields; how the ESC sensor turns frames into voltage, current and mAh; the exact low-battery warning text; which font files the Configurator repository provides (one page or two).

**Tests:**
- **Unit:** golden-byte tests for the SmartAudio codec, the DisplayPort decoder and the KISS encoder; `VtxModel` (table, power, pit); config validation.
- **Live SITL** (`sitl_live` and the Python SITL tests, run in the CI `sitl` job): Betaflight accepts a SmartAudio reply; `MSP_SET_VTX_CONFIG` (the Configurator VTX tab) moves the model within a second; the OSD battery text matches the battery model; a low-charge quad shows the low-battery warning; a stick command opens the OSD menu; two lockstep runs give identical OSD frames.
- **Godot:** grid state, blink, glyph lookup and chase-view hiding (headless has no renderer, so no pixel checks); the Betaflight flight test extended with OSD and HUD VTX checks; the screenshot script extended for a manual look.
- **CI:** no new jobs.

**Docs:** `dev-setup.md` (OSD, VTX, regenerating the patch), `docs/research/sitl-interface.md` (a new M3a section with the probe results), `README.md`.

## 8. Risks

| Risk | Mitigation |
|---|---|
| The SITL reply hook adds patch size and may overflow at 1 kHz. | Step 1 go/no-go; bounded buffers with a counted overflow; TCP fallback for the OSD only. |
| Betaflight's SmartAudio driver may not accept replies over a TCP UART. | Probe (b) first; the patch may need a half-duplex no-op alongside the existing serial no-ops. |
| ESC sensor sum/average semantics produce wrong battery values. | Quarter-current frames, verified live against the battery model. |
| Font licensing and conversion. | Official GPL-3.0 font, notice committed, conversion script committed; the download is approved by the user before it is fetched. |
| An existing `eeprom.bin` hides the new diff. | The stale-diff guard already refuses to load; documented. |
