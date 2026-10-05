---
sidebar_position: 2
---

# Installation

This guide details how to install and configure Schedule Management on your system.

> **Compatibility Note**: This tool is currently optimized for **macOS** (via `launchd`) and **Linux**. Windows support is on the roadmap.

## Prerequisites

Before proceeding, ensure you have the following installed:

*   **Python 3.12+**: [Download Python](https://www.python.org/downloads/)
*   **Git**: [Download Git](https://git-scm.com/downloads)
*   **Terminal**: Any standard terminal emulator (Terminal.app, iTerm2, etc.)
*   **pi CLI**: Required for `rmd setup` and `rmd sync`
*   **Node.js and Rust**: Required only when running or building the optional Tauri desktop app from source

## Installation Methods

We provide an automated installer for convenience, but manual installation is fully supported for users who prefer granular control.

### Method 1: Automated Installer (Recommended)

The `install.sh` script handles dependency installation, configuration scaffolding, and service registration in one go.
It installs `rmd` as the primary CLI and keeps `reminder` as a compatibility alias.

1.  **Run the installer**:
    You can run the installation script directly using `curl`:
    ```bash
    curl -fsSL https://raw.githubusercontent.com/sergiudm/schedule-everything/main/install.sh | bash
    ```

    Alternatively, if you have cloned the repository locally:
    ```bash
    ./install.sh
    ```

    The script will:
    *   Automatically download or clone the repository (in temp space) if run via `curl`.
    *   Install the Python package and dependencies.
    *   Create the `~/SCHEDULE_MANAGEMENT` directory structure.
    *   Ensure required config files exist and scaffold config layout.
    *   Prompt for missing required config values one by one.
    *   Interactively prompt to install the pi CLI (if `npm` is available).
    *   Register the background service (on macOS and Linux).

2.  **Finalize Setup**:
    Follow the on-screen instructions to start the background service.
    -   **macOS** (launchd):
        ```bash
        launchctl load ~/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist
        ```
    -   **Linux** (systemd user service):
        ```bash
        systemctl --user start schedule-management.service
        systemctl --user enable schedule-management.service  # auto-start at login
        ```
        > Make sure lingering is enabled so the user service runs without an
        > active login session: `loginctl enable-linger "$USER"`.

### Method 2: Manual Installation

For advanced users or those integrating into existing environments.

1.  **Clone the repository**:
    ```bash
    git clone https://github.com/sergiudm/schedule-everything.git
    cd schedule-everything
    ```

2.  **Install the Package**:
    We recommend installing in editable mode (`-e`) to easily pull updates.
    ```bash
    uv pip install -e .
    ```
    *Tip: Consider using a virtual environment managed by `uv` or `venv`.*

3.  **Install pi CLI** if you want the AI-assisted commands:
    ```bash
    npm install -g @earendil-works/pi-coding-agent
    ```

4.  **Create Configuration Directory**:
    ```bash
    mkdir -p ~/SCHEDULE_MANAGEMENT/config/user_config_0
    ```

5.  **Initialize Config Files**:
    Copy the templates into the first versioned config set:
    ```bash
    cp config/settings_template.toml ~/SCHEDULE_MANAGEMENT/config/user_config_0/settings.toml
    cp config/week_schedule_template.toml ~/SCHEDULE_MANAGEMENT/config/user_config_0/odd_weeks.toml
    cp config/week_schedule_template.toml ~/SCHEDULE_MANAGEMENT/config/user_config_0/even_weeks.toml
    ```

6.  **Configure Shell Environment**:
    Add the following to your shell profile (`~/.zshrc`, `~/.bash_profile`, etc.) to access the `rmd` CLI:

    ```bash
    export PATH="$HOME/SCHEDULE_MANAGEMENT:$PATH"
    export REMINDER_CONFIG_DIR="$HOME/SCHEDULE_MANAGEMENT/config"
    alias rmd="$HOME/SCHEDULE_MANAGEMENT/rmd"
    ```

7.  **Apply Changes**:
    ```bash
    source ~/.zshrc  # or your specific profile file
    ```

### Method 3: PyPI Installation

*Note: This method installs the library code but requires manual configuration setup.*

```bash
pip install schedule-management
```
After installation, follow steps 3-6 from the "Manual Installation" section above.

## Optional macOS Desktop Tools

### Daily Command Center App

The source tree includes a Tauri 2 desktop app named **Schedule Everything**.
It is a daily command center for the same local files used by the CLI:
tasks, deadlines, habits, today's schedule, and accepted sync overlays.

From the repository root:

```bash
npm install
npm run tauri:dev
```

To create standalone macOS bundles:

```bash
npm run tauri:build
```

The build command first packages `schedule-gui-bridge` as a Python sidecar,
then runs the Tauri release build. Outputs are written to:

```text
src-tauri/target/release/bundle/macos/Schedule Everything.app
src-tauri/target/release/bundle/dmg/Schedule Everything_0.1.0_<arch>.dmg
```

### Übersicht Desktop Widget

Schedule Everything also provides an interactive macOS desktop widget (`rmd-tasks.widget`) powered by [Übersicht](https://tracesof.net/uebersicht/).

- **Installation**: Prompts automatically during `./install.sh` on macOS, or can be installed via `python -c "from schedule_management.desktop_widget import install_widget; install_widget()"`.
- **Interactive Deletion**: Tasks rendered on your desktop wallpaper can be deleted directly via GUI clicks with a two-step inline confirmation (`[✕]` button transforms to `[Confirm?]` on first click, executing `rmd rm <id>` and re-rendering on second click).

## Verifying Installation

Confirm that everything is working correctly.

1.  **Check CLI**:
    ```bash
    rmd --help
    ```
    *Expected output: A list of available commands.*

2.  **Check Service Status**:
    -   **macOS**:
        ```bash
        launchctl list | grep schedule
        ```
        *Expected output: A process ID and status code (usually 0).*
    -   **Linux**:
        ```bash
        systemctl --user status schedule-management.service
        ```
        *Expected output: `active (running)` with recent log lines.*

3.  **View Schedule**:
    ```bash
    rmd status
    ```
    *Expected output: A summary of upcoming events or "No upcoming events".*

## Uninstallation

To completely remove the application:

1.  **Unload the Service**:
    -   **macOS**:
        ```bash
        launchctl unload ~/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist
        ```
    -   **Linux**:
        ```bash
        systemctl --user stop schedule-management.service
        systemctl --user disable schedule-management.service
        ```

2.  **Remove Configuration & Data**:
    ```bash
    rm -rf "$HOME/SCHEDULE_MANAGEMENT"
    ```

3.  **Remove Python Package**:
    ```bash
    pip uninstall schedule-management
    ```

## Troubleshooting

*   **"Command not found: rmd"**: Ensure you have added the alias and exports to your shell profile and sourced it.
*   **Service not starting**: Check the logs (standard error/output) or try running `rmd update` to refresh the service definition.
*   **"Schedule Everything is damaged and can't be opened" on macOS**: Since pre-built DMGs from GitHub Releases are unsigned, macOS Gatekeeper might block them by marking them as "damaged". You can resolve this by running the following command in your terminal:
    ```bash
    xattr -r -d com.apple.quarantine "/Applications/Schedule Everything.app"
    ```
