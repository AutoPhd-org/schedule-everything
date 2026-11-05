---
sidebar_position: 1
---

# Configuration Overview

Schedule Management uses **TOML** (Tom's Obvious, Minimal Language) for all configuration. This design choice ensures that your schedule is human-readable, easy to edit, and version-control friendly.

## Directory Structure

The installer puts your config root in `~/.schedule_management/config/`. Set
`REMINDER_CONFIG_DIR` to use another location; `rmd check` prints the config
root in use.

```text
config/
├── .active_config         # Plain-text id of the active config set
├── synced_schedule.toml   # Today's plan, saved by `rmd sync --apply`
├── tasks/
│   ├── tasks.json         # Auto-generated task database (do not edit manually)
│   ├── tasks.log          # Task history log
│   ├── record.json        # Habit completion records
│   └── procrastinate.json # Deferred urgent tasks with first defer date
├── user_config_0/
│   ├── settings.toml
│   ├── odd_weeks.toml
│   ├── even_weeks.toml
│   ├── habits.toml
│   ├── profile.md         # Your context, written and read by the build-schedule skill
│   └── ddl.json
└── user_config_1/         # Optional later version created for a schedule change
    └── ...
```

Schedule files live in versioned `user_config_n` directories. The
`build-schedule` skill in your AI agent writes changes to a new version and
activates it with `rmd switch`, so earlier versions remain for rollback. The
`.active_config` marker selects which version is live. Shared task data and
today's plan stay at the config-root level. Run `rmd check` to validate the
active directory, or `rmd check --dir DIR` for another one.

## The Core Files

### 1. Settings (`settings.toml`)
Think of this as your **dictionary**. You define what a "pomodoro" is, what sound to play, and what "bedtime" means. You don't schedule *when* things happen here, only *what* they are.

### 2. Schedules (`odd_weeks.toml` / `even_weeks.toml`)
Think of these as your **calendar**. You map specific times to the definitions you created in `settings.toml`.
*   **Odd Weeks**: Active during ISO weeks 1, 3, 5, etc.
*   **Even Weeks**: Active during ISO weeks 2, 4, 6, etc.

See [how today's schedule is built](weekly-schedules.md#how-todays-schedule-is-built)
from the week file, `[common]`, and today's plan.

### 3. Tasks (`tasks.json`)
A simple JSON store for your todo list. Managed through the `rmd` CLI and the browser workspace.

### 4. Versioned Config Sets (`user_config_n`)
Schedule changes made by the `build-schedule` skill create a new config
version instead of overwriting the previous one. Use `rmd switch <id>` to move
between them.

### 5. Today's Plan (`synced_schedule.toml`)
An optional TOML overlay saved by `rmd sync --apply`, usually from the
`plan-day` skill in your AI agent. It stores the task titles for today's
pomodoro/potato blocks without rewriting your base `odd_weeks.toml` or
`even_weeks.toml` templates. Reminders and `rmd status` pick it up
automatically; `rmd sync --clear` removes it.

## Syntax Reference

### Defining an Event
The basic syntax is `"HH:MM" = "value"`.

| Type               | Syntax                                    | Example                                       |
| :----------------- | :---------------------------------------- | :-------------------------------------------- |
| **Reference**      | `"HH:MM" = "key"`                         | `"09:00" = "pomodoro"`                        |
| **Direct Message** | `"HH:MM" = "text"`                        | `"12:00" = "Lunch time! 🥗"`                   |
| **Custom Title**   | `"HH:MM" = { block="key", title="text" }` | `"14:00" = { block="meeting", title="Sync" }` |

`rmd sync --apply` saves today's plan in the same custom-title form, for example:

```toml
[schedule]
"09:00" = { block = "pomodoro", title = "Finish release notes" }
```

### Sections
*   `[monday]` through `[sunday]`: Events specific to that day.
*   `[common]`: Events that occur on *every* day of the week (unless overridden).

## Validation & Safety

The system includes built-in checks to prevent common scheduling errors:
*   **Syntax Check**: Ensures valid TOML format.
*   **Reference Check**: Verifies that every scheduled "pomodoro" is actually defined in `settings.toml`.
*   **Overlap Detection**: Warns if you schedule a 60-minute meeting at 09:00 and another event at 09:30.

## Next Steps

*   **[Settings Reference](settings.md)**: Detailed guide to `settings.toml`.
*   **[Weekly Schedules](weekly-schedules.md)**: How to structure your week.
*   **[Templates](templates.md)**: Copy-pasteable examples.
