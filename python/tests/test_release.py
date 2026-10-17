"""The release's version lives in three places and its notes in one: they must not drift apart (the publish job only
checks the tag against pyproject.toml, so a stale Cargo.toml would ship a server that says another version)."""
import pytest

from conftest import REPO

tomllib = pytest.importorskip("tomllib", reason="tomllib is in the standard library from Python 3.11")


def _workspace_version():
    return tomllib.loads((REPO / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]


def _python_version():
    return tomllib.loads((REPO / "python" / "pyproject.toml").read_text(encoding="utf-8"))["project"]["version"]


def test_the_rust_and_python_versions_agree():
    assert _workspace_version() == _python_version()


def test_cargo_lock_carries_the_workspace_version():
    lock = tomllib.loads((REPO / "Cargo.lock").read_text(encoding="utf-8"))
    ours = {p["name"]: p["version"] for p in lock["package"] if "source" not in p and p["name"].startswith("ofs")}
    assert ours, "no workspace crates found in Cargo.lock"
    stale = {name: version for name, version in ours.items() if version != _workspace_version()}
    assert not stale, f"Cargo.lock lags the workspace version {_workspace_version()}: {stale} (run cargo build)"


def test_the_changelog_has_an_entry_for_the_version():
    version = _python_version()
    headings = [line for line in (REPO / "CHANGELOG.md").read_text(encoding="utf-8").splitlines() if line.startswith("## ")]
    assert any(h.startswith(f"## {version} — ") for h in headings), f"no '## {version} — <date>' heading in {headings[:3]}"
