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


def _changelog_problems(lines):
    """The heading walk the release checker owes the changelog: every '## ' release is a level, '## Unreleased' must
    carry at least one '### ' notes section before the next '## ', and no '### ' may appear before any '## '."""
    problems = []
    release_seen = False
    in_unreleased = False
    unreleased_has_sections = False
    for line in lines:
        if line.startswith("## Unreleased"):
            in_unreleased = True
            release_seen = True
        elif line.startswith("## "):
            if in_unreleased and not unreleased_has_sections:
                problems.append("'## Unreleased' has no '###' sections: the notes were never written under it")
            in_unreleased = False
            release_seen = True
        elif line.startswith("### "):
            if not release_seen:
                problems.append("a '###' section appears before the first '## ' heading")
            elif in_unreleased:
                unreleased_has_sections = True
    if in_unreleased and not unreleased_has_sections:
        problems.append("'## Unreleased' has no '###' sections: the notes were never written under it")
    return problems


def test_an_unfolded_unreleased_section_is_an_error():
    assert _changelog_problems(["## Unreleased", "## 0.2.0 — 2026-10-17", "### Fixed", "- x"])


def test_a_misplaced_heading_is_an_error():
    assert _changelog_problems(["### Fixed", "- x", "## 0.2.0 — 2026-10-17", "### Fixed", "- y"])


def test_the_real_changelog_headings_are_well_formed():
    lines = (REPO / "CHANGELOG.md").read_text(encoding="utf-8").splitlines()
    assert not _changelog_problems(lines), lines[:8]
