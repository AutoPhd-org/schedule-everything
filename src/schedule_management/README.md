# `schedule_management` Package Guide

This directory contains the application code for `schedule_everything`.
If you are reading the code for the first time, start here before jumping
straight into individual modules.

## Start Here

Pick the entry point that matches the question you are trying to answer:

- CLI command wiring: [cli.py](./cli.py)
- CLI compatibility entry point used by `rmd` and `reminder`: [reminder.py](./reminder.py)
- Runtime reminder loop / background behavior: [runner.py](./runner.py)
- Configuration loading and schedule merging: [config.py](./config.py)
- Persistent task / deadline / habit storage: [data/loaders.py](./data/loaders.py)
- Config validation for `rmd check` and the browser editor: [config_check.py](./config_check.py)
- AI agent plugins (Claude Code, Codex, Antigravity) that build schedules and plan days: [../../plugins/schedule-everything/](../../plugins/schedule-everything/)

## Mental Model

The codebase is organized around four main flows:

1. CLI flow
   `pyproject.toml` exposes `rmd` and `reminder`, both pointing at `schedule_management.reminder:main`.
   The compatibility module forwards into [cli.py](./cli.py), which builds the parser and dispatches into `commands/`.

2. Runtime reminder flow
   The background process loads config and schedules, then [runner.py](./runner.py) loops forever, triggering alarms, popups, urgent-task prompts, deadline alerts, and report generation.

3. Persistence flow
   JSON and TOML files under the user config directory are read and written via [data/loaders.py](./data/loaders.py) plus a few config helpers in [config.py](./config.py) and [synced_schedule.py](./synced_schedule.py).

4. Agent planning flow
   The package makes no AI calls. Schedule building and daily planning happen in the user's coding agent through the plugin in [plugins/schedule-everything/](../../plugins/schedule-everything/). Its skills use deterministic CLI commands: `rmd check` ([config_check.py](./config_check.py)) validates config files and reports paths, and `rmd sync --json/--apply/--clear` ([commands/sync.py](./commands/sync.py)) reads today's focus slots and saves or clears the daily plan overlay.

## Package Tour

### Top-level modules

- [__init__.py](./__init__.py)
  Shared constants such as config file paths and ANSI colors.

- [cli.py](./cli.py)
  The main argparse router. If you want to add or modify a command, this is the first file to inspect along with the target file in `commands/`.

- [reminder.py](./reminder.py)
  Legacy import/entry-point compatibility layer. Useful when tests or scripts still import old names.

- [reminder_macos.py](./reminder_macos.py)
  Another compatibility module that re-exports the modularized implementation.

- [config.py](./config.py)
  Loads `settings.toml`, `odd_weeks.toml`, and `even_weeks.toml`. `ScheduleConfig` exposes typed accessors for settings, while `WeeklySchedule` builds today’s effective schedule.

- [runner.py](./runner.py)
  Core runtime engine. This is where scheduled events are checked, urgent tasks are prompted, deadlines are surfaced, and daily/weekly/monthly automation happens.

- [platform.py](./platform.py)
  OS-specific dialog and sound abstractions. If a popup or prompt behaves differently across macOS/Linux, start here.

- [popups.py](./popups.py)
  Higher-level daily summary and habit-tracking popup flows built on top of `platform.py`.

- [report.py](./report.py)
  Weekly/monthly PDF report generation and auto-report scheduling logic.

- [visualizer.py](./visualizer.py)
  Generates PDF visualizations of odd/even weekly schedules.

- [synced_schedule.py](./synced_schedule.py)
  Stores and applies the date-scoped daily plan overlay saved by `rmd sync --apply`.

- [config_check.py](./config_check.py)
  `check` command and the file validation shared with the browser configuration editor.

- [time_utils.py](./time_utils.py), [utils.py](./utils.py)
  Lower-level time and helper functions used across multiple features.

- [install_config_wizard.py](./install_config_wizard.py)
  Install-time config validation/completion helper for bootstrapping required files.

### `commands/`

This folder contains user-facing CLI handlers.

- [commands/tasks.py](./commands/tasks.py)
  `add`, `rm`, `ls`

- [commands/deadlines.py](./commands/deadlines.py)
  `ddl add`, `ddl rm`, `ddl`

- [commands/habits.py](./commands/habits.py)
  `track`

- [commands/status.py](./commands/status.py)
  `status`, `view`

- [commands/sync.py](./commands/sync.py)
  `sync`: today's focus slots as JSON, and validating, saving, or clearing the daily plan

- [commands/service.py](./commands/service.py)
  service-oriented commands such as `update`, `stop`, `report`, and config editing helpers

- [commands/completion.py](./commands/completion.py)
  shell completion script generation

## Data Boundaries

There are a few important persistence files and each has a clear owner:

- `settings.toml`, `odd_weeks.toml`, `even_weeks.toml`
  Read via [config.py](./config.py)

- `tasks/tasks.json`, `tasks/tasks.log`, `tasks/procrastinate.json`
  Managed via [data/loaders.py](./data/loaders.py) and task logic in [commands/tasks.py](./commands/tasks.py) / [runner.py](./runner.py)

- `ddl.json`
  Managed by [commands/deadlines.py](./commands/deadlines.py) and read by [runner.py](./runner.py)

- `tasks/record.json`
  Habit completion records used by [commands/habits.py](./commands/habits.py), [popups.py](./popups.py), and [report.py](./report.py)

- `synced_schedule.toml`
  Generated and consumed through [synced_schedule.py](./synced_schedule.py)

## Common Reading Paths

Use these when you need to answer a focused question quickly.

### How does a CLI command work?

1. Open [cli.py](./cli.py) and find the parser registration.
2. Follow the handler import into the corresponding file in [commands/](./commands/).
3. If the command reads or writes files, continue into [data/loaders.py](./data/loaders.py) or [config.py](./config.py).

### How does a reminder get triggered?

1. Start in [runner.py](./runner.py).
2. Follow schedule lookup through [config.py](./config.py).
3. Follow popup / sound behavior through [platform.py](./platform.py) and [popups.py](./popups.py).

### How does `rmd sync` work?

1. Start in [commands/sync.py](./commands/sync.py).
2. Follow overlay persistence and schedule application in [synced_schedule.py](./synced_schedule.py).
3. Follow current-schedule helpers in [config.py](./config.py) and [commands/status.py](./commands/status.py).
4. The planning instructions an agent follows are in [plugins/schedule-everything/skills/plan-day/SKILL.md](../../plugins/schedule-everything/skills/plan-day/SKILL.md).

### How is a schedule built or changed?

1. Read the agent skill in [plugins/schedule-everything/skills/build-schedule/](../../plugins/schedule-everything/skills/build-schedule/).
2. Validation rules live in [config_check.py](./config_check.py) (`rmd check`).
3. Versioned config sets (`user_config_<n>`) and activation (`rmd switch`) are in [config_layout.py](./config_layout.py) and [commands/service.py](./commands/service.py).

## Notes For Contributors

- This package targets Python 3.12+, so modern typing and standard-library features are preferred.
- Most user-visible behavior is covered by tests under `tests/`, especially CLI behavior and runner logic.
- Compatibility modules like [reminder.py](./reminder.py) and [reminder_macos.py](./reminder_macos.py) exist to keep older imports stable; avoid removing them casually.


The local browser interface lives in `web/` (Python services/server) and the
repository-level `web/` (TypeScript source). Launch with `rmd web`. Run
`bun run build` after frontend edits to refresh the bundled static assets.
