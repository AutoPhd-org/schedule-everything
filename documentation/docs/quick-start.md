---
sidebar_position: 3
---

# Quick Start

This guide focuses on the fastest path to a useful schedule.

If you want the AI-assisted flow, start with `rmd setup`.
If you prefer hand-editing TOML, the manual path is included later on this page.

## Fastest Path: AI-Assisted Setup

### 1. Install the project and pi

```bash
git clone https://github.com/sergiudm/schedule-everything.git
cd schedule-everything
./install.sh
npm install -g @earendil-works/pi-coding-agent
```

### 2. Run the profile-first planner

```bash
rmd setup
```

What happens:

- pi handles credentials and model selection; authenticate with `pi` first. An optional model override can be placed in `~/.schedule_management/llm.toml`.
- It creates or refines `profile.md` next to your schedule files.
- It asks follow-up questions until that profile is detailed enough to plan from.
- It shows a summary before writing the schedule.

### 3. Add tasks and sync today

```bash
rmd add "Finish proposal draft" 9
rmd add "Review pull request" 7
rmd sync
```

`rmd sync` reads `tasks/tasks.json`, assigns today's untitled
`pomodoro`/`potato` blocks to specific tasks, shows a preview, and only writes
`synced_schedule.toml` after approval.

### 4. Verify and launch

```bash
rmd status
rmd status -v
rmd view
rmd update
```

## Manual Path: Hand-Editing the Schedule

Use this if you want full manual control over the TOML files.

### 1. Create the config files

```bash
mkdir -p ~/schedule_management/config/user_config_0
cp config/settings_template.toml ~/schedule_management/config/user_config_0/settings.toml
cp config/week_schedule_template.toml ~/schedule_management/config/user_config_0/odd_weeks.toml
cp config/week_schedule_template.toml ~/schedule_management/config/user_config_0/even_weeks.toml
```

Later accepted schedule edits create `user_config_1`, `user_config_2`, and so
on under the same config root.

### 2. Define reusable blocks in `settings.toml`

```toml
[settings]
sound_file = "/System/Library/Sounds/Ping.aiff"
alarm_interval = 5
max_alarm_duration = 300
show_tasks_after_add = false

[time_blocks]
pomodoro = 25
short_break = 5
long_break = 15
meeting = 60
lunch = 60

[time_points]
bedtime = "Wind down and disconnect 😴"
standup = "Daily Standup Meeting 🗣️"
```

### 3. Map time to actions in `odd_weeks.toml`

```toml
[common]
"12:00" = "lunch"
"23:00" = "bedtime"

[monday]
"09:00" = "pomodoro"
"09:25" = "short_break"
"09:30" = "pomodoro"
"10:00" = { block = "meeting", title = "Weekly Planning" }
```

For a simple weekly schedule, copy the same structure to `even_weeks.toml`.

### 4. Optional: add a synced overlay later

Once you start using tasks, `rmd sync` can generate a daily overlay file:

```toml
[schedule]
"09:00" = { block = "pomodoro", title = "Finish proposal draft" }
```

That overlay lives in `synced_schedule.toml` and does not rewrite your base
odd/even week templates.

## Next Steps

- [Installation](installation.md)
- [Configuration Overview](configuration/overview.md)
- [Weekly Schedules](configuration/weekly-schedules.md)
- [CLI Overview](cli/overview.md)


## Browser workspace

Run `rmd web` to manage the same local data through the browser. The task board
groups all tasks by type and retains importance and reminder details. Existing
CLI syntax and task numbering are unchanged. See [Browser workspace](browser.md).
