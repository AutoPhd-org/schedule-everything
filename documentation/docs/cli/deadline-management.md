---
sidebar_position: 4
---

# Deadline Management Commands

Commands for managing event deadlines with automatic date handling and visual urgency indicators.

## ddl add

Add a new deadline or update an existing one, optionally repeating.

### Syntax
```bash
rmd ddl add "EVENT_NAME" "M.D" [--repeat weekly|biweekly|monthly]
```

### Parameters
| Parameter        | Type   | Description                                                |
| ---------------- | ------ | ---------------------------------------------------------- |
| `EVENT_NAME`     | string | Name of the event (quoted if contains spaces)              |
| `M.D` or `MM.DD` | string | Deadline date in month.day format (e.g., "7.4" or "07.04") |
| `--repeat`       | option | Repeat `weekly`, `biweekly` (every two weeks), or `monthly`; the date is the first occurrence |

### Examples
```bash
# Repeating deadlines
rmd ddl add "rent" "12.1" --repeat monthly
rmd ddl add "team report" "11.6" --repeat biweekly

# Add basic deadline
rmd ddl add "homework2" "7.4"

# Add deadline with full date
rmd ddl add "project submission" "12.25"

# Add deadline for next year (if date already passed this year)
rmd ddl add "exam" "3.15"

# Update existing deadline
rmd ddl add "homework2" "7.10"
```

### Smart Date Handling
The system automatically determines the correct year:
- If the date hasn't occurred yet this year → uses current year
- If the date has already passed this year → uses next year

Example on November 22, 2025:
```bash
# These use 2025 (not yet passed)
rmd ddl add "final exam" "12.15"

# These use 2026 (already passed)
rmd ddl add "spring project" "3.20"
```

### Duplicate Handling
If you add a deadline with the same event name as an existing one, it updates the date instead of creating a duplicate:

```bash
# Initial deadline
rmd ddl add "homework2" "7.4"

# Update date (no duplicate created)
rmd ddl add "homework2" "7.10"
```

### Output
```
✅ Deadline 'homework2' added successfully for 2026-07-04!
```

Or when updating:
```
✅ Deadline for 'homework2' updated from 2026-07-04 to 2026-07-10
```

## ddl rm

Remove one or more deadline events.

### Syntax
```bash
rmd ddl rm "EVENT_NAME" [EVENT_NAME...]
```

### Parameters
| Parameter    | Type   | Description                                             |
| ------------ | ------ | ------------------------------------------------------- |
| `EVENT_NAME` | string | Name of the event to delete (quoted if contains spaces) |

### Examples
```bash
# Remove single deadline
rmd ddl rm "homework2"

# Remove multiple deadlines at once
rmd ddl rm "homework2" "project submission" "exam"

# Remove deadline with spaces in name
rmd ddl rm "final project presentation"
```

### Output
```
✅ Deadline 'homework2' deleted successfully!
```

## ddl

List all deadlines sorted by date with urgency indicators.

### Syntax
```bash
rmd ddl
```

### Examples
```bash
# Basic deadline list
rmd ddl
```

### Urgency Status Legend

| Status    | Color  | Criteria | Description       |
| --------- | ------ | -------- | ----------------- |
| 🔴 TODAY   | Red    | 0 days   | Deadline is today |
| 🔴 URGENT  | Red    | 1-3 days | Extremely urgent  |
| 🟡 SOON    | Yellow | 4-7 days | Due within a week |
| 🟢 OK      | Green  | 8+ days  | Plenty of time    |
| ⚠️ OVERDUE | Red    | Negative | Past deadline     |

### Automatic Cleanup

Deadlines are automatically removed once they are two or more days overdue
(`days_left <= -2`). A deadline that was due yesterday remains visible as
overdue, while a deadline that was due two days ago is pruned the next time
deadlines are listed or checked by the reminder service.

Repeating deadlines are never removed this way. At the same point they move to
their next occurrence (a week, two weeks, or a month later), so `rmd ddl`, the
reminders, and the browser always show the next one. In `rmd ddl` their event
names carry a ↻ marker with the repeat rule. A monthly deadline set on the 29th,
30th, or 31st falls on the last day of shorter months and returns to its day
afterward. Remove a repeating deadline with `rmd ddl rm` to stop it.

### Filtering and Sorting

The deadline list is automatically:
- **Sorted by date**: Earliest deadlines appear first
- **Color-coded**: Visual urgency indicators
- **Calculated**: Shows exact days remaining
- **Pruned**: Removes deadlines that are two or more days overdue
