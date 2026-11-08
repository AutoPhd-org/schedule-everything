
---
sidebar_position: 1
---

# macOS Guide

Comprehensive guide for setting up and using Schedule Management on macOS.

## Installation

### Prerequisites
- macOS 11 (Big Sur) or later, Apple silicon or Intel
- [Rust](https://rustup.rs), to build `rmd`; no other runtime is needed

### Using the Installation Script

The easiest way to install on macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/AutoPhd-org/schedule-everything/main/install.sh | bash
```

The script will:
1. Build the `rmd` binary with `cargo` and install it in `~/SCHEDULE_MANAGEMENT/`
2. Create your config root in `~/.schedule_management/config/`
3. Install the LaunchAgent for auto-start (`rmd daemon`)
4. Create the `reminder` alias and convenience scripts

### Manual Installation

If you prefer manual control, follow the
[manual installation](../installation.md#method-2-manual-installation) steps.
Manual installs do not register the LaunchAgent.

## System Service Setup

### LaunchAgent Configuration

The installation creates a LaunchAgent that runs the reminder service automatically:

**Location**: `~/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist`

**Contents** (abridged):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.sergiudm.schedule.management.reminder</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/YOUR_USERNAME/SCHEDULE_MANAGEMENT/rmd</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/YOUR_USERNAME/SCHEDULE_MANAGEMENT/logs/schedule_management.out</string>
    <key>StandardErrorPath</key>
    <string>/Users/YOUR_USERNAME/SCHEDULE_MANAGEMENT/logs/schedule_management.err</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>REMINDER_CONFIG_DIR</key>
        <string>/Users/YOUR_USERNAME/.schedule_management/config</string>
    </dict>
</dict>
</plist>
```

### Managing the Service

```bash
# Load the service (start automatically)
launchctl load ~/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist

# Unload the service (stop automatically)
launchctl unload ~/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist

# Check if service is running
launchctl list | grep schedule

# View service logs
tail -f ~/SCHEDULE_MANAGEMENT/logs/schedule_management.out
tail -f ~/SCHEDULE_MANAGEMENT/logs/schedule_management.err
```

## Browser Workspace

Launch the local browser interface:

```bash
rmd web
```

Your tasks, schedules, settings, and habit records remain in the same local
files used by the CLI. The browser includes task creation and editing,
completion/cancellation, deadlines, habit checks, today's plan with a clear
option, agent plugin install commands and example prompts, configuration
editing, mode and schedule switching, reminder controls, and PDF downloads. Tasks are grouped by type, with importance,
postponement, and procrastination details preserved. All tasks are shown.

Launching again reuses an open workspace tab. The app asks the browser to bring
it forward; browser focus permissions can prevent it from becoming the active
tab. If the tab has been closed, a new tab opens on the existing server.
The server listens only on `127.0.0.1` and continues running when the terminal
closes. CLI commands remain available.

The installed package includes the browser assets; Node.js is only needed to
rebuild them during development. No native desktop app or widget is shipped.
See [Browser workspace](../browser.md) for details.

## macOS-Specific Configuration

### Sound Files

macOS includes many built-in system sounds:

```toml
[settings]
# Popular system sounds
sound_file = "/System/Library/Sounds/Ping.aiff"
sound_file = "/System/Library/Sounds/Glass.aiff"
sound_file = "/System/Library/Sounds/Hero.aiff"
sound_file = "/System/Library/Sounds/Pop.aiff"
sound_file = "/System/Library/Sounds/Basso.aiff"
sound_file = "/System/Library/Sounds/Funk.aiff"
sound_file = "/System/Library/Sounds/Morse.aiff"
sound_file = "/System/Library/Sounds/Tink.aiff"

# Custom sound files
sound_file = "/Users/yourname/Music/notification.wav"
```

### Notification Settings

Schedule Management uses macOS native notifications. Configure notification settings:

1. **System Preferences** → **Notifications & Focus**
2. Find **Script Editor** (reminder dialogs are shown through `osascript`) or **Terminal** in the app list
3. Configure:
   - Alert style: **Alerts** (for persistent notifications)
   - Allow notifications: **On**
   - Sounds: **On**
   - Badges: **On**

### Security & Privacy

Grant necessary permissions:

1. **System Preferences** → **Security & Privacy** → **Privacy**
2. **Accessibility**: Add Terminal/iTerm if needed
3. **Automation**: Allow `rmd` (or `osascript`) to control System Events if asked

## Troubleshooting macOS Issues

### Service Won't Start

```bash
# Check service
