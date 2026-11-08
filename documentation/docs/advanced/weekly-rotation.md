
---
sidebar_position: 1
---

# Weekly Rotation System

Schedule Everything features an intelligent weekly rotation system that automatically alternates between different schedules based on ISO week numbering.

## How Weekly Rotation Works

The system uses the ISO 8601 week numbering standard to determine whether a week is odd or even:

- **Odd weeks**: Weeks 1, 3, 5, 7, 9, etc.
- **Even weeks**: Weeks 2, 4, 6, 8, 10, etc.

The ISO week numbering system ensures consistent week numbering across years and handles edge cases like weeks that span year boundaries.

## Configuration Files

### Odd Weeks (`odd_weeks.toml`)
Used during odd-numbered weeks. Typically contains:
- More intensive work schedules
- Focus-heavy activities
- Longer work sessions

### Even Weeks (`even_weeks.toml`)
Used during even-numbered weeks. Typically contains:
- Lighter schedules
- More breaks and flexibility
- Meeting-heavy weeks

## ISO Week Numbering

### Understanding ISO Weeks
- Week 1 is the first week with at least 4 days in the new year
- Weeks start on Monday and end on Sunday
- The system handles year transitions automatically

### Examples
```
2024-W01 (January 1-7, 2024) → Uses odd_weeks.toml
2024-W02 (January 8-14, 2024) → Uses even_weeks.toml
2024-W03 (January 15-21, 2024) → Uses odd_weeks.toml
```

### Checking Current Week
```bash
# Check current ISO week
date +%V

# Check if current week is odd or even
date +%V   # ISO week number: odd weeks use odd_weeks.toml, even weeks even_weeks.toml
```

## Use Cases for Weekly Rotation

### Alternating Work Intensity
**odd_weeks.toml** - High intensity:
```toml
[monday]
"08:00" = "deep_work"
"10:00" = "short_break"
"10:15" = "deep_work"
"12:00" = "lunch"
"13:00" = "deep_work"
"15:00" = "short_break"
"15:15" = "deep_work"
```

**even_weeks.toml** - Moderate intensity:
```toml
[monday]
"09:00" = "pomodoro"
"10:00" = "long_break"
"11:00" = "pomodoro"
"12:00" = "lunch"
"14:00" = "meeting"
"15:00" = "pomodoro"
```

### Alternating Meeting Schedules
**odd_weeks.toml** - Meeting-heavy:
```toml
[monday]
"09:00" = { block = "meeting", title = "Team Standup" }
"10:00" = { block = "meeting", title = "Sprint Planning" }
"11:00" = { block = "meeting", title = "Client Review" }
"14:00" = { block = "meeting", title = "1-on-1 Meetings" }
```

**even_weeks.toml** - Focus time:
```toml
[monday]
"08:30" = "deep_work"
"10:30" = "long_break"
"11:30" = "deep_work"
"14:00" = "deep_work"
```

### Alternating Personal Activities
**odd_weeks.toml** - Gym focus:
```toml
[monday]
"06:00" = { block = "exercise", title = "Strength Training" }
"18:00" = { block = "exercise", title = "Cardio Session" }
```

**even_weeks.toml** - Recovery focus:
```toml
[monday]
"06:30" = "meditation"
"18:00" = { block = "yoga", title = "Restorative Yoga" }
```

## Advanced Rotation Patterns

### Bi-weekly Sprints
Perfect for agile development teams:

**odd_weeks.toml** - Sprint weeks:
```toml
[monday]
"09:00" = { block = "planning", title = "Sprint Planning" }
"11:00" = "pomodoro"
"14:00" = "pomodoro"

[friday]
"14:00" = { block = "review", title = "Sprint Review" }
"15:00" = { block = "retrospective", title = "Sprint Retrospective" }
```

**even_weeks.toml** - Development weeks:
```toml
