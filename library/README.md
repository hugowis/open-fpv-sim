Part data files are CC-BY-SA-4.0 (the parent design §10).

The library is one TOML per part, sorted into kind directories — `frames/`, `motors/`, `props/`, `escs/`,
`batteries/`, `vtx/`. Quads reference parts by id, and the id is the file stem
(`motors/opendrone-openmotor-2207.toml` is id `opendrone-openmotor-2207`). The loader resolves a quad's
references against `<quad dir>/../library`, overridable with `OFS_LIBRARY` (a `;`-separated list on Windows, `:`
elsewhere; the first root that has a part id wins). Parts do not reference other parts.

To add a part, drop a TOML into the kind directory, named for its id. Give it `schema_version = 1`, a
`name` and its one data section (`[motor]`, `[frame]`, …), and give every parameter a `[provenance]` entry
— a `source` (`datasheet`, `measured` or `estimated`), an optional link and a note. A part whose provenance
is incomplete is refused at load; the shipped values and their sources are listed in
`docs/research/component-library.md`. Then reference the id from a quad (`frame = "…"`,
`motors = ["…"]`, `[battery] part = "…"`, `[vtx] part = "…"`) — the quad carries the wiring (UART, battery
rate, default VTX power), the part carries the hardware.
