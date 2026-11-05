---
name: build-schedule
description: Build or change the user's Schedule Everything weekly routine (settings.toml, odd/even week schedules, habits.toml, profile.md) through a short interview, then validate it and reload reminders with the local `rmd` CLI. Use when the user wants to set up, create, redesign, or adjust their schedule, routine, time blocks, reminder times, or habits.
---

# Build or change a Schedule Everything routine

Schedule Everything is a local reminder app driven by TOML files and the `rmd`
CLI. Your job is to turn the user's life into those files, the way a thoughtful
planner would: understand them first, agree on a plan in plain language, then
write valid configuration and reload reminders.

## 0. Check the environment

1. Run `rmd check --json`. It prints the config root, the active config
   directory and its files, whether each file exists, and validation errors.
   If `rmd` is missing, tell the user to install Schedule Everything first and
   stop.
2. Decide the mode:
   - **Build**: `settings.toml`, `odd_weeks.toml`, or `even_weeks.toml` is
     missing, or the user asks to start over.
   - **Modify**: the files exist and the user wants changes.
3. Read the existing files and `profile.md` if present. Treat an existing
   profile as a draft to refine, not something to discard.

See [references/config-format.md](references/config-format.md) for every file
format, with examples.

## 1. Understand the user (build, or when the profile is thin)

Ask focused questions, a few at a time, until you can plan well. Cover what
the user can realistically provide:

- role, life context, timezone
- wake and sleep window
- fixed obligations and no-go windows (classes, meetings, commute, caregiving)
- main goals and priorities
- habits, exercise, recovery needs
- energy pattern: best-focus and low-energy windows
- preferences and patterns they dislike (meeting grouping, minimum focus length)
- upcoming deadlines or workload peaks
- odd/even week differences (alternating timetables)

If the user shares a timetable file or image, read it yourself and ask only
targeted clarifications. Update `profile.md` in your head as you go; you will
write it at the end.

## 2. Propose a plain-language summary and get confirmation

Before writing any file, show a short, readable summary: a typical weekday,
special days, the weekend, focus blocks and their lengths, breaks, exercise,
habits, and reminder times. Ask the user to confirm or adjust. Do not write
configuration until they confirm. For a small, unambiguous modification, a
one-line restatement is enough.

Use these evidence-informed defaults unless the user says otherwise:

- Protect at least 7 hours of sleep opportunity with consistent sleep/wake times.
- Do not trade sleep for work when other options exist.
- For general health, spread about 150–300 minutes of moderate activity across
  the week, with strength work on 2 or more days when feasible.
- Break up long sedentary work with short movement or recovery breaks.
- Put demanding cognitive work in the user's best-focus windows and admin in
  low-energy windows.
- Prefer daylight earlier in the day; avoid stimulating late-night work.

These are scheduling heuristics, not medical advice. The user's own
constraints (clinical needs, disability, shift work, caregiving) always win.

## 3. Write the configuration

**Build mode**: write `settings.toml`, `odd_weeks.toml`, `even_weeks.toml`,
`habits.toml`, and `profile.md` into the active config directory.

**Modify mode**: keep a rollback point by making a new version.

1. From `rmd check --json`, take `activeConfigDir`, `nextConfigId`, and
   `nextConfigDir`.
2. Copy the whole active directory to `nextConfigDir`.
3. Make the smallest effective edit in the copy. Keep unrelated sections,
   keys, names, and comments untouched. Update `profile.md` if the request
   changes the user's situation or preferences.
4. Validate the copy with `rmd check --dir <new directory>`.
5. Activate it with `rmd switch <nextConfigId>` (this also reloads reminders).

Rules for both modes:

- Use labels defined in `settings.toml`: a `[time_blocks]` or `[time_points]`
  name, or a `{ block = "...", title = "..." }` inline table whose `block` is a
  time block. An undefined plain label only shows as a message with no
  duration, so `rmd check` warns about it; treat that warning as a typo unless
  the user wants a one-off message.
- Use quoted 24-hour `"HH:MM"` keys. Avoid overlaps: leave each block's length
  free before the next entry.
- If there is no odd/even difference, make the two week files identical.
- Keep names short, stable, and reusable (`meeting`, `exercise`, `lunch_time`).
- Name plannable focus blocks `pomodoro` or `potato` (set their lengths in
  `[time_blocks]`); only those, without a fixed title, can be filled day by day
  with the `plan-day` skill.
- Prefer `[common]` for entries shared by every day.

## 4. Validate and reload

1. Run `rmd check` (or `rmd check --dir <dir>`). Fix every error and run it
   again until it passes. Review warnings too: fix typos, and mention any you
   leave on purpose. Never leave the user with invalid files.
2. Build mode: run `rmd update` to load the new schedule into the reminder
   service. Modify mode: `rmd switch` already reloaded it.
3. Run `rmd status` and show the user what is active now and what comes next.
4. Tell the user what changed, where the files are, and that they can ask
   you for adjustments or plan a day with the `plan-day` skill.

## Safety

- Only write inside the config root reported by `rmd check`.
- Do not delete existing `user_config_<n>` directories; they are the user's
  history.
- Do not edit `tasks/` data files (tasks, logs, habit records) in this skill.
