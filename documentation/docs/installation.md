---
sidebar_position: 2
title: Installation
description: Install the rmd CLI and background reminder service on macOS or Linux, then add the schedule-everything plugin to your AI agent.
---

# Installation

This guide installs the `rmd` CLI, your configuration, and the background
reminder service, then adds the plugin to your AI agent.

> **Compatibility**: macOS (via `launchd`) and Linux (via a `systemd` user
> service). Windows support is on the roadmap.

## Prerequisites

*   **Python 3.12+**: [Download Python](https://www.python.org/downloads/)
*   **Git**: [Download Git](https://git-scm.com/downloads)
*   **Terminal**: any standard terminal (Terminal.app, iTerm2, GNOME Terminal, …)
*   **An AI coding agent** (optional): Claude Code, OpenAI Codex, or Google Antigravity, to build schedules and plan days with the Schedule Everything plugin
*   **Node.js**: only needed to rebuild the browser assets from source; the Python package includes them

## Method 1: Automated installer (recommended)

The `install.sh` script installs dependencies, scaffolds your configuration,
and registers the background service in one go. It installs `rmd` as the
primary CLI and keeps `reminder` as a compatibility alias.

```bash
curl -fsSL https://raw.githubusercontent.com/AutoPhd-org/schedule-everything/main/install.sh | bash
```

Or, from a clone of the repository:

```bash
git clone https://github.com/AutoPhd-org/schedule-everything.git
cd schedule-everything
./install.sh
```

The script:

*   downloads the repository to a temporary directory when run through `curl`;
*   installs the app and a Python environment (managed by `uv`) in `~/SCHEDULE_MANAGEMENT`, including the `rmd` wrapper, logs, and a copy of the agent plugin;
*   creates your config root in `~/.schedule_management/config` with a first `user_config_0` version (an existing config is kept);
*   prompts for missing required config values one by one;
*   registers the background service (LaunchAgent on macOS, systemd user service on Linux).

Then start the service:

-   **macOS** (launchd):
    ```bash
    launchctl load ~/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist
    ```
-   **Linux** (systemd user service):
    ```bash
    systemctl --user start schedule-management.service
    systemctl --user enable schedule-management.service  # auto-start at login
    ```
    Enable lingering so the user service runs without an active login
    session: `loginctl enable-linger "$USER"`.

## Method 2: Manual installation

For advanced users or those integrating into existing environments. This
method does not register the background service; use the installer for that.

1.  **Clone the repository and install the package** in a virtual environment:
    ```bash
    git clone https://github.com/AutoPhd-org/schedule-everything.git
    cd schedule-everything
    uv venv .venv
    source .venv/bin/activate
    uv pip install -e .
    ```

2.  **Create the configuration** from the templates in `config/user_config_0/`:
    ```bash
    mkdir -p ~/.schedule_management/config/user_config_0
    cp config/user_config_0/settings_template.toml ~/.schedule_management/config/user_config_0/settings.toml
    cp config/user_config_0/week_schedule_template.toml ~/.schedule_management/config/user_config_0/odd_weeks.toml
    cp config/user_config_0/week_schedule_template.toml ~/.schedule_management/config/user_config_0/even_weeks.toml
    cp config/user_config_0/habits_template.toml ~/.schedule_management/config/user_config_0/habits.toml
    ```

3.  **Point the CLI at it** (optional when you use the default location above).
    Add to your shell profile (`~/.zshrc`, `~/.bashrc`, …):
    ```bash
    export REMINDER_CONFIG_DIR="$HOME/.schedule_management/config"
    ```

4.  **Validate**:
    ```bash
    rmd check
    ```

## Method 3: PyPI

This installs the library and the `rmd` command, without configuration or the
background service:

```bash
pip install schedule-management
```

Then follow steps 2–4 of the manual installation.

## Add the AI agent plugin (optional)

Building a schedule and planning a day happen in your own coding agent. The
plugin lives in the [`plugins/schedule-everything`](https://github.com/AutoPhd-org/schedule-everything/tree/main/plugins/schedule-everything)
directory of the repository and is installed into your agent, not into
Schedule Everything:

<Figure
  src="img/diagrams/how-it-works.svg"
  alt="You ask your AI coding agent; the schedule-everything plugin installed in the agent runs rmd commands; rmd writes local files that drive reminders and the browser workspace"
/>

<AgentInstall />

The installer also copies the plugin to `~/SCHEDULE_MANAGEMENT/plugins/schedule-everything`,
so Antigravity users can run `agy plugin install ~/SCHEDULE_MANAGEMENT/plugins/schedule-everything`
instead of cloning. See [AI agent plugins](ai-agents.md) for the skills and
example prompts. The agent sends what it reads to its own model provider;
Schedule Everything itself makes no AI calls.

## Browser workspace

Launch the local browser interface:

```bash
rmd web
```

It uses the same local files as the CLI: tasks, deadlines, habits, today's
plan, the agent plugin install commands, configuration editing, mode and
schedule switching, reminder controls, and PDF downloads. Launching again
reuses an open workspace tab. The server listens only on `127.0.0.1` and keeps
running when the terminal closes. The installed package includes the browser
assets; no native desktop app or widget is shipped. See
[Browser workspace](browser.md).

## Verifying the installation

1.  **Check the CLI**:
    ```bash
    rmd --help
    ```
    *Expected: a list of available commands.*

2.  **Check the service**:
    -   **macOS**:
        ```bash
        launchctl list | grep schedule
        ```
        *Expected: a process ID and status code (usually 0).*
    -   **Linux**:
        ```bash
        systemctl --user status schedule-management.service
        ```
        *Expected: `active (running)` with recent log lines.*

3.  **Validate the configuration**:
    ```bash
    rmd check
    ```
    *Expected: the config root, the checked directory and its files, then "Configuration is valid."*

4.  **View the schedule**:
    ```bash
    rmd status
    ```
    *Expected: what is active now and the next event.*

## Uninstallation

1.  **Unload the service**:
    -   **macOS**:
        ```bash
        launchctl unload ~/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist
        rm ~/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist
        ```
    -   **Linux**:
        ```bash
        systemctl --user stop schedule-management.service
        systemctl --user disable schedule-management.service
        ```

2.  **Remove the app**:
    ```bash
    rm -rf "$HOME/SCHEDULE_MANAGEMENT"
    ```

3.  **Remove your configuration and history** (optional; this deletes your
    schedules, tasks, and habit records):
    ```bash
    rm -rf "$HOME/.schedule_management"
    ```

4.  **Remove the plugin** from your agent with that agent's plugin commands.

## Troubleshooting

*   **"command not found: rmd"**: add `~/SCHEDULE_MANAGEMENT` to your `PATH`
    (the installer prints the exact line) and reload your shell profile.
*   **Service not starting**: check the logs in `~/SCHEDULE_MANAGEMENT/logs/`,
    or run `rmd update` to reload the service.
*   **Configuration errors**: run `rmd check` for missing files, TOML syntax
    errors, unknown blocks, and likely label typos.
