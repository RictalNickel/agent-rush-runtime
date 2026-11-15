#!/usr/bin/env python3
"""Start the installed control-side Finance OpenBB MCP service."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from boxoro_finance.service import main

if __name__ == "__main__":
    raise SystemExit(main())
