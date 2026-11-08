# Porting notes: Python `schedule_management` → Rust `rmd`

The Rust crate replaces the Python package one module at a time. Each Rust
module mirrors a Python module (`commands/tasks.py` → `src/commands/tasks.rs`,
`web/services.py` → `src/web/services.rs`, ...). The goal is behavioural
parity: same CLI commands, flags, exit codes, JSON output, data files and
browser API, so the AI-agent plugin skills, the TypeScript web UI and existing
users' data keep working unchanged.

## Contracts that must not change

- `rmd` subcommands, flags and exit codes (see `src/schedule_management/cli.py`).
- JSON printed by `rmd check --json` and `rmd sync --json`, and the result of
  `rmd sync --apply` (the plugin skills in `plugins/` depend on them).
- Every browser API command in `web/api.py` (`{"command", "payload"}` →
  `{"ok": true, "data"}` / `{"ok": false, "error": {"code", "message", "details"}}`).
  The TypeScript UI in `web/src` is not being changed.
- On-disk files under the config root: `user_config_n/*.toml`, `ddl.json`,
  `tasks/tasks.json`, `tasks/tasks.log`, `tasks/record.json`,
  `tasks/procrastinate.json`, `tasks/mode.txt`, `synced_schedule.toml`.
  JSON files keep Python's `json.dump(indent=2, ensure_ascii=False)` layout.

## Conventions

- **Time**: never call `chrono::Local::now()` / `Utc::now()` directly. Use
  `crate::clock::{now, today, now_local, now_utc, unix_time}`; they honour
  `RMD_NOW` (frozen local time, e.g. `2026-11-09T10:30:00`) for tests and the
  golden recordings. Durations measured for sleeping/timeouts can use `Instant`.
- **Paths**: `crate::paths::runtime()` gives the active `RuntimePaths`
  (re-resolved on every call, like the Python `DynamicPath` constants).
- **Records are JSON values**: tasks, deadlines, log entries and habit records
  stay `serde_json::Value` with insertion order preserved (`preserve_order`),
  exactly like the Python dicts, so unknown fields survive round trips. TOML
  documents are converted to `py::Dict` (ordered JSON objects) by
  `crate::config::load_toml_file`.
- **Python semantics**: `crate::py` has truthiness (`truthy`), `py_str`,
  `py_repr`, `py_int`, `py_float`, `float_repr`, `json.dumps` layouts
  (`dumps`, `dumps_pretty`, `write_json_pretty`), `datetime.fromisoformat`
  (`parse_iso_datetime`, `parse_iso_date`), `strptime`, `isoformat_*`,
  `weekday_name`. Use them wherever the Python relied on that behaviour.
- **Text**: user-facing strings the Python wrapped in `_t(...)` go through
  `crate::i18n::t` / `tf` with the *identical* English source text, so the
  generated Chinese table (`src/i18n_table.rs`) keeps matching.
  `tf("Task: {name}", &[("name", &value)])` replaces `_t(...).format(...)`.
- **Errors**: print the same user-facing messages as the Python and return
  the same exit codes. Internally use `anyhow`/`thiserror`.
- **Tests**: port the matching Python tests from `tests/` into `#[cfg(test)]`
  modules. Anything touching the environment or the config root must use
  `crate::test_support::TestEnv` (it serialises tests and restores env vars).
  `TestEnv::with_fixture()` copies `tests/config`.
- Run `cargo fmt`, `cargo clippy --all-targets` (no warnings) and
  `cargo test` before committing.

## Internal subcommands

- `rmd __api [JSON]` → `web::api::cli_main` (same as `python -m schedule_management.web.api`).
- `rmd __web-server --root R --port P` → `web::server::run`.
- `rmd daemon` → `reminder::run_service` (what the LaunchAgent / systemd unit runs).
- `rmd __setup-config [ARGS...]` → `install_config_wizard::run`.
