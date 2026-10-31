# iBUS receiver implementation plan

Spec: `docs/superpowers/specs/2026-10-26-ibus-receiver-design.md`. Test first; one commit per task.

1. **Spec and plan** (this commit), with the SITL patch line `#define USE_SERIALRX_IBUS` (needed by task 5; built and
   applied in a separate WSL directory so the shared SITL binary is untouched).
2. **Codec, test first.** `crates/ofs-radio/tests/ibus.rs`: golden frame, round trip, checksum, errors, mapping. Then
   `crates/ofs-radio/src/ibus.rs` (`frame`, `decode`, `checksum`, `ticks_to_us`, `us_to_ticks`) until green.
3. **Receiver, test first.** Receiver tests in the same file (pacing at 500, 250, 150 Hz, loss, latest channels, 14
   channels), then `IbusReceiver` in `ibus.rs`.
4. **Config and wiring.** `ReceiverKind::Ibus` in `ofs-config`, the match arm in `radio_chain` (`ofs-sim`), a config
   test (kind parses; the iBUS quad loads), the example quad `quads/opendrone-5f-ibus.toml` and its diff.
5. **Live check.** An ignored `ofs-fc` test: iBUS sticks on UART2 of the rebuilt SITL read back over `MSP_RC`, with an
   iBUS diff in `crates/ofs-fc/tests/data`. Record the result in `docs/research/ibus-receiver.md`.
6. **Docs.** `docs/research/ibus-receiver.md` (facts, provenance, live result), `docs/writing-a-radio-model.md`,
   `docs/roadmap.md`, `README.md` and `CHANGELOG.md` (Unreleased), the SITL docs (`docs/research/sitl-interface.md`,
   `docs/dev-setup.md` if they list receivers).
7. **Gates.** `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
   `cargo fmt --check`, `python -m pytest python/tests`. Godot suites only if the protocol changed (it does not).
