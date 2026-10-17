"""Repro bundles: the folder a failed session writes (docs/repro-bundle.md), and running one again."""
from __future__ import annotations

import pathlib
import subprocess
from dataclasses import dataclass

from .client import find_binary
from .errors import ConfigError, OfsError


@dataclass(frozen=True)
class ReplayResult:
    """What running a bundle again showed.

    `reproduced`: the replay failed with the same error at the same tick and ended on the same digest.
    `exact`: the session was lockstep with an intact journal, so a mismatch means the failure is not deterministic
    (a session in real time, or one whose journal outgrew its cap, replays approximately: `exact` is False)."""

    reproduced: bool
    exact: bool
    reached_tick: int
    expected_tick: int
    digest: int
    expected_digest: int
    failure: str
    expected_failure: str


def find_bundles(logs_dir) -> list[pathlib.Path]:
    """The repro bundle folders in a logs directory (`sim.logs_dir`), oldest first."""
    root = pathlib.Path(logs_dir)
    if not root.is_dir():
        return []

    def order(p: pathlib.Path):
        stem, _, number = p.name[: -len(".repro")].rpartition("-")
        return (stem, int(number)) if number.isdigit() else (p.name, 0)

    # `bundle.toml` is written last: a folder without it is one that is still being written (or was cut short).
    whole = (p for p in root.iterdir() if p.is_dir() and p.name.endswith(".repro") and (p / "bundle.toml").is_file())
    return sorted(whole, key=order)


def replay_bundle(path, binary: str | None = None) -> ReplayResult:
    """Runs a repro bundle again (`ofs-sim --replay`) and says whether the failure came back.

    Raises `ConfigError` when the folder cannot be run (missing files, an unknown format)."""
    # ofs-sim writes UTF-8; the default text mode would read it with the locale codepage on Windows.
    done = subprocess.run(
        [find_binary(binary), "--replay", str(path)], capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    if done.returncode == 2:
        raise ConfigError(done.stderr.strip() or f"{path} cannot be replayed")
    if done.returncode not in (0, 1):
        raise OfsError(f"ofs-sim --replay exited with code {done.returncode}: {done.stderr.strip()}")
    fields = dict(line.split("=", 1) for line in done.stdout.splitlines() if "=" in line)
    return ReplayResult(
        reproduced=fields["reproduced"] == "true",
        exact=fields["exact"] == "true",
        reached_tick=int(fields["reached_tick"]),
        expected_tick=int(fields["expected_tick"]),
        digest=int(fields["digest"], 16),
        expected_digest=int(fields["expected_digest"], 16),
        failure=fields.get("failure", ""),
        expected_failure=fields.get("expected_failure", ""),
    )
