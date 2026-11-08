# Golden parity tests

The original Python implementation is the oracle for the Rust `rmd` binary.

- `fixtures/standard/` is a config root (`user_config_0`, tasks, task log,
  deadlines, habit records, a conference cache) generated through the Python
  API at frozen times by `golden.py fixture`.
- `cases.json` lists every case: an `rmd` command line (or a browser API request
  run through `rmd __api JSON`), a frozen local time, optional setup files,
  stdin and environment, and how to compare stdout/stderr (`json`, exact
  `text`, or `ignore`).
- `expected/<case>.json` holds the Python result: exit code, stdout, stderr,
  the files the command created or changed (`files`) and removed (`deleted`),
  with temp paths replaced by `<ROOT>` and `<HOME>`.
- `known_differences.json` maps case names to the reason a difference is accepted.

Record (needs the Python package and `time-machine`):

```bash
uv run --with time-machine python tests/golden/golden.py record
```

Replay against Rust:

```bash
cargo test -p rmd --test golden
```

`GOLDEN_FILTER=sync cargo test -p rmd --test golden` runs only matching cases.
Both sides run with `TZ=America/Los_Angeles`; Rust gets the frozen time through
`RMD_NOW`, Python through `time-machine`.
