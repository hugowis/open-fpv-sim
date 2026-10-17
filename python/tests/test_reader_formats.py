"""ofs.reader on MCAP files written in the ways other tools write them: the fast path (zstd or uncompressed chunks that
carry message indexes) and the fallback (lz4 chunks, no message indexes) must read the same signals identically."""
import json

import pytest

from ofs import reader

mcap_writer = pytest.importorskip("mcap.writer")
Writer, CompressionType, IndexType = mcap_writer.Writer, mcap_writer.CompressionType, mcap_writer.IndexType

# (name, compression, index types, the fast path should run)
VARIANTS = [
    ("zstd with message indexes", CompressionType.ZSTD, IndexType.ALL, True),
    ("uncompressed with message indexes", CompressionType.NONE, IndexType.ALL, True),
    ("lz4 with message indexes", CompressionType.LZ4, IndexType.ALL, False),
    ("zstd without message indexes", CompressionType.ZSTD, IndexType.CHUNK, False),
]


def _write(path, compression, index_types):
    """Three signals over ~1000 messages in about 30 chunks: `a` and `b` every millisecond, `c` every seventh, and a
    second `a` message at the last instant (two records of one signal with the same time)."""
    with open(path, "wb") as f:
        w = Writer(f, chunk_size=2000, compression=compression, index_types=index_types)
        w.start()
        schema = w.register_schema(name="record", encoding="jsonschema", data=b"{}")
        a, b, c = (w.register_channel(topic, "json", schema) for topic in ("a", "b", "c"))
        t = 0
        for i in range(400):
            t = 1_000_000 * (i + 1)
            w.add_message(a, t, json.dumps({"v": float(i)}).encode(), t)
            w.add_message(b, t, json.dumps({"x": i, "y": -i, "z": 0.5}).encode(), t)
            if i % 7 == 0:
                w.add_message(c, t, json.dumps({"v": i * 2.0}).encode(), t)
        w.add_message(a, t, json.dumps({"v": -1.0}).encode(), t)
        w.finish()


@pytest.mark.parametrize("name,compression,index_types,fast", VARIANTS, ids=[v[0] for v in VARIANTS])
def test_every_variant_reads_like_the_mcap_reader(tmp_path, name, compression, index_types, fast):
    path = tmp_path / "file.mcap"
    _write(path, compression, index_types)
    for patterns in (None, ["b"], ["a", "c"], ["c", "a"], "b"):
        got = reader.read(path, patterns)
        assert got == reader._read_with_mcap(path, patterns), f"{name}: {patterns}"
        assert reader._used_indexed_path is fast, f"{name}: the fast path {'should' if fast else 'must not'} run"
    columns, times, rows = reader.read(path, ["b"])
    assert columns == ["b.x", "b.y", "b.z"] and len(times) == 400 and rows[3] == {"b.x": 3, "b.y": -3, "b.z": 0.5}
    columns, times, rows = reader.read(path, ["a", "c"])
    assert rows[-1]["a"] == -1.0, "of two records of one signal at one time, the later one stays"
    assert sum("c" in row for row in rows) == 58, "c is recorded every seventh instant"


def test_two_channels_sharing_a_topic_are_read_the_slow_way(tmp_path):
    """The iterator merges the records of both channels by time and file order; per-channel indexes cannot, so the
    record that wins at a shared instant would differ."""
    path = tmp_path / "shared.mcap"
    with open(path, "wb") as f:
        w = Writer(f, chunk_size=2000, compression=CompressionType.ZSTD, index_types=IndexType.ALL)
        w.start()
        schema = w.register_schema(name="record", encoding="jsonschema", data=b"{}")
        first, second = (w.register_channel("a", "json", schema) for _ in range(2))
        for i in range(100):
            t = 1_000_000 * (i // 2 + 1)
            w.add_message(first if i % 2 else second, t, json.dumps({"v": float(i)}).encode(), t)
        w.finish()
    assert reader.read(path) == reader._read_with_mcap(path)
    assert reader._used_indexed_path is False


def test_a_file_without_a_statistics_record_is_read_the_slow_way(tmp_path):
    """Without statistics nothing proves the message indexes account for every record, so they are not trusted."""
    path = tmp_path / "no-statistics.mcap"
    with open(path, "wb") as f:
        w = Writer(f, chunk_size=2000, compression=CompressionType.ZSTD, index_types=IndexType.ALL, use_statistics=False)
        w.start()
        schema = w.register_schema(name="record", encoding="jsonschema", data=b"{}")
        a = w.register_channel("a", "json", schema)
        for i in range(100):
            w.add_message(a, 1_000_000 * (i + 1), json.dumps({"v": float(i)}).encode(), 1_000_000 * (i + 1))
        w.finish()
    assert reader.read(path) == reader._read_with_mcap(path)
    assert reader._used_indexed_path is False


def test_a_pattern_matching_nothing_still_names_the_near_misses(tmp_path):
    path = tmp_path / "file.mcap"
    _write(path, CompressionType.ZSTD, IndexType.ALL)
    with pytest.raises(KeyError) as e:
        reader.read(path, ["nothing.like.this"])
    assert "matches no recorded signal" in str(e.value)
