
---
sidebar_position: 2
---

# Linux Guide

Comprehensive guide for setting up and using Schedule Management on Linux.

> The CLI, scheduler loop, reminders, and reports all run natively on Linux.
> The browser workspace is available on Linux. Launch `rmd web` to manage your local data in a browser.

## Installation

### Prerequisites
- A modern Linux distribution (Ubuntu/Debian, Fedora, or Arch are detected automatically by the installer)
- Python 3.12 or higher
- `curl` and `git`
- A notification daemon and a sound theme for full popup/audio behavior (see below)

### Using the Installation Script

The easiest way to install on Linux:

```bash
# Clone the repository (with submodules for AI-assisted commands)
git clone --recurse-submodules https://github.com/PhDeasy-org/schedule-everything.git
cd schedule-everything

# Run the installation script
./install.sh
```

The script will:
1. Install distribution packages (build tools, Python headers, OpenSSL, etc.) via `apt`/`dnf`/`pacman`.
2. Install `uv` and create a virtual environment under `~/SCHEDULE_MANAGEMENT/.venv`.
3. Scaffold the versioned config layout (`user_config_0`, shared `tasks/`).
4. Install and enable a `systemd` **user** service for auto-start.
5. Generate `rmd`/`reminder`/`start|stop|restart_reminders.sh` convenience scripts.

### Manual Installation

If you prefer manual control, follow the "Manual Installation" section of the
[Installation guide](../installation.md). Make sure to set both environment
variables in your shell profile (`~/.bashrc` or `~/.zshrc`):

```bash
export PATH="$HOME/SCHEDULE_MANAGEMENT:$PATH"
export REMINDER_CONFIG_DIR="$HOME/SCHEDULE_MANAGEMENT/config"
```

## System Service Setup

### systemd User Service

The installation registers a `systemd` **user** service that runs the reminder
loop in the background.

**Service name**: `schedule-management.service`
**Unit file**: `~/.config/systemd/user/schedule-management.service`

The unit runs `reminder_macos.py` (a thin compatibility shim around the
cross-platform `ScheduleRunner`) from the `~/SCHEDULE_MANAGEMENT/.venv`
virtualenv, and writes logs to `~/SCHEDULE_MANAGEMENT/logs/`.

### Managing the Service

```bash
# Start the service
systemctl --user start schedule-management.service

# Stop the service
systemctl --user stop schedule-management.service

# Enable auto-start at login
systemctl --user enable schedule-management.service

# Disable auto-start
systemctl --user disable schedule-management.service

# Check status
systemctl --user status schedule-management.service

# Reload after editing the unit file
systemctl --user daemon-reload
```

> **Lingering**: By default, user services stop when you log out. To keep the
> reminder running across reboots and headless sessions, enable lingering for
> your user:
> ```bash
> loginctl enable-linger "$USER"
> ```

### Logs

```bash
# Live logs via journalctl
journalctl --user -u schedule-management.service -f

# Or the redirect files used by the installed service
tail -f ~/SCHEDULE_MANAGEMENT/logs/schedule_management.out
tail -f ~/SCHEDULE_MANAGEMENT/logs/schedule_management.err
```

## Linux-Specific Configuration

### Sound Files

When `[settings] sound_file` is not set, Schedule Management falls back to a
platform default. On Linux this is the freedesktop theme sound:

```toml
[settings]
# Default used when this key is absent (ships with sound-theme-freedesktop)
sound_file = "/usr/share/sounds/freedesktop/stereo/complete.oga"

# Or any audio file your playback backend supports
sound_file = "/home/yourname/Music/notification.wav"
```

Playback tries `paplay` (PulseAudio/PipeWire), then `aplay` (ALSA), then
`play` (SoX). If none can play the file (for example the freedesktop theme is
not installed), the alarm **still fires** — only the audio is skipped; the
popup dialog still appears. To get the default sound on Debian/Ubuntu:

```bash
sudo apt install sound-theme-freedesktop
```

### Notifications & Dialogs

Schedule Management uses native desktop dialogs and notifications on Linux. It
tries each of the following in order; install at least one:

| Tool | Desktop | Package (Debian/Ubuntu) |
| --- | --- | --- |
| `zenity` | GNOME / most GTK desktops | `sudo apt install zenity` |
| `kdialog` | KDE Plasma | `sudo apt install kdialog` |
| `notify-send` (libnotify) | Generic notifications | `sudo apt install libnotify-bin` |

Ensure a notification daemon is running (e.g. `mako`, `dunst`, `swaync`, or
your desktop environment's built-in daemon) so reminders are visible.

### Opening Reports

`rmd report` and `rmd view` open generated PDFs with `xdg-open`. If the file
does not open automatically, the path is still printed — check that
`xdg-utils` is installed and a default PDF viewer is configured:

```bash
xdg-mime default org.gnome.Evince.desktop application/pdf
```

## Troubleshooting Linux Issues

### Service Won't Start

```bash
# Check status and recent log lines
systemctl --user status schedule-management.service
journalctl --user -u schedule-management.service --since "10 min ago"

# Reload units after editing the unit file
systemctl --user daemon-reload
systemctl --user restart schedule-management.service
```

### No Sound on Alarms

- Confirm a playback backend is installed (`paplay`, `aplay`, or `play`).
- If using the freedesktop default, install `sound-theme-freedesktop`.
- Verify the `sound_file` path exists and is readable.

### Popups Don't Appear

- Install `zenity`, `kdialog`, or `notify-send`.
- Make sure a notification daemon is running for your session.

### Service Stops After Logout

Enable lingering so the user service persists:

```bash
loginctl enable-linger "$USER"
```
