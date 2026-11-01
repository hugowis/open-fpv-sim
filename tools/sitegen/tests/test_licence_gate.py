import json
import subprocess
from pathlib import Path

import pytest

from sitegen.osm import FETCH_NOTE

ROOT = Path(__file__).resolve().parents[3]


def test_no_fetched_snapshot_is_committed_before_the_licence_is_confirmed():
    try:
        files = subprocess.run(["git", "ls-files", "*.osm.json"], cwd=ROOT, capture_output=True, text=True, check=True).stdout.split()
    except (OSError, subprocess.CalledProcessError):
        pytest.skip("git is unavailable or this is not a checkout")
    fetched = [f for f in files if json.loads((ROOT / f).read_text(encoding="utf-8")).get("source_note", "").startswith(FETCH_NOTE)]
    assert not fetched, f"{fetched} came from `fetch` (real OSM data); see docs/research/sitegen-licensing.md. The owner deletes this test at sign-off."
