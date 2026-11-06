"""Make the qutebrowser adapter importable when pytest runs from the repo."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "plugins" / "qutebrowser"))
