# Golden parity tests

These cases are a frozen spec of the original Python implementation, which
`rmd` replaced. They were recorded from the Python package before it was
removed, and `cargo test` replays them against the Rust binary.

- `fixtures/standard/` is a config root (`user_config_0`, tasks, task log,
  deadlines, habit records, a conference cache) that was generated through the
  Python API at frozen times.
- `cases.json` lists every case: an `rmd` command line (or a browser API request
  run through `rmd __api JSON`), a frozen local time, optional setup files,
  stdin and environment, and how to compare stdout/stderr (`json`, exact
  `text`, or `ignore`).
- `expected/<case>.json` holds the Python result: exit code, stdout, stderr,
  the files the command created or changed (`files`) and removed (`deleted`),
  with temp paths replaced by `<ROOT>` and `<HOME>`.
- `known_differences.json` maps case names to the reason a difference is accepted.

Replay against Rust:

```bash
cargo test -p rmd --test golden
```

`GOLDEN_FILTER=sync cargo test -p rmd --test golden` runs only matching cases.
Cases run with `TZ=America/Los_Angeles`, and `rmd` gets the frozen time
through `RMD_NOW`.

When `rmd` changes behaviour on purpose, cover the new behaviour with Rust
tests and either update the affected `expected/<case>.json` by hand or list
the case in `known_differences.json` with the reason.

## Re-recording from the Python implementation

The recorder (`golden.py`, which defines the cases and builds the fixture, and
`run_python.py`, which runs the Python CLI with a frozen clock) was removed
with the Python package. Both are still in the last commit that has them,
`a0c8d41394924b9fdeec00399726631b9fb69dc0` (the merge of the Rust rewrite).
To re-record, check that commit out in a separate worktree, record there, and
copy the results back:

```bash
git worktree add ../schedule-everything-python a0c8d41394924b9fdeec00399726631b9fb69dc0
cd ../schedule-everything-python
uv run --with time-machine python tests/golden/golden.py record    # cases.json + expected/
uv run --with time-machine python tests/golden/golden.py fixture   # fixtures/standard data files
```

Then copy `tests/golden/cases.json`, `tests/golden/expected/` and, if you
rebuilt it, `tests/golden/fixtures/` into this checkout.
