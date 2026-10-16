# The handset over USB serial (M5's capture)

Date 2026-10-15. Handset: RadioMaster TX15 (EdgeTX), USB `0483:5740`. Host: Windows 11 and WSL2 Ubuntu.

M5 was first specified as "an ELRS handset in simulator mode speaks CRSF over USB serial; the simulator reads its
channel frames". The capture-first rule (spec §2 of the serial-radio design) tested that premise before building
anything, and it did not hold.

## What was found

- **The port.** In "USB Serial (VCP)" mode the radio enumerates as a composite device with two interfaces. Windows
  binds `usbser` to the first and cannot start it (`CM_PROB_FAILED_START`, status `0xC000000E`); the second
  ("TX15 Serial Port") gets no driver (`CM_PROB_FAILED_INSTALL`). A different cable, port, power cycle, and forcing
  the Microsoft serial driver onto the composite parent changed nothing.
- **From Linux it opens.** With `usbipd-win` sharing the device into WSL (`usbipd bind --busid <id>` once, as
  administrator, then `usbipd attach --wsl Ubuntu --busid <id>`), the WSL kernel's `cdc_acm` creates `/dev/ttyACM0`.
  The device node needs `chmod a+rw` or the `dialout` group.
- **The port is quiet.** At 420000 and 115200 baud, nothing arrived in 4 s with the radio idle. In EdgeTX's
  `USB-VCP` setting (SYS > Hardware: CLI, Lua, Telemetry Mirror) set to CLI the port answers `help`
  (`help [<command>]`): it is a console, not a stream of channel frames. EdgeTX offers no VCP mode that streams the
  radio's CRSF channels; Telemetry Mirror copies telemetry the other way.
- **How simulators read a radio.** Over USB joystick (HID) mode, which the project already supports.

## What it means

- No serial radio in M5: the joystick stays the input, and M5 became swappable radio components and live-flight
  recording (`docs/superpowers/specs/2026-10-15-m5-radio-components-design.md`, `docs/writing-a-radio-model.md`).
- **Untested routes** (roadmap): the external module bay speaks CRSF (a half-duplex single wire) to whatever is
  plugged in, so a PC with a USB-UART adapter and a bay breakout could play the module; and an EdgeTX Lua script
  could stream channel values over `serialWrite` with USB-VCP in Lua mode (the docs list the functions but not which
  port they use). Neither was tried.
- The Windows driver failure is specific to this radio's descriptors; the Linux path works through `usbipd`.
