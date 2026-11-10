
---
sidebar_position: 2
---

# Settings Configuration

The `settings.toml` file contains global configuration, reusable time blocks, and reminder messages. This file is the foundation of your Schedule Management setup.

## File Structure

```toml
[settings]
# Global system settings

[time_blocks]
# Reusable time block definitions

[time_points]
# Reusable time point message definitions

[task_settings]
# Task management settings (optional)
```

## Settings Section

### Basic Settings

```toml
[settings]
language = "en"
sound_file = "/System/Library/Sounds/Ping.aiff"
alarm_interval = 5
max_alarm_duration = 300
show_tasks_after_add = false
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `language` | string | `"en"` | Interface language for the CLI, reminder popups, and the browser workspace. Supported values are `"en"` (English), `"zh"` (Chinese / 中文), `"es"` (Spanish / Español), and `"ko"` (Korean / 한국어). The browser's **Settings → Language** picker writes this value. Can also be overridden temporarily via the `REMINDER_LANG` environment variable. |
| `sound_file` | string | `"/System/Library/Sounds/Ping.aiff"` | Path to the sound file for notifications |
| `alarm_interval` | integer | `5` | Seconds between repeated alerts |
| `max_alarm_duration` | integer | `300` | Maximum duration for alerts in seconds (5 minutes) |
| `show_tasks_after_add` | boolean | `false` | When `true`, every successful `rmd add ...` immediately prints the same task table as `rmd ls`. |

### Advanced Settings

```toml
[settings]
sound_file = "/System/Library/Sounds/Glass.aiff"
alarm_interval = 3
max_alarm_duration = 600
config_dir = "~/.schedule_management/config"
log_level = "INFO"
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `config_dir` | string | Custom configuration directory path |
| `log_level` | string | Logging level (DEBUG, INFO, WARNING, ERROR) |

## Time Blocks Section

Time blocks define reusable durations for scheduled activities. Each time block triggers both start and end notifications.

### Common Time Blocks

```toml
[time_blocks]
pomodoro = 25           # 25-minute work sessions
long_break = 40         # 40-minute breaks
short_break = 5         # 5-minute short breaks
meeting = 50            # 50-minute meetings
exercise = 30           # 30-minute exercise sessions
lunch = 60              # 1-hour lunch break
napping = 30            # 30-minute naps
deep_work = 90          # 90-minute deep work sessions
```

### Custom Time Blocks

You can define any time block that suits your routine:

```toml
[time_blocks]
coding = 45             # 45-minute coding sessions
reading = 20            # 20-minute reading sessions
meditation = 15         # 15-minute meditation
standup = 15            # 15-minute standup meetings
review = 30             # 30-minute review sessions
planning = 20           # 20-minute planning sessions
```

## Time Points Section

Time points define reusable messages for one-time reminders.

### Common Time Points

```toml
[time_points]
go_to_bed = "Time to wind down and get ready for bed 😴"
summary_time = "Great work today! Time to summarize your accomplishments 🎉"
wake_up = "Good morning! Time to start your day 🌅"
lunch_time = "Lunch time! Take a break and nourish yourself 🍽️"
stretch_time = "Time to stand up and stretch your body 🧘"
hydrate = "Remember to drink some water 💧"
```

### Custom Time Points

Create time points that match your routine:

```toml
[time_points]
check_email = "Time to check and respond to emails 📧"
daily_standup = "Daily standup meeting starts now 👥"
code_review = "Time to review pull requests 🔍"
retrospective = "Sprint retrospective meeting 📝"
demo_time = "Demo time! Show your work 🎬"
retrospect = "Time to reflect on your day 🤔"
```

## Task Settings Section (Optional)

Configure task management behavior:

```toml
[task_settings]
max_tasks = 100                 # Maximum number of tasks
default_importance = 5          # Default importance level (1-10)
auto_cleanup_days = 30          # Auto-cleanup completed tasks after N days
show_completed = false          # Show completed tasks in list
```

## Task Types Section

Task types are shared by tasks and deadlines. Each key is a type ID and each
value its name; the browser adds, renames, and deletes them on the task board.
Once you change a type there, every type's color is saved in
`[task_type_colors]`, so deleting a type never recolors the others. A type
without a saved color takes the palette color of its place in the list.

```toml
[task_types]
1 = "Coursework"
2 = "Research"

[task_type_colors]
1 = "blue"      # blue, orange, aqua, yellow, magenta, green, violet, or red
2 = "orange"
```

## Browser Settings

Launch `rmd web`, then open **Settings** to edit reminders, settings, odd/even
week schedules, habits, deadlines, or your profile. Task types are added,
renamed, and deleted on the task board in the overview. The configuration editor
validates TOML and refuses to overwrite a file that changed since it was loaded.
The browser uses the same active configuration as the CLI.

## Sound File Configuration

### macOS System Sounds
```toml
sound_file = "/System/Library/Sounds/Ping.aiff"
sound_file = "/System/Library/Sounds/Glass.aiff"
sound_file = "/System/Library/Sounds/Hero.aiff"
sound_file = "/System/Library/Sounds/Pop.aiff"
```

### Custom Sound Files
```toml
sound_file = "/Users/yourname/Music/notification.wav"
sound_file = "/Users/yourname/Music/gentle-chime.mp3"
```

### Linux Sound Files
```toml
sound_file = "/usr/share/sounds/freedesktop/stereo/complete.oga"
sound_file
