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

Schedule Everything is a local-first, AI-assisted scheduling CLI for building a
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

### 5. Optional macOS Desktop Tools (Daily Command Center & Übersicht Widget)

Schedule Everything provides desktop integrations for macOS users:

- **Tauri 2 Daily Command Center**: A desktop app that uses the same local config, tasks, deadlines, habits, and sync overlay files as the CLI, presenting them as a daily command center with quick task/deadline entry, habit checks, and `rmd sync` proposal review.

  You can download a pre-built DMG from GitHub Releases or build it yourself:
  ```bash
  npm install
  npm run tauri:dev
  npm run tauri:build
  ```
  `npm run tauri:build` packages the Python JSON bridge as a sidecar and writes the macOS bundles under `src-tauri/target/release/bundle/`.

- **Übersicht Desktop Widget**: Displays your task list as a live, interactive desktop wallpaper widget (`rmd-tasks.widget`).
  - **Interactive Task Deletion**: Tasks displayed on the desktop widget can be deleted directly via GUI actions. Each task row includes an inline delete button `[✕]`.
    - **First click**: The button transforms to `[Confirm?]` with a red highlight for a 3-second safety window.
    - **Second click**: Confirms deletion, executes `rmd rm <id>` in the background, and immediately re-renders the widget.
    - **Auto-reset**: If `[Confirm?]` is not clicked within 3 seconds, it safely resets to `[✕]`.

> [!TIP]
> **macOS "App is damaged" Workaround**: Since pre-built DMGs are unsigned, macOS Gatekeeper may show a warning saying the app is damaged. You can easily fix this by dragging the app to `/Applications` and running:
> ```bash
> xattr -r -d com.apple.quarantine "/Applications/Schedule Everything.app"
> ```

## Core Commands

| Command | What it does |
| --- | --- |
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
| `rmd settings` | Interactive TUI for editing `settings.toml` and the optional pi model override (arrow keys, Enter/Space to edit, `s` to save, `q` to quit) |

`rmd setup` previews proposed file changes before applying a schedule modification. Press `Ctrl+C` to cancel; the settings editor asks whether to save unsaved changes before quitting.

## Manual Setup and Docs

The low-level manual configuration flow has been moved to the docs.

- [Introduction](https://PhDeasy-org.github.io/schedule-everything/docs/intro)
- [Quick Start](https://PhDeasy-org.github.io/schedule-everything/docs/quick-start)
- [Installation](https://PhDeasy-org.github.io/schedule-everything/docs/installation)
- [Configuration Overview](https://PhDeasy-org.github.io/schedule-everything/docs/configuration/overview)
- [CLI Overview](https://PhDeasy-org.github.io/schedule-everything/docs/cli/overview)

## License

Distributed under the **MIT License**. See [LICENSE](LICENSE).
