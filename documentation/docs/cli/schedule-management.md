---
sidebar_position: 2
---

# Schedule Management Commands

Commands for managing your schedule, viewing upcoming events, and controlling the reminder service.

## AI agent plugins

Building a weekly routine and planning a day happen in your own coding agent
through the `schedule-everything` plugin for Claude Code, OpenAI Codex, or
Google Antigravity. The plugin works through the local `rmd` CLI. The agent
sends what it reads to its own model provider; Schedule Everything itself makes
no AI calls. The full guide is [AI agent plugins](../ai-agents.md).

<Figure
  src="img/diagrams/how-it-works.svg"
  alt="You ask your AI coding agent; the schedule-everything plugin runs rmd commands such as rmd check, rmd sync --json, rmd sync --apply, rmd switch, and rmd update; rmd writes local files that drive reminders and the browser workspace"
/>

### Install

<AgentInstall />

Then ask your agent things like "Build my weekly schedule", "Plan my focus
blocks for today", or "Move my workouts to the evenings".

### build-schedule

Builds or changes your weekly routine.

- Interviews you profile-first, reading `profile.md` when it exists.
- Proposes a plain-language summary and waits for your confirmation before writing any file.
- Writes `settings.toml`, `odd_weeks.toml`, `even_weeks.toml`, `habits.toml`, and `profile.md`, validates them with [`rmd check`](#check), then reloads reminders with `rmd update`.
- For changes to an existing schedule, copies the active config to a new `user_config_<n>` version, edits the copy, validates it with `rmd check --dir`, and activates it with `rmd switch <n>`. Older versions remain for rollback.
- Applies evidence-informed defaults when you leave details open: at least 7 hours of sleep opportunity, 150–300 minutes of moderate activity per week with strength work on 2 or more days, movement breaks, demanding work in your best-focus windows, daylight early in the day, and no stimulating late-night work. These are heuristics, not medical advice; your own constraints win.

### plan-day

Fills today's focus blocks.

- Reads today's context with `rmd sync --json`.
- Assigns open tasks to today's focus blocks and confirms the plan with you.
- Saves the plan with `rmd sync --apply`. See [sync](#sync).

## switch

Activate a specific versioned config snapshot and reload the background
service.

### Syntax
```bash
rmd switch CONFIG_ID
```

### What it does
- Validates that `CONFIG_ID` exists as `user_config_<id>` under your config root.
- Updates `.active_config` so the selected snapshot becomes live.
- Restarts the installer-managed reminder service when its restart helper is available.
- Prints the valid ids when you request an invalid one.

### Examples
```bash
# Switch back to the first generated schedule
rmd switch 0

# Activate the third accepted revision
rmd switch 2
```

## update

Reload the schedule configuration files and restart the background service. If
the active config directory contains `.git`, `rmd update` runs `git pull
--rebase` first. Otherwise it skips the git step and reloads the local files
as-is.

### Syntax
```bash
rmd update
```

### What it does
- Pulls the latest config changes when your config directory is git-managed.
- Restarts the installer-managed reminder service when its restart helper is available.
- Falls back to a local-only reload flow for configs that are not git-managed, such as those written by an AI agent or by hand.

### Examples
```bash
# Basic update
rmd update
```

## status

Show upcoming events and current service status.
When today has a saved plan (see [sync](#sync)), pomodoro and potato blocks
show both the block type and the planned task title.

### Syntax
```bash
rmd status [OPTIONS]
```

### Options
| Option | Description |
|--------|-------------|
| `-v, --verbose` | Show detailed schedule for today |

### Examples
```bash
# Show current status and next events
rmd status

# Show detailed today's schedule
rmd status -v
```

## sync

Show, save, or clear today's plan for focus blocks (untitled `pomodoro` and
`potato` blocks). `rmd sync` makes no AI calls: the
[`plan-day`](#plan-day) skill in your AI agent reads `--json`, drafts the plan
with you, and saves it with `--apply`.

### Syntax
```bash
rmd sync [--json | --apply FILE | --clear]
```

### Options
| Option | Description |
|--------|-------------|
| (none) | Show today's focus blocks and the planned task for each, or a hint to plan with your AI agent |
| `--json` | Print today's context for agents as JSON |
| `--apply FILE` | Save a plan for today from a JSON file; use `-` to read standard input |
| `--clear` | Remove today's plan |

### What it does
- `--json` prints `date`, `weekday`, `parity`, `mode`, `skipped`, `slots` (each with `time`, `block`, `minutes`, and planned `title`), the day's `events`, open `tasks` (priority, type, postponement, and procrastination), upcoming `deadlines`, and the current `plan`.
- `--apply` reads a plan like the one below. Keys must be today's slot times; a partial plan is fine. Titles can be up to 120 characters. Applying replaces any earlier plan for today.
- The plan is stored as the overlay `synced_schedule.toml` in the config root. Reminders and `rmd status` pick it up automatically, without a restart.
- A plan changes only today; `odd_weeks.toml` and `even_weeks.toml` are never modified.
- Bare `rmd sync` and `--apply` are refused in P mode, and `--apply` is refused on skip days. `--json` and `--clear` work in both modes.

<Figure
  src="img/diagrams/todays-schedule.svg"
  alt="The odd or even week file is chosen by ISO week, merged with the [common] entries, and today's plan then adds task titles to untitled pomodoro and potato blocks"
  caption="A saved plan is an overlay on today's merged schedule. See how today's schedule is built in Weekly Schedules."
/>

```json
{
  "date": "2026-11-04",
  "assignments": {
    "09:00": "Draft thesis intro",
    "14:00": "Review lab PR"
  }
}
```

### Examples
```bash
# Show today's focus blocks and their planned tasks
rmd sync

# Print today's context for an AI agent
rmd sync --json

# Save a plan from a file, or from standard input
rmd sync --apply plan.json
rmd sync --apply - < plan.json

# Remove today's plan
rmd sync --clear
```

## check

Validate a config directory: the active one by default, or the one given with
`--dir`.

### Syntax
```bash
rmd check [--dir DIR] [--json]
```

### Options
| Option | Description |
|--------|-------------|
| `--dir DIR` | Config directory to check instead of the active one |
| `--json` | Print the result and config paths as JSON |

### What it does
- Requires `settings.toml`, `odd_weeks.toml`, and `even_weeks.toml`.
- Checks TOML and JSON syntax and required tables: `settings.toml` needs `[settings]`, `[time_blocks]`, and `[time_points]`; `habits.toml` needs `[habits]`; `ddl.json` must be a list of event/deadline objects.
- Checks the week files against `settings.toml`. An inline `{ block = "...", title = "..." }` whose `block` is not a `[time_blocks]` name is an error, because reminders ignore it.
- Reports warnings that do not make the directory invalid: a missing `[tasks]` or `[paths]` table (built-in defaults apply), and a plain label that is not a `[time_blocks]` or `[time_points]` name (it is shown as a plain reminder message with no duration, which is usually a typo).
- Prints the config root, the checked directory, its files, errors, and warnings. Exits with code 1 when the directory is invalid.
- `--json` prints `configRoot`, `activeId`, `activeConfigDir`, `configIds`, `nextConfigId`, `nextConfigDir`, `directory`, `files` (each with `path` and `exists`), `valid`, `errors`, and `warnings`.

### Examples
```bash
# Validate the active config directory
rmd check

# Validate a new version before activating it
rmd check --dir ~/.schedule_management/config/user_config_2
rmd switch 2

# Machine-readable result for scripts and AI agents
rmd check --json
```

## view

Generate a visual representation of your schedule as a PDF document. This command creates a multi-page PDF combining your Odd and Even week schedules and opens it in your default PDF viewer (`xdg-open` on Linux).

### Syntax
```bash
rmd view
```

### Examples
```bash
# Generate and open schedule PDF visualization
rmd view
```

## edit

Open the TOML schedule configuration files directly in your default system editor.

### Syntax
```bash
rmd edit [FILE]
```

### Options
FILE choices: `settings`, `odd`, `even`, `deadlines`, `habits` (default is `settings` if omitted).

### Examples
```bash
# Edit settings (default)
rmd edit

# Edit odd weeks schedule
rmd edit odd

# Edit deadlines
rmd edit deadlines
```

## settings

Open the interactive settings TUI editor to customize your configuration parameters with keyboard navigation.

### Syntax
```bash
rmd settings
```

### What it does

- Launches a keyboard-driven TUI screen inside the terminal.
- Allows navigating sections and configuration keys using `Up` / `Down`, with `Enter` to open a section or edit a value.
- Allows editing numeric or text values inline, toggling boolean values with `Space`, and picking choices/days from lists.
- Supports adding new custom keys under configurable sections.
- Shows one shortcut per action in the footer, with local editing actions above navigation and save/quit controls.
- Uses `Esc` to return from a section or cancel the current editor. At the section list, `Esc` stays in settings; use `q` to quit.
- Prompts when quitting with unsaved changes: `s` saves and quits, `d` discards and quits, and `Esc` keeps editing. Repeating `q` does not discard changes.
- Shows a modified marker for unsaved settings changes, including on the section list. `Ctrl+C` also opens the quit confirmation when changes are unsaved.
- Persists changes back to your active `settings.toml` configuration on save (`s`).

### Keyboard actions

| Context | Actions |
| --- | --- |
| Section list | `↑` / `↓` Move · `Enter` Open · `s` Save · `q` Quit |
| Settings section | `↑` / `↓` Move · `Enter` Edit (or `Space` Toggle for booleans) · `a` Add · `d` Delete · `Esc` Back · `s` Save · `q` Quit |
| Choice picker | `↑` / `↓` Move · `Enter` Apply · `Esc` Cancel |
| Multi-select | `↑` / `↓` Move · `Space` Toggle · `Enter` Apply · `Esc` Cancel |
| Text/number/time editor | `Enter` Apply · `Esc` Cancel · `Backspace` Delete a character |
| Multi-step weekday/day editor | `Enter` Next on the first step, Apply on the time step · `Esc` Cancel the entire edit |
| New key editor | `Enter` Add · `Esc` Cancel |
| Time list | `↑` / `↓` Move · `Space` Edit item · `a` Add · `d` Delete · `Enter` Apply list · `Esc` Cancel list |
| Unsaved changes | `s` Save and quit · `d` Discard and quit · `Esc` Keep editing |

Applying a field or list updates the in-memory configuration; use `s` from a browsing panel to write changes to disk. In a time list, cancelling an item edit returns to the list, and cancelling a new item leaves no placeholder behind.

The old `e` / `x` quit shortcuts still work in browsing panels, and `Left` / `Backspace` remain back/cancel aliases in sections, choice pickers, and time lists. The footer displays only the primary shortcuts. Time-list controls have changed: use Space to edit and Enter to apply; Esc, Left, and Backspace now cancel the list instead of applying it.

## stop

Stop the reminder-runner background service.

### Syntax
```bash
rmd stop
```

## report

Generate a weekly or monthly productivity report as a PDF document.

### Syntax
```bash
rmd report TYPE [OPTIONS]
```

### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `TYPE` | string | `weekly` for the calendar week containing the target date, or `monthly` for the calendar month containing the target date |

### Options
| Option | Description |
|--------|-------------|
| `-d, --date` | Target date in YYYY-MM-DD format (default: today) |
| `--days` | Compatibility flag for older scripts. `weekly` only accepts `7`; `monthly` does not support it. |

### Examples
```bash
# Generate the weekly report for the current week
rmd report weekly

# Generate the monthly report for the current month
rmd report monthly

# Generate the weekly report for the week containing a specific date
rmd report weekly -d 2024-02-01

# Generate the monthly report for the month containing a specific date
rmd report monthly -d 2024-02-01
```
