"""Reading the simulator's MCAP recordings: a wide table, one row per recorded time and one column per signal
component (`imu.gyro_frd_radps.x`, `battery.voltage_v`, `body.att_q.w`, ...), indexed by `time_s`.

Needs the `log` extra (`pip install open-fpv-sim[log]`): `mcap` for every function, plus `pandas` for
`to_dataframe` and `to_parquet` (Parquet also needs `pyarrow`). Patterns follow the server's rules: `*` matches
any run of characters, dots included, and `?` exactly one character.
"""
from __future__ import annotations

import csv
import fnmatch
import json
import os
import re

_INSTALL = "install the log extra: pip install open-fpv-sim[log]"


def _mcap_reader():
    try:
        from mcap.reader import make_reader
    except ImportError as e:
        raise ImportError(f"reading recordings needs the mcap package; {_INSTALL}") from e
    return make_reader


def _pattern_re(pattern: str):
    # fnmatch's `[...]` classes are not part of the pattern language: escape them before translating.
    return re.compile(fnmatch.translate(pattern.replace("[", "[[]")))


def near_misses(pattern: str, names) -> list:
    """Up to five names sharing a literal piece of `pattern` (its dot-separated parts without wildcards): more
    shared pieces first, then names starting with the first piece, then name order (the server's ranking)."""
    parts = [p.replace("*", "").replace("?", "") for p in pattern.split(".")]
    parts = [p for p in parts if p]
    scored = [(sum(p in n for p in parts), bool(parts) and n.startswith(parts[0]), n) for n in names]
    scored = [s for s in scored if s[0] > 0]
    scored.sort(key=lambda s: (-s[0], not s[1], s[2]))
    return [n for _, _, n in scored[:5]]


def _select(patterns, topics) -> list:
    """The topics `patterns` select, in pattern order (each pattern's matches in name order, no repeats); every
    topic for None. A pattern matching none raises KeyError naming its near misses."""
    if patterns is None:
        return list(topics)
    if isinstance(patterns, str):
        patterns = [patterns]
    problems, chosen = [], []
    for p in patterns:
        rx = _pattern_re(p)
        hits = [t for t in topics if rx.fullmatch(t)]
        if not hits:
            near = near_misses(p, topics)
            problems.append(f"pattern '{p}' matches no recorded signal" + (f" (similar: {', '.join(near)})" if near else ""))
        chosen.extend(t for t in hits if t not in chosen)
    if problems:
        raise KeyError("; ".join(problems))
    return chosen


_MAGIC = b"\x89MCAP0\r\n"


def _check_magic(path, f):
    """A ValueError unless the open file starts like an MCAP file (the inputs CSV and Blackbox logs sit beside them)."""
    head = f.read(len(_MAGIC))
    f.seek(0)
    if head != _MAGIC:
        raise ValueError(f"{path}: not an MCAP file")


def _summary(path, reader):
    """The file's summary section, or a ValueError that says the file is unfinished: a recording that stopped on a
    write error (a full disk), or whose server was killed, has no footer and no summary for indexed readers to find."""
    try:
        summary = reader.get_summary()
    except Exception as e:  # mcap's own errors for a truncated file are about records, not about the cause
        raise ValueError(_unfinished(path)) from e
    if summary is None:
        raise ValueError(_unfinished(path))
    return summary


def _unfinished(path) -> str:
    return (f"{path}: unfinished recording (it was stopped by a write error such as a full disk, or the server was "
            "killed while recording); the recording_stopped event names the cause")


def _damaged(path) -> str:
    return (f"{path}: damaged recording (part of it cannot be read; a write error such as a full disk can leave the "
            "last chunk half written); the recording_stopped event names the cause")


def read(path, patterns=None):
    """The selected signals as (columns, times_ns, rows): `rows[i]` maps column -> value at `times_ns[i]`. A scalar
    signal is one column named after it; a vector or quaternion is one column per component (`<name>.x`)."""
    make_reader = _mcap_reader()
    with open(path, "rb") as f:
        _check_magic(path, f)
        reader = make_reader(f)
        summary = _summary(path, reader)
        topics = sorted(c.topic for c in summary.channels.values())
        selected = _select(patterns, topics)
        rows, columns = {}, {}
        try:
            for _, channel, message in reader.iter_messages(topics=selected, log_time_order=True):
                record = json.loads(message.data)
                names = columns.get(channel.topic)
                if names is None:
                    names = columns[channel.topic] = [channel.topic] if list(record) == ["v"] else [
                        f"{channel.topic}.{k}" for k in record]
                row = rows.setdefault(message.log_time, {})
                for name, value in zip(names, record.values()):
                    row[name] = value
        except Exception as e:  # mcap's and json's errors for a half-written chunk say nothing of the cause
            raise ValueError(_damaged(path)) from e
    times = sorted(rows)
    return [c for t in selected for c in columns.get(t, [])], times, [rows[t] for t in times]


def to_dataframe(path, patterns=None):
    """A pandas DataFrame indexed by `time_s`, one column per signal component; NaN where a slower signal has no
    record at that time."""
    try:
        import pandas as pd
    except ImportError as e:
        raise ImportError(f"to_dataframe needs pandas; {_INSTALL}") from e
    columns, times, rows = read(path, patterns)
    index = pd.Index([t / 1e9 for t in times], name="time_s")
    return pd.DataFrame.from_records(rows, index=index, columns=columns)


def to_csv(path, out, patterns=None) -> str:
    """Writes the selected signals as CSV (`time_s` first, empty cells where a signal has no record). Needs only
    `mcap`. Returns `out`."""
    columns, times, rows = read(path, patterns)
    with open(out, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["time_s", *columns])
        for t, row in zip(times, rows):
            w.writerow([repr(t / 1e9), *(repr(row[c]) if c in row else "" for c in columns)])
    return os.fspath(out)


def to_parquet(path, out, patterns=None) -> str:
    """Writes the selected signals as Parquet (`df.to_parquet`; needs pandas and pyarrow). Returns `out`."""
    to_dataframe(path, patterns).to_parquet(out)
    return os.fspath(out)


class Log:
    """One finished recording on disk (what `Sim.log` and `Sim.recording.stop()` hand back)."""

    def __init__(self, path, messages: int = 0, first_s: float = 0.0, last_s: float = 0.0):
        self.path = os.fspath(path)
        self.messages = messages
        self.first_s = first_s
        self.last_s = last_s

    def signals(self) -> list:
        """The recorded signals' names (bus names), sorted."""
        make_reader = _mcap_reader()
        with open(self.path, "rb") as f:
            _check_magic(self.path, f)
            return sorted(c.topic for c in _summary(self.path, make_reader(f)).channels.values())

    def to_dataframe(self, patterns=None):
        return to_dataframe(self.path, patterns)

    def to_csv(self, out, patterns=None) -> str:
        return to_csv(self.path, out, patterns)

    def to_parquet(self, out, patterns=None) -> str:
        return to_parquet(self.path, out, patterns)

    def __repr__(self):
        return f"Log({self.path!r}, messages={self.messages}, first_s={self.first_s}, last_s={self.last_s})"
