---
name: plan-day
description: Plan the user's day in Schedule Everything by assigning open tasks to today's focus blocks, then saving the plan with `rmd sync --apply` so reminders announce each block's task. Use when the user asks to plan today, fill their focus blocks, decide what to work on, or replan the rest of the day.
---

# Plan today's focus blocks

Schedule Everything reminds the user at each block of their weekly routine.
A daily plan gives today's focus blocks a concrete purpose ("pomodoro: Draft
the intro"). The plan only affects today and is stored locally; the weekly
routine files are never changed.

## 1. Read today's context

Run `rmd sync --json`. It prints:

- `date`, `weekday`, `parity`, `mode`, `skipped`
- `slots`: today's assignable focus blocks, each with `time`, `block`,
  `minutes`, and the currently planned `title` (or `null`)
- `events`: the whole day's schedule for context
- `tasks`: open tasks with `description`, `priority` (1–10), `typeName`,
  `alarmFrom` (postponed until that date), and `procrastinated`
- `deadlines`: upcoming deadlines with `daysLeft`
- `plan`: the plan already saved for today, or `null`

Stop and tell the user if:

- `mode` is `"p"`: P mode is the flexible, schedule-free mode. Daily plans are
  for J mode; the user can switch with `rmd mode j`.
- `skipped` is true: today is a skip day with no reminders.
- `slots` is empty: there are no focus blocks today to fill.

## 2. Draft the plan

Assign one task to each slot you can fill well. Guidelines:

- Ask the user briefly what matters most today if it is not obvious, and
  respect anything they say over these defaults.
- Start from importance and deadlines: tasks tied to deadlines in the next few
  days and tasks with priority 8–10 first.
- Skip tasks whose `alarmFrom` is in the future (they are postponed).
- Give procrastinated tasks an early, short slot to restart momentum.
- Match task weight to block length (`minutes`) and time of day: demanding
  work in the morning or the user's best-focus window, lighter work late.
- A large task can span several consecutive slots; use the same title or
  numbered parts ("Thesis ch. 3 (1/2)").
- Leave a slot unassigned rather than inventing work. Titles must be short
  (under 80 characters) and match a task description when one exists.

Show the plan as a compact time-ordered list and ask for confirmation or
changes. Revise until the user is happy.

## 3. Save the plan

Write the plan to a temporary JSON file and apply it:

```json
{
  "date": "2026-11-04",
  "assignments": {
    "09:00": "Draft thesis intro",
    "14:00": "Review lab PR #42"
  }
}
```

```bash
rmd sync --apply /path/to/plan.json
```

`rmd sync --apply -` reads the same JSON from standard input. Keys must be
slot `time`s from step 1, `date` must be today, and you may leave slots out.
The command validates and saves the plan; the running reminder service picks it
up within a minute, with no restart. If it reports an error, fix the JSON and
apply again.

Applying replaces today's previous plan. To remove today's plan entirely, run
`rmd sync --clear`.

## 4. Confirm

Run `rmd status` and tell the user what is happening now and next. Mention
that the browser workspace (`rmd web`) shows the plan on the Schedule page.
