# Schedule Everything

<p align="center">
  <img src="assets/logo.png" alt="Schedule Everything logo" width="520">
</p>

[![Logic Tests](https://github.com/PhDeasy-org/schedule-everything/actions/workflows/logic-tests.yml/badge.svg)](https://github.com/PhDeasy-org/schedule-everything/actions/workflows/logic-tests.yml)
[![CLI Tests](https://github.com/PhDeasy-org/schedule-everything/actions/workflows/cli-tests.yml/badge.svg)](https://github.com/PhDeasy-org/schedule-everything/actions/workflows/cli-tests.yml)
[![PyPI version](https://badge.fury.io/py/schedule-management.svg)](https://pypi.org/project/schedule-management)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Documentation](https://img.shields.io/badge/docs-GitHub_Pages-blue)](https://PhDeasy-org.github.io/schedule-everything/)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/PhDeasy-org/schedule-everything)
[中文版本](README_zh.md)

Schedule Everything is a local-first, AI-assisted scheduling workspace with a browser interface and CLI for building a
durable weekly routine and then turning today's focus blocks into concrete
work.

## Workflow

<p align="center">
  <img
    src="assets/workflow.png"
    alt="Workflow for building a schedule with rmd setup, then assigning daily tasks with rmd sync"
    width="860"
  >
</p>

## Quick Start

### 1. Install

Using curl (recommended):
```bash
curl -fsSL https://raw.githubusercontent.com/PhDeasy-org/schedule-everything/main/install.sh | bash
```

Or from a local clone:
```bash
git clone https://github.com/PhDeasy-org/schedule-everything.git
cd schedule-everything
./install.sh
```

`install.sh` sets up the local environment, copies files, scaffolds the configuration, and registers background services. pi is
required for AI-assisted commands such as `rmd setup` and `rmd sync`; the installer
can install it via `npm install -g @earendil-works/pi-coding-agent`.

### 2. Build Your Schedule with One Command!

```bash
rmd setup
```

After a short conversation about your workday, constraints, and habits,
`rmd setup` builds or updates `profile.md`, shows a summary for confirmation,
and only then writes your schedule files into `user_config_0`. pi owns
credentials and model selection (authenticate with `pi` first); an optional
model override can be set in `~/.schedule_management/llm.toml`. Later accepted changes are
saved as `user_config_1`, `user_config_2`, and so on under the same config
root while `tasks/` remains shared.

Once that schedule exists, the system can remind you about scheduled blocks,
habit/deadline prompts, and give you both a live status view and a PDF
visualization of the result.

### 3. Add Tasks and Sync Today

Plans change faster than weekly templates. When that happens, add tasks and
sync the current day instead of rebuilding the whole schedule.

```bash
rmd add "Finish proposal draft" 9
rmd add "Review PR #128" 7
# Postpone daily urgent alarm until tomorrow (optional third argument: days)
rmd add "Biology homework" 9 1
rmd sync
```

Set `show_tasks_after_change = true` under `[settings]` if you want every
successful `rmd add ...` or `rmd rm ...` to immediately print the same task table as `rmd ls`.

`rmd sync` reads `tasks/tasks.json`, proposes task assignments for today's
pomodoro/potato blocks, and regenerates if you reject the preview with
feedback.

### 4. Check the Result

```bash
rmd status
rmd status -v
rmd view
rmd update
rmd switch 0
```

When a sync overlay exists for today, `rmd status` shows the block type and
the specific assigned event, for example `pomodoro: Finish proposal draft`.
`rmd update` reloads the reminder service. If your config directory is a git
repository, it pulls the latest schedule changes first; otherwise it skips the
git step and reloads your local files as-is.

### 5. Open the Browser Workspace

Launch the local browser interface:

```bash
rmd web
```

Your tasks, schedules, settings, and habit records remain in the same local
files used by the CLI. The browser includes task creation and editing,
completion/cancellation, deadlines, habit checks, AI schedule conversations,
sync proposal review, configuration editing, mode and schedule switching,
reminder controls, and PDF downloads. Tasks are grouped by type, with importance,
postponement, and procrastination details preserved. All tasks are shown.

Launching again reuses an open workspace tab. The app asks the browser to bring
it forward; browser focus permissions can prevent it from becoming the active
tab. If the tab has been closed, a new tab opens on the existing server.
The server listens only on `127.0.0.1` and continues running when the terminal
closes. CLI commands remain available.

The installed package includes the browser assets; Node.js is only needed to
rebuild them during development. No native desktop app or widget is shipped.
See [Browser workspace](documentation/docs/browser.md) for details.

## Core Commands

| Command | What it does |
| --- | --- |
| `rmd web` | Open the local browser workspace, reusing an existing tab |
| `rmd setup` | Build or modify your schedule with a profile-first AI workflow |
| `rmd sync` | Assign today's pomodoro/potato blocks to tasks with preview + approval |
| `rmd status [-v]` | Show what is happening now and today's schedule, including synced titles |
| `rmd add/ls/rm` | Manage the task list that feeds the sync flow (`rm` counts as completed) |
| `rmd cancel` / `rmd drop` | Remove a task without counting it as done (added by mistake, or giving up) |
| `rmd history [n]` | Show recent task activities; completed, cancelled, and dropped are shown distinctly (default: 5) |
| `rmd track` | Record habits |
| `rmd ddl` | Manage deadlines; entries two or more days overdue are auto-pruned |
| `rmd view` | Generate a PDF schedule visualization |
| `rmd switch <id>` | Activate a different `user_config_n` snapshot and reload the service |
| `rmd mode [j\|p]` | Switch or display the current mode (j mode allows all reminders, p mode cancels specific event alarms) |
| `rmd settings` | Interactive TUI for editing `settings.toml` and the optional pi model override (↑/↓ to move, Enter to open/apply, Esc to back/cancel, Space to toggle, `s` to save, `q` to quit) |

`rmd setup` previews proposed file changes before applying a schedule modification. Press `Ctrl+C` to cancel. The settings editor shows actions for the current panel; quitting with unsaved changes offers `s` to save and quit, `d` to discard and quit, or Esc to keep editing. In time lists, Space edits an item, Enter applies the list, and Esc cancels it.

## Manual Setup and Docs

The low-level manual configuration flow has been moved to the docs.

- [Introduction](https://PhDeasy-org.github.io/schedule-everything/docs/intro)
- [Quick Start](https://PhDeasy-org.github.io/schedule-everything/docs/quick-start)
- [Installation](https://PhDeasy-org.github.io/schedule-everything/docs/installation)
- [Configuration Overview](https://PhDeasy-org.github.io/schedule-everything/docs/configuration/overview)
- [CLI Overview](https://PhDeasy-org.github.io/schedule-everything/docs/cli/overview)

## License

Distributed under the **MIT License**. See [LICENSE](LICENSE).
