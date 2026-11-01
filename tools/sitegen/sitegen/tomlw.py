import json
import re


def _key(k: str) -> str:
    return k if re.fullmatch(r"[A-Za-z0-9_-]+", k) else _str(k)


def _str(s: str) -> str:
    # JSON string escapes are valid TOML basic-string escapes; only DEL needs adding.
    return json.dumps(s, ensure_ascii=False).replace("\x7f", "\\u007f")


def _value(v) -> str:
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, str):
        return _str(v)
    if isinstance(v, (int, float)):
        return repr(v)  # repr of inf and nan is valid TOML too
    if isinstance(v, (list, tuple)):
        return "[" + ", ".join(_value(x) for x in v) + "]"
    raise TypeError(f"cannot write {type(v).__name__} as a TOML value")


def _is_tables(v) -> bool:
    return isinstance(v, (list, tuple)) and bool(v) and all(isinstance(x, dict) for x in v)


def _emit(d: dict, path: str, out: list[str]) -> None:
    for k, v in d.items():
        if not isinstance(v, dict) and not _is_tables(v):
            out.append(f"{_key(k)} = {_value(v)}")
    for k, v in d.items():
        name = f"{path}.{_key(k)}" if path else _key(k)
        for t in ([v] if isinstance(v, dict) else v if _is_tables(v) else []):
            out += ["", f"[{name}]" if isinstance(v, dict) else f"[[{name}]]"]
            _emit(t, name, out)


def dumps(d: dict) -> str:
    """Deterministic TOML: scalars first, then tables and arrays of tables, each in insertion order."""
    out: list[str] = []
    _emit(d, "", out)
    return "\n".join(out).lstrip("\n") + "\n"
