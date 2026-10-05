---
sidebar_position: 2
---

# Schedule Management Commands

Commands for managing your schedule, viewing upcoming events, and controlling the reminder service.

## setup

Launch an interactive setup wizard that builds or modifies schedules with a pi-powered assistant. pi owns credentials and model selection; `rmd setup` only carries an optional model override.

### Syntax
```bash
rmd setup
```

### What it does
- Credentials and model selection are handled by pi (its own auth store and provider env vars); authenticate with `pi` first.
- Optionally override the model via `~/.schedule_management/llm.toml` (a single `model = "..."` line; leave it unset to use pi's default).
- Uses pi (`pi`) as the setup-agent runtime.
- Checks whether a complete local schedule configuration already exists.
- Routes to either a build flow (new schedule) or a modify flow (existing schedule).
- Reads or writes `profile.md` in the same config directory as `settings.toml`.
- In build flow, iteratively refines the profile first, asks for timetable context when available, then produces a pure-text schedule summary before generating TOML.
- In modify flow, reads `profile.md` first so edits stay aligned with the user's long-term context.
- Writes the first accepted schedule to `user_config_0`, then versions later accepted edits as `user_config_n+1`.
- Switches the active config snapshot after an accepted modification.
- Before applying a modification, shows a diff of the proposed schedule files (up to 100 lines) and asks for approval. Rejecting it lets you request a revision.
- Uses evidence-informed defaults around sleep regularity, physical activity, movement breaks, and daytime light exposure when the user leaves details open.
- Only after you confirm the summary does it generate TOML configuration files.
- During build/modify turns, the pi-backed agent can attach local files/images and read local context files when needed. The CLI limits its tools to reading and searching; it applies generated files after your approval.
- Recommends `rmd view` and supports iterative adjustments.
- Press `Ctrl+C` or close terminal input to cancel the setup conversation without applying a pending schedule.

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
- Falls back to a local-only reload flow for configs created by `rmd setup` or manual edits.

### Examples
```bash
# Basic update
rmd update
```

## status

Show upcoming events and current service status.
When `rmd sync` has produced an accepted overlay for today, pomodoro and
potato blocks show both the block type and the assigned task title.

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

Generate today's pomodoro/potato task assignments from `tasks/tasks.json` with
an LLM, show a preview, and only save the overlay after you approve it.

### Syntax
```bash
rmd sync
```

### What it does
- Loads today's untitled pomodoro/potato blocks from the active schedule.
- Reads tasks from `tasks/tasks.json` and sorts them by priority.
- Uses the same pi-backed model configuration flow as `rmd setup`.
- Shows a preview table before writing `synced_schedule.toml`.
- If you reject the preview, asks for a reason and regenerates using that feedback.
- Applies the accepted overlay only to the matching day, so base odd/even templates stay unchanged.

### Examples
```bash
# Generate and review today's focus-block assignments
rmd sync
```

## view

Generate a visual representation of your schedule as a PDF document. This command creates a multi-page PDF combining your Odd and Even week schedules and immediately opens it in your default PDF viewer on macOS.

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
- Shows a modified marker for unsaved settings or model changes, including on the section list. `Ctrl+C` also opens the quit confirmation when changes are unsaved.
- Persists changes back to your active `settings.toml` configuration on save (`s`).
- Includes a **🤖 Model Settings** page (listed at the top) for the optional pi `--model` override. Edit the value as free text (e.g. a `provider/model-id` or pattern), or choose **clear** to reset it and let pi choose. The override is stored in the global `~/.schedule_management/llm.toml` (shared across config sets); credentials and model selection otherwise remain with pi. Pressing `s` saves both `settings.toml` and `llm.toml`.

### Keyboard actions

| Context | Actions |
| --- | --- |
| Section list | `↑` / `↓` Move · `Enter` Open · `s` Save · `q` Quit |
| Settings section | `↑` / `↓` Move · `Enter` Edit (or `Space` Toggle for booleans) · `a` Add · `d` Delete · `Esc` Back · `s` Save · `q` Quit |
| Model Settings | `↑` / `↓` Move · `Enter` Edit or Clear, depending on the selected row · `Esc` Back · `s` Save · `q` Quit |
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
