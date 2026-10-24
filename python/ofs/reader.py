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
import struct

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
    return (f"{path}: unfinished recording (it is still being recorded, so its summary is not written yet: stop it "
            "first with sim.recording.stop(), or read sim.log, which stops it; otherwise it was stopped by a write "
            "error such as a full disk, or the server was killed while recording, and the recording_stopped event "
            "names the cause)")


def _damaged(path) -> str:
    return (f"{path}: damaged recording (part of it cannot be read; a write error such as a full disk can leave the "
            "last chunk half written); the recording_stopped event names the cause")


# Test-only: whether the last `read` took the message-index path (set by `read`, never consulted by it).
_used_indexed_path = False

# MCAP records (https://mcap.dev/spec): a record is an opcode byte and a u64 length, then its body.
_RECORD_HEAD = struct.Struct("<BQ")
_MESSAGE_HEAD = struct.Struct("<BQHIQQ")  # opcode, length, channel, sequence, log_time, publish_time
_OP_MESSAGE = 0x05
_U32 = struct.Struct("<I")
_U64 = struct.Struct("<Q")
_INDEX_ENTRY = struct.Struct("<QQ")  # log_time, offset into the chunk's decompressed records
_OP_MESSAGE_INDEX = 0x07


class _IndexCorrupt(Exception):
    """The message indexes do not agree with the records they point at: read the file the slow way, which trusts
    nothing of theirs. Raised for a lying entry time, a wrong record opcode or channel, or truncated structures."""


def _row_of(record, topic, columns, rows, log_time):
    names = columns.get(topic)
    if names is None:
        names = columns[topic] = [topic] if list(record) == ["v"] else [f"{topic}.{k}" for k in record]
    row = rows.setdefault(log_time, {})
    for name, value in zip(names, record.values()):
        row[name] = value


def _finish(selected, rows, columns):
    times = sorted(rows)
    return [c for t in selected for c in columns.get(t, [])], times, [rows[t] for t in times]


def _collect_with_mcap(path, reader, selected):
    """The selected signals' records through `mcap`'s message iterator (every record of the file is parsed)."""
    rows, columns = {}, {}
    try:
        for _, channel, message in reader.iter_messages(topics=selected, log_time_order=True):
            _row_of(json.loads(message.data), channel.topic, columns, rows, message.log_time)
    except Exception as e:  # mcap's and json's errors for a half-written chunk say nothing of the cause
        raise ValueError(_damaged(path)) from e
    return rows, columns


def _zstd_decompressor():
    try:
        import zstandard
    except ImportError:
        return None
    return zstandard.ZstdDecompressor()


def _can_read_by_index(summary) -> bool:
    """Every chunk is zstd or uncompressed, some carry message indexes, and the file has a statistics record to check
    them against (the server's recordings have all of that)."""
    chunks = summary.chunk_indexes
    return (
        bool(chunks)
        and summary.statistics is not None
        and all(c.compression in ("zstd", "") for c in chunks)
        and any(c.message_index_offsets for c in chunks)
    )


def _collect_by_index(f, summary, selected):
    """The selected signals' records, read through each chunk's per-channel message indexes: only the wanted records
    are cut out of the (decompressed) chunk and parsed. Returns None when the indexes do not account for every record
    of a selected signal (the caller then reads the file the slow way); an index that disagrees with the records it
    points at raises _IndexCorrupt, which the caller also answers with the slow way, so no wrong data is returned."""
    wanted = {cid: channel.topic for cid, channel in summary.channels.items() if channel.topic in set(selected)}
    if len(set(wanted.values())) != len(wanted):
        return None  # two channels on one topic: only the iterator merges their records in file order
    decompress = None
    rows, columns, seen = {}, {}, {cid: 0 for cid in wanted}
    for chunk in summary.chunk_indexes:
        entries = []  # (the entry's log_time, offset into the chunk's records, channel id), in file order per channel
        for cid, index_offset in chunk.message_index_offsets.items():
            if cid not in wanted:
                continue
            f.seek(index_offset)
            head = f.read(_RECORD_HEAD.size + 2 + _U32.size)
            if head[0] != _OP_MESSAGE_INDEX or struct.unpack_from("<H", head, _RECORD_HEAD.size)[0] != cid:
                raise _IndexCorrupt("a message-index record is not its channel's")
            (size,) = _U32.unpack_from(head, _RECORD_HEAD.size + 2)
            body = f.read(size)
            if len(body) != size:
                raise _IndexCorrupt("a message index is cut short")
            for entry_time, offset in _INDEX_ENTRY.iter_unpack(body):
                entries.append((entry_time, offset, cid))
            seen[cid] += size // _INDEX_ENTRY.size
        if not entries:
            continue
        f.seek(chunk.chunk_start_offset + _RECORD_HEAD.size + 8 + 8 + 8 + 4)  # past start, end, size and crc
        (name_size,) = _U32.unpack(f.read(_U32.size))
        f.read(name_size)
        (records_size,) = _U64.unpack(f.read(_U64.size))
        raw = f.read(records_size)
        if len(raw) != records_size:
            raise _IndexCorrupt("a chunk is cut short")
        if chunk.compression == "zstd":
            decompress = decompress or _zstd_decompressor()
            if decompress is None:
                return None
            data = decompress.decompress(raw, chunk.uncompressed_size)
        else:
            data = raw
        for entry_time, offset, cid in entries:
            if offset + _MESSAGE_HEAD.size > len(data):
                raise _IndexCorrupt("a message index entry points outside its chunk's records")
            opcode, length, channel, _, log_time, _ = _MESSAGE_HEAD.unpack_from(data, offset)
            if offset + _RECORD_HEAD.size + length > len(data):
                raise _IndexCorrupt("a message index entry's record runs past its chunk's records")
            if opcode != _OP_MESSAGE or channel != cid:
                raise _IndexCorrupt("a message index points at something that is not its message")
            if log_time != entry_time:
                raise _IndexCorrupt("a message index entry's time disagrees with its message")
            start = offset + _MESSAGE_HEAD.size
            _row_of(json.loads(data[start:offset + _RECORD_HEAD.size + length]), wanted[cid], columns, rows, log_time)
    counts = summary.statistics.channel_message_counts if summary.statistics else None
    if counts is not None and any(seen[cid] != counts.get(cid, 0) for cid in wanted):
        return None
    return rows, columns


def _read_with_mcap(path, patterns=None):
    """`read` through `mcap`'s own message iterator: the reference the fast path is tested against, and its fallback."""
    make_reader = _mcap_reader()
    with open(path, "rb") as f:
        _check_magic(path, f)
        reader = make_reader(f)
        summary = _summary(path, reader)
        selected = _select(patterns, sorted(c.topic for c in summary.channels.values()))
        rows, columns = _collect_with_mcap(path, reader, selected)
    return _finish(selected, rows, columns)


def read(path, patterns=None):
    """The selected signals as (columns, times_ns, rows): `rows[i]` maps column -> value at `times_ns[i]`. A scalar
    signal is one column named after it; a vector or quaternion is one column per component (`<name>.x`).

    A recording with message indexes in zstd or uncompressed chunks (the server's own) is read by cutting only the
    selected signals' records out of each chunk, several times faster than parsing every record; any other file is
    read through `mcap`'s message iterator. On foreign files whose log times run backwards within a topic, or whose
    JSON keys vary between records, the column names and their order can differ from the iterator path's."""
    global _used_indexed_path
    _used_indexed_path = False
    make_reader = _mcap_reader()
    with open(path, "rb") as f:
        _check_magic(path, f)
        reader = make_reader(f)
        summary = _summary(path, reader)
        selected = _select(patterns, sorted(c.topic for c in summary.channels.values()))
        collected = None
        if _can_read_by_index(summary):
            try:
                collected = _collect_by_index(f, summary, selected)
            except _IndexCorrupt:
                pass  # the indexes lie about the records they point at: the slow way reads the true ones
            except Exception as e:  # a half-written chunk: the same verdict as the slow path gives
                raise ValueError(_damaged(path)) from e
            _used_indexed_path = collected is not None
        if collected is None:
            collected = _collect_with_mcap(path, reader, selected)
    return _finish(selected, *collected)


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
