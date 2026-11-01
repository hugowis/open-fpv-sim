import tomllib

from sitegen.tomlw import dumps

DOC = {
    "name": "Чорнобиль \"Демо\"\\ \t\x7f",
    "n": 3,
    "x": 0.1,
    "big": 1e16,
    "on": True,
    "list": [1.5, 2.0, -0.0],
    "pairs": [["a", 0.25], ["b", 0.75]],
    "pilot": {"position": [1.0, 2.0], "deep": {"k": "v"}},
    "receiver": {"diversity": False, "antennas": [{"name": "a", "gain": 2.0, "sub": {"z": 1}}, {"name": "b", "gain": 3.0}]},
    "weird key": 1,
}


def test_the_emitter_round_trips_through_tomllib():
    assert tomllib.loads(dumps(DOC)) == DOC


def test_scalars_come_before_tables_and_the_output_is_stable():
    out = dumps({"t": {"a": 1}, "s": 2})
    assert out.index("s = 2") < out.index("[t]")
    assert dumps(DOC) == dumps(DOC)
    assert "Чорнобиль" in dumps({"n": "Чорнобиль"})  # real UTF-8, not \u escapes
