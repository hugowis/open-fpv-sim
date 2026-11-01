import sys
from pathlib import Path

# The package is run from the repo root without being installed.
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
