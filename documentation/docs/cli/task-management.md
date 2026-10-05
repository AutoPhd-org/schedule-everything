---
sidebar_position: 3
---

# Task Management Commands

Commands for managing your personal task list with importance levels and smart duplicate handling.

## add

Add a new task or update an existing one with an importance level.

### Syntax
```bash
rmd add "TASK_DESCRIPTION" PRIORITY_LEVEL [TASK_TYPE] [POSTPONE_DAYS]
```

### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `TASK_DESCRIPTION` | string | Description of the task (quoted if contains spaces) |
| `PRIORITY_LEVEL` | integer | Priority level from 1-10 (higher = more important) |
| `TASK_TYPE` | integer (optional) | Task type ID as configured in `settings.toml` (e.g. 1 for "read papers", 2 for "gym work") |
| `POSTPONE_DAYS` | integer (optional) | Number of days to postpone the daily urgent alarm (e.g. 1 for tomorrow, 2 for two days later) |

### Examples
```bash
# Add basic task (defaults to task type 1)
rmd add "Complete project proposal" 8

# Add task with a specific task type (e.g. 2 for gym work)
rmd add "Go to the gym" 7 2

# Add high-priority task with a specific type and alarm postponed to tomorrow
rmd add "Biology homework" 9 1 1
```

### Interactive Mode
If you run `rmd add` directly without parameters in an interactive terminal, or if you omit some parameters, the system will display a friendly **Task Creator Wizard** panel, guiding you through a couple of quick questions one by one with emojis to complete the task description, priority level, task type, and the optional **postpone days** (to make it a future task whose daily urgent alarm starts later). Press Enter at the postpone prompt to skip it and start the alarm today.

#### Creating a new task type inline
At the **task type** prompt, the configured types are listed by number (e.g. `1. read papers`). If you enter a **positive number that is not on the list**, it is treated as a new type and the wizard will:

1. Ask you to enter a **name** for the new type.
2. Save it to `settings.toml` under `[task_types]` (so it appears for future tasks too).
3. Assign the new type to the current task.

```bash
$ rmd add
...
🔢 Select a task type:
  1. read papers
  2. gym work
  3. coding
  4. other
Enter Task Type Number: 5
✨ New task type! Let's create type 5.
🏷️  Enter a name for task type 5: writing
✅ Task type 'writing' (number 5) created and saved!
```

If you press Enter without a name, or the name already exists, you'll be asked again. Pressing `Ctrl+C` cancels the new-type creation and returns you to the task type selection prompt. Non-numeric input is still rejected.

If you are running in a non-interactive environment (such as a script), omitting the required parameters will print an error message and exit with an error code. Any omitted optional parameters (like task type) will fall back to their default values (e.g. task type 1).

### Task List Color Coding & Legend
In `rmd ls`, the color of each task row/priority bar represents the **Task Type** instead of the priority level. A color legend is displayed at the bottom of the task list showing the association between colors and task types configured in `settings.toml`.

### Smart Duplicate Handling
If you add a task with the same description as an existing task, it updates the priority level instead of creating a duplicate:

```bash
# Initial task
rmd add "Buy groceries" 3

# Update importance (no duplicate created)
rmd add "Buy groceries" 6
```

### Show the List After Adding
By default, `rmd add` prints only the add/update confirmation. To always display
the full `rmd ls` task table after a successful add or update, enable this in
`settings.toml`:

```toml
[settings]
show_tasks_after_add = true
```

## rm

Remove one or more tasks by description or ID.

`rm` counts the removal as a **completion** — the task is recorded in history
with action `deleted` and shows up in `rmd history`, reports, and the daily
popup's completed-task count. Use [`cancel`](#cancel) or [`drop`](#drop)
instead when the task should NOT be counted as done.

### Syntax
```bash
rmd rm TASK_IDENTIFIER [TASK_IDENTIFIER...]
```

### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `TASK_IDENTIFIER` | string/integer | Task description (quoted) or task ID number (from `rmd ls`) |

### Examples
```bash
# Remove by description (counts as completed)
rmd rm "Buy groceries"

# Remove multiple tasks by description
rmd rm "Call dentist" "Organize desk"

# Remove by task ID
rmd rm 1 2 3
```

## cancel

Cancel one or more tasks that were added **by mistake**. Use this when an
earlier `rmd add` was wrong.

`cancel` takes the task out of `tasks.json` just like `rm` (and syncs the
procrastinate list), but it records the action as `cancelled` instead of
`deleted`, so it is **not** counted as a completion in history, reports, or the
daily popup. In `rmd history`, cancelled tasks are shown with a `🚫 cancelled`
tag and a dimmed, struck-through style so they are visually distinct from
completed work.

### Syntax
```bash
rmd cancel TASK_IDENTIFIER [TASK_IDENTIFIER...]
```

### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `TASK_IDENTIFIER` | string/integer | Task description (quoted) or task ID number (from `rmd ls`) |

### Examples
```bash
# Cancel an accidentally added task (not counted as done)
rmd cancel "typo task"

# Cancel by task ID
rmd cancel 2

# Cancel multiple at once
rmd cancel "wrong task" "another typo"
```

## drop

Give up on one or more tasks — for example, when your task list has grown too
long and you want to abandon some items without crediting them as done.

`drop` takes the task out of `tasks.json` just like `rm` (and syncs the
procrastinate list), but it records the action as `dropped` instead of
`deleted`, so it is **not** counted as a completion in history, reports, or the
daily popup. In `rmd history`, dropped tasks are shown with a `🏳️ dropped` tag
in yellow so they are visually distinct from both completed and cancelled work.

### Syntax
```bash
rmd drop TASK_IDENTIFIER [TASK_IDENTIFIER...]
```

### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `TASK_IDENTIFIER` | string/integer | Task description (quoted) or task ID number (from `rmd ls`) |

### Examples
```bash
# Give up on a task (not counted as done)
rmd drop "side project"

# Drop by task ID
rmd drop 4

# Drop several at once
rmd drop "low priority idea" "stale research"
```

### rm vs cancel vs drop

All three commands remove the task from your list and keep the procrastinate
list in sync. They differ only in the recorded history:

| Command | Recorded action | Counts as completion | History look |
|---------|-----------------|----------------------|--------------|
| `rmd rm` | `deleted` | ✅ Yes | normal (priority styling) |
| `rmd cancel` | `cancelled` | ❌ No | `🚫 cancelled` (dim, struck through) |
| `rmd drop` | `dropped` | ❌ No | `🏳️ dropped` (yellow) |

## ls

List all tasks sorted by urgency and importance (highest first).

### Syntax
```bash
rmd ls [-t TASK_TYPE]
```

### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `-t`, `--type` | string (optional) | Only show tasks of this type. Accepts a task type **ID** (e.g. `1`) or **name** (e.g. `coding`); name matching is case-insensitive. |

### Examples
```bash
# Basic task list
rmd ls

# Only show tasks of type 1 (e.g. "read papers")
rmd ls -t 1
rmd ls --type 1

# Filter by type name instead of ID (case-insensitive)
rmd ls --type coding
rmd ls -t "Gym Work"
```

#### Filtered IDs stay consistent

When you filter with `--type`, the displayed **ID numbers stay aligned with the unfiltered list** — they are not re-numbered. So an ID you see in `rmd ls -t 1` can be passed straight to `rmd rm`, `rmd cancel`, or `rmd drop` (those commands always resolve IDs against the full `rmd ls` order).

```bash
$ rmd ls -t 1
 ID │ Priority         │ Description
  1 │ ████████░░ (8)   │ Read paper        # type 1
  3 │ ███░░░░░░░ (3)   │ Review paper      # type 1

$ rmd rm 3   # removes "Review paper" (same ID as in the full list)
```

The footer reflects the filter, e.g. `Showing 2 of 5 tasks (type: read papers)`. If no tasks match, you get `No tasks found for type '...'`; if the type itself is unknown, the command prints `Unknown task type '...'` with the valid types and exits with an error.

### Procrastinate Tag

At each configured `daily_urgent` / `daily_urgency` time, the reminder service opens a task-by-task popup for high-priority tasks (priority 8-10). For each task:

- Click `Yes` to mark it complete and remove it from `tasks.json`
- Click `No` to keep it and mark it as procrastinated
- Click `Stop` to leave the remaining urgent tasks unchanged

Tasks you mark as **not completed** are recorded in a procrastinate list file:

- `tasks/procrastinate.json` under your `REMINDER_CONFIG_DIR`
- Each entry stores the task description and the first day it was deferred

In `rmd ls`, procrastinated tasks are shown with:

- A `⏳` prefix
- A procrastination age label such as `(deferred today)` or `(3 days overdue)`
- A dim/italic style if deferred today, or a striking bold red style if overdue (1 or more days)

In later urgent-task popups, the reminder also shows how many days the task has already been procrastinated.

When a procrastinated task is complete (`rmd rm`), it is automatically removed from `tasks/procrastinate.json`.

## history

Display recent completed task activities, grouped by calendar day with visual separators.

Each activity shows the task description, its priority level, when it was started (added), when it ended (removed), and the total duration.

An activity's end depends on which command removed the task, and history renders them distinctly:
- **Completed** (`rmd rm`) — normal priority styling.
- **Cancelled** (`rmd cancel`) — shown with a `🚫 cancelled` tag, dimmed and struck through (added by mistake).
- **Dropped** (`rmd drop`) — shown with a `🏳️ dropped` tag in yellow (gave up).

Only `rm` removals count as completion; `cancel` and `drop` activities still appear in history so you can tell what happened, but they are excluded from completion counts in reports and the daily popup.

### Syntax
```bash
rmd history [COUNT]
```

### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `COUNT` | integer (optional) | Number of recent activities to show (default: 5) |

### Examples
```bash
# Show the 5 most recent completed activities
rmd history

# Show the 10 most recent completed activities
rmd history 10

# Show the 3 most recent completed activities
rmd history 3
```

---

# Habit Management Commands

Commands for interacting with your configured daily habits.

## <a name="habits"></a>track

Log completed habits for today. Habits are loaded from `habits.toml`.

### Syntax
```bash
rmd track [HABIT_IDS...]
```

### Parameters
| Parameter | Type | Description |
|-----------|------|-------------|
| `HABIT_IDS` | array of strings | (Optional) Space-separated list of Habit IDs to mark completed for today. |

### Examples
```bash
# Mark habits 1 and 2 as completed for today
rmd track 1 2

# Open an interactive prompt window to tick off habits
rmd track
```


## Browser workspace

Run `rmd web` to manage the same local data through the browser. The task board
groups all tasks by type and retains importance and reminder details. Existing
CLI syntax and task numbering are unchanged. See [Browser workspace](../browser.md).
