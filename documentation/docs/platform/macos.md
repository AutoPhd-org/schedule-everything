
---
sidebar_position: 1
---

# macOS Guide

Comprehensive guide for setting up and using Schedule Management on macOS.

## Installation

### Prerequisites
- macOS 10.14 (Mojave) or later
- Python 3.12 or higher
- Administrator privileges for system service setup

### Using the Installation Script

The easiest way to install on macOS:

```bash
# Clone the repository
git clone https://github.com/sergiudm/schedule_management.git
cd schedule_management

# Run the installation script
./install.sh
```

The script will:
1. Install Python dependencies
2. Create the application directory (`~/schedule_management/`)
3. Set up configuration files
4. Install the launchd service for auto-start
5. Configure the CLI tool

### Manual Installation

If you prefer manual control:

```bash
# Install Python package
pip install schedule-management

# Create directories
mkdir -p ~/schedule_management/config/user_config_0

# Copy configuration templates
cp config/settings_template.toml ~/schedule_management/config/user_config_0/settings.toml
cp config/week_schedule_template.toml ~/schedule_management/config/user_config_0/odd_weeks.toml
cp config/week_schedule_template.toml ~/schedule_management/config/user_config_0/even_weeks.toml

# Set up shell profile
echo 'export PATH="$HOME/schedule_management:$PATH"' >> ~/.zshrc
echo 'export REMINDER_CONFIG_DIR="$HOME/schedule_management/config"' >> ~/.zshrc
echo 'alias rmd="$HOME/schedule_management/rmd"' >> ~/.zshrc

# Reload shell
source ~/.zshrc
```

## System Service Setup

### LaunchAgent Configuration

The installation creates a LaunchAgent that runs the reminder service automatically:

**Location**: `~/Library/LaunchAgents/com.sergiudm.schedule_management.plist`

**Contents**:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.sergiudm.schedule_management</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/python3</string>
        <string>/Users/YOUR_USERNAME/schedule_management/reminder_macos.py</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/YOUR_USERNAME/schedule_management/logs/stdout.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/YOUR_USERNAME/schedule_management/logs/stderr.log</string>
</dict>
</plist>
```

### Managing the Service

```bash
# Load the service (start automatically)
launchctl load ~/Library/LaunchAgents/com.sergiudm.schedule_management.plist

# Unload the service (stop automatically)
launchctl unload ~/Library/LaunchAgents/com.sergiudm.schedule_management.plist

# Check if service is running
launchctl list | grep schedule

# View service logs
tail -f ~/schedule_management/logs/stdout.log
tail -f ~/schedule_management/logs/stderr.log
```

## Browser Workspace

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
2. Find **Python** or **Terminal** in the app list
3. Configure:
   - Alert style: **Alerts** (for persistent notifications)
   - Allow notifications: **On**
   - Sounds: **On**
   - Badges: **On**

### Security & Privacy

Grant necessary permissions:

1. **System Preferences** → **Security & Privacy** → **Privacy**
2. **Accessibility**: Add Terminal/iTerm if needed
3. **Automation**: Allow Python to control System Events

## Troubleshooting macOS Issues

### Service Won't Start

```bash
# Check service
