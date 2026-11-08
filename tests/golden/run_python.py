"""Run the original Python `rmd` (or its browser API) with the clock frozen.

Usage: python tests/golden/run_python.py --now 2026-11-09T10:30:00 -- ARGV...

`ARGV` is what follows `rmd` on the command line. `__api JSON` runs the
browser API entry point (`python -m schedule_management.web.api JSON`). The
local time zone comes from `TZ`; the Rust binary gets the same instant through
`RMD_NOW`, so both implementations see identical wall-clock and UTC times.
"""

from __future__ import annotations

import datetime as dt
import os
import sys
from pathlib import Path
from zoneinfo import ZoneInfo

import time_machine

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "src"))


def main() -> int:
    args = sys.argv[1:]
    if len(args) < 3 or args[0] != "--now" or args[2] != "--":
        print(__doc__, file=sys.stderr)
        return 64
    now, argv = args[1], args[3:]
    moment = dt.datetime.fromisoformat(now).replace(tzinfo=ZoneInfo(os.environ.get("TZ", "UTC")))
    with time_machine.travel(moment, tick=False):
        if argv and argv[0] == "__api":
            from schedule_management.web.api import main as api_main

            return api_main(argv[1:])
        from schedule_management.cli import main as cli_main

        sys.argv = ["rmd", *argv]
        return cli_main()


if __name__ == "__main__":
    sys.exit(main())
