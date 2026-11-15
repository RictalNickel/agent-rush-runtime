#!/usr/bin/env python3
"""Installation-relative isolated entrypoint; never import workspace modules."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from boxoro_finance.cli import main

if __name__ == "__main__":
    raise SystemExit(main())
