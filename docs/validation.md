# Validation: comparing the simulator with real hardware

Every model in the simulator is a simplification, and most of its constants are `estimated` (the component library
records which: `docs/research/component-library.md`). This is how real data replaces the estimates: a defined
format for **submissions** (measurements and the quad they were taken on), a tool that checks a submission and
compares it with the simulator, and a report. It is *tracked, not gating* (parent spec §8.5): CI prints the report of
the worked example in its job summary and never fails because the model is wrong, only because a shipped submission is
malformed.

## A submission

A folder with a `submission.toml` and one data file. Two kinds exist:

| `kind` | Data | Compared today |
|---|---|---|
| `thrust_stand` | A CSV of a motor and propeller on a stand | yes: against the simulator's own ESC, motor and propeller |
| `blackbox_step` | The decoded CSV of a Blackbox log of a step response, and the Betaflight diff of the tune | the format is checked; the step-response comparator waits for real data |

### `submission.toml`

```toml
schema_version = 1                 # this document describes 1
kind = "thrust_stand"              # or "blackbox_step"
title = "OpenMotor 2207 1950 KV, 5-inch tri-blade, 6S"
quad = "../../../quads/opendrone-5f-freestyle.toml"   # relative to this file; the quad the parts come from
motor = 0                          # thrust_stand: which of the quad's motors (its prop and ESC come with it); default 0
data = "thrust.csv"                # the data file, next to this one
diff = "tune.diff"                 # blackbox_step only: the Betaflight diff of the tune that flew

[origin]
source = "measured"                # datasheet | measured | estimated (the library's provenance vocabulary)
note = "How it was measured: the stand, the ESC, the battery, the room."
contributor = "Your name or handle"
license = "CC-BY-SA-4.0"           # required: community data comes under the component library's licence
```

The quad is the aircraft the figures describe. To submit a motor or propeller the library lacks, add the part files
(`library/motors/…`, `library/props/…`, with provenance) and a quad that uses them, and put the quad in the
submission folder under `quads/` beside a `library/` (the loader finds the library beside the quads).

### `thrust.csv` (thrust stand)

A header, then one row per steady-state point. Blank lines and lines starting with `#` are ignored; a spreadsheet's
byte-order mark and CRLF line endings are fine.

| Column | Required | Unit | Meaning |
|---|---|---|---|
| `command` | yes | 0 to 1 | The ESC command (throttle) as a fraction of full |
| `voltage_v` | yes | V | The pack (supply) voltage *under that load*, so the comparison needs no battery model |
| `rpm` | one of these three | rev/min | Rotor speed |
| `thrust_n` | | N | Thrust |
| `current_a` | | A | Current drawn from the battery (not the phase current) |

A blank cell is "not measured". Every row needs at least one measured value. Air is taken as sea-level standard
(1.225 kg/m³), as in the model; say in `origin.note` if yours was not.

```csv
command,voltage_v,rpm,thrust_n,current_a
0.50,23.8,21500,5.1,11.0
1.00,22.4,34000,12.7,52.0
```

### `flight.csv` (Blackbox step response)

The output of `blackbox_decode` on the `.bbl` (`blackbox_decode --stdout flight.bbl > flight.csv`): its header
must contain `time (us)`, `gyroADC[0]`, `gyroADC[1]`, `gyroADC[2]`, `setpoint[0]`, `setpoint[1]` and `setpoint[2]`.
Name the Betaflight `diff all` of the tune in `diff`. Today the tool checks that both are present and the header is
right, and the report says "not compared". (The column names are those of blackbox-tools; the validator prints the
header it found when one is missing, so a mismatch is easy to see.)

## Running it

    cargo run -p ofs-validate -- path/to/submission-folder
    cargo run -p ofs-validate -- path/to/submission-folder --out REPORT.md

It prints a Markdown report and exits 0 when the submission is valid, whatever the error in it; 2 when it is invalid
(one problem per line, each naming its file and line, all at once). `--max-error-pct N` exits 1 when any error is above
N percent (off by default). `--summary` appends the report to `$GITHUB_STEP_SUMMARY`, as CI does.

## Reading a report

For each row: the measured figure, the simulated one and the error, `(simulated − measured) / measured`, in percent.
The summary gives the mean and the largest absolute error per quantity. The simulator holds the motor at the row's
`command` and `voltage_v` until the rotor speed stops changing (a row that never settles is reported as such).

An error here is a finding about the **model**: usually a wrong constant (`kv`, resistance, `Ct`, `Cp` in the library
parts) or a simplification (the ESC command is mapped linearly to duty; real ESCs differ). When a submission is
`measured`, the maintainers update the part's `[provenance]` from `estimated` and cite the submission.

## The worked example

`validation/thrust-stand/example-2207-5in/` is a synthetic submission: hand-written, datasheet-style figures for the
shipped 2207 motor and 5-inch propeller, labelled `estimated`, with its generated `REPORT.md`. It is there to show the
format and the report, **not** as a measurement; the large errors it shows say nothing about the model. A test fails
when `REPORT.md` is not what the tool prints, so regenerate it with the `--out` command above after a model change.

## Contributing data

Open an issue (label `needs-data`) or a pull request with the submission folder under
`validation/<kind>/<your-name>/`, the generated `REPORT.md`, and a line in the PR on how it was measured. Run the tool
on it first.
