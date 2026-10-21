# The repro bundle

When a session fails — a Betaflight SITL crash or hang, a non-finite value in a signal, an internal panic — the
server writes a **repro bundle**: one folder with everything needed to run the session again. Attach it to an issue (zip
the folder); a maintainer replays it and sees the same failure. Simulated failures (a dead motor, a lost radio link)
are drone behaviour, not errors, and write nothing. A `Load` that fails (a bad quad file) writes nothing either: the
error already says what to fix.

## Where it is

`<logs dir>/<quad file stem>-<n>.repro/`, in the same logs directory as recordings and Blackbox logs (`sim.logs_dir`;
`.ofs-data/logs` by default). The `sim_error` event names it:

    non-finite value in signal 'imu.accel_frd_mps2' (repro bundle: <logs dir>/opendrone-5f-freestyle-1.repro)

From Python, `ofs.repro.find_bundles(sim.logs_dir)` lists the bundles, oldest first (a folder with no `bundle.toml`, which is written last, is still being written and is
skipped). If the bundle cannot be written
(a read-only or full disk), the event says `(repro bundle failed: <why>)` and the original error is unchanged.

## What is in it

| File | What |
|---|---|
| `bundle.toml` | Format version, the error (`reason`, `failure_kind`), the tick and time it happened at, the seed, mode (`lockstep` or `realtime`), `base_hz`, whether the flight controller was the open-loop stand-in (`open_loop_fc`) and whether that had the built-in stabilizer (`stabilize`; a bundle without the key was never stabilized), the quad, world and tail window, the bus digest at the failure, the journal size, and `exact` (below). |
| `quads/` | The quad file and its Betaflight diff, as the session read them at `Load` (editing the files afterwards changes nothing in the bundle). A diff given as an absolute path or with `..` is stored by its file name and the bundled quad says so. |
| `library/` | The merged component library the session used, as read at `Load`: every relative path from the first `OFS_LIBRARY` root that has it, else the library beside the quads. |
| `worlds/` | The world file, when the session had one. |
| `journal.txt`, `inputs-<n>.csv` | Everything that steered the run, by tick: stick changes, transmitter on/off, faults (immediate and scheduled), `clear_faults`, and each `play_inputs` file. |
| `tail.mcap` | The last 10 simulated seconds of the default channel table, from a recorder every session runs in memory (about 15 MB for the shipped quad). Opens in Foxglove or `ofs.reader`. |
| `firmware/` | Betaflight sessions: `eeprom.bin` and `betaflight.diff` as they were right after `Load`, and the end of `sitl.log`. |
| `README.txt` | How to replay it. |

## Replaying

    ofs-sim --replay path/to/quad-1.repro

or

    import ofs
    result = ofs.repro.replay_bundle("path/to/quad-1.repro")
    print(result.reproduced, result.exact, result.failure)

The bundle merges the roots resolved at `Load` into `library/` first-match-wins; a root added to `OFS_LIBRARY` after
`Load` is not in the bundle. The replay builds the bundle's own quad and library (the replayer's `OFS_LIBRARY` is
ignored), starts Betaflight from
the bundle's EEPROM if it was a Betaflight session, applies the journal tick for tick in lockstep, and stops where the
original failed. **Reproduced** means the replay failed with the same error at the same tick *and* ended on the same
bus digest. `ofs-sim --replay` prints `key=value` lines (`reproduced`, `exact`, `reached_tick`, `expected_tick`,
`digest`, `expected_digest`, `failure`, `expected_failure`) and exits 0 when reproduced, 1 when not, 2 when the folder
cannot be run.

**Exact or approximate.** `exact = true` for a lockstep session whose journal is complete: then a replay that does not
reproduce means the failure is not deterministic (the usual case is a Betaflight crash that does not recur). It is
`false` for a real-time session (Betaflight runs on its own clock there, so the same calls land at slightly different
points of its run) and for a session whose journal outgrew its cap of 250 000 entries (about an hour of 50 Hz stick
changes; `journal_truncated = true`). Those still replay, but a mismatch proves nothing.

## What it does not do

It does not capture the Configurator's changes to Betaflight's settings after `Load` (the EEPROM is the one from
`Load`), the pilot's goggles view, or a recording the session was making (`tail.mcap` is separate). Only the last 10
seconds of channels are kept; the journal covers the whole session.
