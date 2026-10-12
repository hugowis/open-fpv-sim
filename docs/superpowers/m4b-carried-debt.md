# M4b carried debt

## Deferred features
- **Windmilling drag on a dead motor** — a scale-0 motor freewheels through prop drag only.
- **Per-cell battery model** — WeakCell is additive on the pack Thevenin; capacity fade stays out.
- **Vibration transmissibility** — rigid-body, below-resonance only; no structural model.
- **RadioLoss on the downlink** — only the uplink fraction is modelled; the downlink keeps the
  link model's own error rate (the telemetry slot ratio is unaffected).
- **Vibration on mixed pole-pair open-loop builds** — theta is per-motor and honest, but the
  rate disturbance ignores gyroscopic coupling (pre-existing physics simplification).
- **`test_sitl_link.py` is still not in the sitl job's list** (inherited m3e debt).
- **The radio-fault no-op test cannot prove stream provenance** — `zero_fraction_is_a_no_op` runs with fading
  off, where a leaked draw would shift both rigs identically and still pass. One follow-up test in
  `the_loss_draws_do_not_depend_on_the_transmitter`'s shape (fading on, at the sensitivity edge, varying
  `fault.radio.loss_fraction`) would pin that the model's own stream never moves; the draw-site comment that
  overclaims this softens with it (surfaced in Task 4).
- **The burst validation message says "on_ms and off_ms" while the parameters are seconds** — the
  `crates/ofs-sim/src/vehicle.rs` message itself prints `got on {on_s} s / off {off_s} s`; rename the words with
  the next grep of its test (surfaced in Task 4).
- **`msp_status.py`'s `_exchange` has no partial-frame buffering** — one `recv` must carry the whole `$M>` frame;
  loopback lockstep makes that hold, a real network hop would not (surfaced in Task 7).
- **`status.close()` is not in try/finally** in `test_burst_loss_failsafes_and_betaflight_flags_it` — a failed
  assertion leaks the socket (surfaced in Task 7).
- **`import ofs` in `test_sitl_faults.py` is unused** — the faults come from `ofs.faults` directly; the import is
  brief-skeleton inheritance (surfaced in Task 7).
- **The interference cache's exact-equality key deserves a one-line comment** — `interference_for` matches
  `f == freq_mhz && e == injected` exactly; a NaN frequency or emitter would recompute every field
  (deterministic, unreachable today) (surfaced in Task 6).

## Rulings worth remembering
- Fault randomness comes from a dedicated `elrs-faults` RNG stream: fault-active runs differ
  from fault-free ones only in the loss decisions; the model's own stream never shifts.
- The failsafe decode reads arming-disable flags (RX_FAILSAFE 1<<2, FAILSAFE 1<<1), not the
  BOXFAILSAFE mode bit — a real RX-loss failsafe never sets the pilot-switch path.
- `fault.battery.weak_cell` carries cell index + 1 (0 = none) so the synthesis can find it.
- The video interference cache is keyed on (frequency, injected emitter), not frequency alone — Task 6's
  sanctioned model change: the m3b cache swallowed runtime injections; the vehicle-level emitter test exposed
  it, and the key change keeps no-fault paths bit-identical.
