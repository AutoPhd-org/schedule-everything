# Porting notes: `rmd` and the original Python implementation

`rmd` is a port of the original Python implementation of Schedule Everything,
which was removed from the repository once the port was complete (its last
version is in commit `a0c8d41`). The port kept behavioural parity: same CLI
commands, flags, exit codes, JSON output, data files and browser API, so the
AI-agent plugin skills, the TypeScript web UI and existing users' data kept
working unchanged. [`tests/golden`](../../tests/golden) holds the Python
implementation's recorded output as a frozen spec that `cargo test` replays.

## Contracts that must not change

- `rmd` subcommands, flags and exit codes (see `src/cli.rs`).
- JSON printed by `rmd check --json` and `rmd sync --json`, and the result of
  `rmd sync --apply` (the plugin skills in `plugins/` depend on them).
- Every browser API command in `src/web/api.rs` (`{"command", "payload"}` →
  `{"ok": true, "data"}` / `{"ok": false, "error": {"code", "message", "details"}}`),
  which the TypeScript UI in `web/src` calls.
- On-disk files under the config root: `user_config_n/*.toml`, `ddl.json`,
  `tasks/tasks.json`, `tasks/tasks.log`, `tasks/record.json`,
  `tasks/procrastinate.json`, `tasks/mode.txt`, `synced_schedule.toml`.
  JSON files keep Python's `json.dump(indent=2, ensure_ascii=False)` layout.

## Conventions

- **Time**: never call `chrono::Local::now()` / `Utc::now()` directly. Use
  `crate::clock::{now, today, now_local, now_utc, unix_time}`; they honour
  `RMD_NOW` (frozen local time, e.g. `2026-11-09T10:30:00`) for tests and the
  golden cases. Durations measured for sleeping/timeouts can use `Instant`.
- **Paths**: `crate::paths::runtime()` gives the active `RuntimePaths`
  (re-resolved on every call).
- **Records are JSON values**: tasks, deadlines, log entries and habit records
  stay `serde_json::Value` with insertion order preserved (`preserve_order`),
  exactly like the Python dicts, so unknown fields survive round trips. TOML
  documents are converted to `py::Dict` (ordered JSON objects) by
  `crate::config::load_toml_file`.
- **Python semantics**: `crate::py` has truthiness (`truthy`), `py_str`,
  `py_repr`, `py_int`, `py_float`, `float_repr`, `json.dumps` layouts
  (`dumps`, `dumps_pretty`, `write_json_pretty`), `datetime.fromisoformat`
  (`parse_iso_datetime`, `parse_iso_date`), `strptime`, `isoformat_*`,
  `weekday_name`. Use them wherever output has to keep the Python behaviour.
- **Text**: user-facing strings go through `crate::i18n::t` / `tf`; the
  English text is the key into the Chinese table `src/i18n_table.rs`. That
  table is maintained by hand: add new strings to it directly, sorted by the
  UTF-8 bytes of the English text (a unit test checks the order).
  `tf("Task: {name}", &[("name", &value)])` fills `{name}` placeholders.
- **Errors**: keep user-facing messages and exit codes stable (the golden
  cases and the plugin skills depend on them). Internally use
  `anyhow`/`thiserror`.
- **Tests**: unit tests live in `#[cfg(test)]` modules. Anything touching the
  environment or the config root must use `crate::test_support::TestEnv` (it
  serialises tests and restores env vars). `TestEnv::with_fixture()` copies
  `tests/fixtures/config`. `cargo test -p rmd --test golden` replays the golden
  cases.
- Run `cargo fmt`, `cargo clippy --all-targets` (no warnings) and
  `cargo test` before committing.

## Internal subcommands

- `rmd __api [JSON]` → `web::api::cli_main`.
- `rmd __web-server --root R --port P` → `web::server::run`.
- `rmd daemon` → `reminder::run_service` (what the LaunchAgent / systemd unit runs).
- `rmd __setup-config [ARGS...]` → `install_config_wizard::run`.
