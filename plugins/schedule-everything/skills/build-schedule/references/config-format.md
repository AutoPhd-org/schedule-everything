# Schedule Everything configuration format

All files live in the active config directory reported by `rmd check`
(usually `~/.schedule_management/config/user_config_<n>/`). Times are 24-hour
`"HH:MM"` strings and must be quoted as TOML keys.

## settings.toml

Required tables: `[settings]`, `[time_blocks]`, `[time_points]`, `[tasks]`,
`[paths]`. `[task_types]` is optional but recommended.

```toml
[settings]
sound_file = "/System/Library/Sounds/Ping.aiff"
alarm_interval = 5          # seconds between alarm repeats
max_alarm_duration = 300    # seconds
skip_days = ["sunday"]      # weekdays with no reminders
language = "en"             # "en", "zh", "es", or "ko"
show_tasks_after_change = true

[time_blocks]               # name = length in minutes; a block alarm also fires when it ends
pomodoro = 25           # plannable short focus block
potato = 90             # plannable deep-work block
meeting = 50
long_break = 40

[time_points]               # name = message shown at that moment (no end alarm)
lunch_time = "Lunch time 🍱"
exercise = "Time to move 🏃"
summary_time = "Wrap up today and write a short summary"
go_to_bed = "Time for bed 😴"

[tasks]
daily_urgent = ["08:31", "14:01", "17:31", "21:31"]   # urgent-task reminders
ddl_urgent = ["08:32", "14:02", "17:32", "22:32"]     # deadline reminders
daily_summary = "23:35"
habit_prompt = "23:50"                                 # daily habit check-in
weekly_review = "sunday 20:00"
monthly_review = "1 20:00"                             # day-of-month HH:MM

[paths]
config_dir = "config"
reports_path = "~/Desktop/reports"

[task_types]                # shared by tasks and deadlines; keep IDs stable
1 = "Research"
2 = "Coding"
3 = "Writing"
4 = "Other"
```

Keep the user's existing `[settings]`, `[tasks]`, `[paths]`, and
`[task_types]` values unless they ask to change them.

## odd_weeks.toml and even_weeks.toml

The app alternates between these by ISO week parity (`rmd status` shows the
current parity). If the user has no odd/even difference, make both files
identical.

Allowed tables: `[monday]` … `[sunday]` and `[common]`. `[common]` applies to
every day; a weekday entry at the same time overrides it.

```toml
[monday]
"09:00" = "potato"
"10:45" = "meeting"
"12:30" = "lunch_time"

[wednesday]
"09:00" = "potato"
"14:00" = { block = "meeting", title = "Advisor 1:1" }

[common]
"18:30" = "exercise"
"23:00" = "go_to_bed"
```

Each value is either:

- the name of a `[time_blocks]` or `[time_points]` entry, or
- an inline table `{ block = "<time block name>", title = "<what it is for>" }`
  for a block with a fixed purpose.

Every name used must exist in `settings.toml`. Leave enough room after each
block for its length; avoid overlapping blocks.

Daily planning (`rmd sync` and the `plan-day` skill) fills only blocks named
`pomodoro` or `potato` that have no fixed `title`. Use those two block names,
with whatever lengths suit the user, for focus time they want to plan day by
day; other blocks (for example `meeting`) are never assigned tasks.

## habits.toml

```toml
[habits]
1 = "Sleep by 23:30"
2 = "Exercise 30 minutes"
3 = "Read 20 pages"
```

Keep existing IDs stable so habit history stays meaningful.

## profile.md

A Markdown profile used for future schedule changes. Suggested headings:

```markdown
# Basic Information
- Role, timezone, wake/sleep window, meals, commute

# Main Goals

# Hard Constraints
- Fixed classes, meetings, no-go windows

# Preferences
- Best focus windows, disliked patterns, meeting grouping

# Habits

# Energy Pattern
```
