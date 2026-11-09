---
sidebar_position: 4
title: AI agent plugins
description: Build your weekly routine, plan each day, and get task suggestions to review with Claude Code, OpenAI Codex, or Google Antigravity through the schedule-everything plugin.
---

# AI agent plugins

Schedule Everything has no AI built in. Instead, the **`schedule-everything`
plugin** teaches your own coding agent (Claude Code, OpenAI Codex, or Google
Antigravity) to build your weekly routine, plan your days, and suggest tasks
it finds in your email and calendar for you to review. The agent does
the thinking; the local `rmd` CLI does the deterministic work and your
reminders follow the files it writes.

<Figure
  src="img/diagrams/how-it-works.svg"
  alt="You ask your AI coding agent; the schedule-everything plugin runs rmd commands; rmd validates and writes local files that drive reminders and the browser workspace"
/>

## Install the plugin

Install Schedule Everything first (see [Installation](installation.md)), so the
`rmd` command is available. Then add the plugin to your agent:

<AgentInstall />

The browser workspace has the same commands with copy buttons: open
`rmd web`, go to **Schedule**, and use the **Plan with your AI agent** card.

## Three skills

| Skill | Use it to |
| --- | --- |
| `build-schedule` | Interview you, agree on a plan in plain language, write `settings.toml`, `odd_weeks.toml`, `even_weeks.toml`, `habits.toml`, and `profile.md`, then validate with `rmd check` and reload reminders. |
| `plan-day` | Read today's focus blocks and open tasks with `rmd sync --json`, agree on a plan with you, and save it with `rmd sync --apply`. |
| `suggest-tasks` | Look through what it knows about you and your connected apps for work you have not captured, and save a few suggestions with `rmd suggest add`. You add or reject each one on the task board. |

### build-schedule

1. Runs `rmd check --json` to find your config root, the active config
   directory, and which files exist.
2. Asks focused questions (wake and sleep window, fixed obligations, goals,
   habits, energy pattern, odd/even week differences) and keeps them in
   `profile.md`. If you share a timetable file or image, it reads it and only
   asks what is unclear.
3. Shows a plain-language summary of a typical week and **waits for your
   confirmation** before writing any file.
4. Writes the files, validates them with `rmd check`, and reloads reminders
   with `rmd update`.

Changes to an existing schedule never overwrite it: the skill copies the
active directory to the next `user_config_<n>` version, edits the copy,
validates it with `rmd check --dir`, and activates it with
`rmd switch <n>`. Older versions stay for rollback.

When you leave details open, it applies evidence-informed defaults: at least
7 hours of sleep opportunity, 150–300 minutes of moderate activity per week
with strength work on 2 or more days, movement breaks in long desk stretches,
demanding work in your best-focus window, and daylight early in the day. These
are heuristics, not medical advice; your own constraints win. See the
[Introduction](intro.md#evidence-informed-defaults) for sources.

### plan-day

1. Runs `rmd sync --json` to read today's focus blocks (untitled `pomodoro`
   and `potato` blocks), open tasks with importance and postponement, and
   upcoming deadlines.
2. Matches the most important tasks and nearest deadlines to the blocks,
   shows you a time-ordered plan, and revises it until you are happy.
3. Saves it with `rmd sync --apply`. Reminders and `rmd status` announce each
   block's task, without a restart.

<Figure
  src="img/screens/schedule-agents.webp"
  alt="The Schedule page with today's timeline, the today's plan card, and tabbed plugin install commands for Claude Code, Codex, and Antigravity"
  caption="After planning, each focus block on the Schedule page shows its task. Clear today's plan removes it; your weekly files never change."
/>

A plan changes only today. P mode and skip days have no plan: the skill tells
you and stops.

### suggest-tasks

1. Runs `rmd suggest ls --json` to read your task types, open tasks, the
   suggestions still waiting, the ones you accepted or rejected in the last 60
   days, and your `profile.md`.
2. Looks through what it knows about you and the apps connected to your agent
   (email, calendar, documents, issue trackers) for things that need doing:
   a request waiting for your reply, a form or bill with a date, a talk to
   prepare. It only reads those apps, and treats what it reads as data: an
   email cannot instruct it.
3. Skips anything already on your list or waiting, anything you rejected, and
   notifications that ask nothing of you, and keeps to a few good suggestions,
   each with an importance, a type, an optional finish-by time, and a
   one-sentence reason.
4. Saves each with `rmd suggest add`, withdraws its earlier suggestions that
   no longer apply with `rmd suggest rm`, and tells you what it suggested.

The agent never adds tasks itself. Suggestions wait on the task board, dashed
and labeled with who suggested them and why, until you click **Add to my
tasks** (optionally after **Adjust**) or **Reject**. See
[Suggestions from your AI agent](browser.md#suggestions-from-your-ai-agent).

#### Run it as a routine

Have your agent run the skill on a schedule, for example every weekday morning,
and new suggestions are waiting when you open the board. Use your agent app's
**local** scheduling feature (a scheduled task or automation) with a prompt
such as:

> Use the suggest-tasks skill: look through my email and calendar since your
> last run and suggest tasks for my Schedule Everything board.

The routine must run on the computer where Schedule Everything is installed,
with access to `rmd` and to the apps you want it to read. A cloud-hosted
routine cannot reach your local task files. Suggesting the same task twice
only updates the waiting suggestion, and at most 20 can wait at once, so a
routine cannot flood your board.

## Example prompts

| Say | What happens |
| --- | --- |
| "Build my weekly schedule" | `build-schedule` interviews you, confirms a summary, then writes and validates your routine. |
| "Plan my focus blocks for today" | `plan-day` fills today's `pomodoro` and `potato` blocks with tasks and saves the plan. |
| "Move my workouts to the evenings" | `build-schedule` makes a new `user_config_<n>` version with the change and switches to it. |
| "Suggest tasks from my email and calendar" | `suggest-tasks` leaves a few suggestions on your task board for you to add or reject. |

## What the agent runs

The plugin only uses documented `rmd` commands, so you can run every step
yourself:

| Command | Purpose |
| --- | --- |
| [`rmd check [--dir DIR] [--json]`](cli/schedule-management.md#check) | Validate a config directory and report the config root, versions, and file paths |
| [`rmd sync --json`](cli/schedule-management.md#sync) | Print today's focus blocks, events, open tasks, deadlines, and current plan |
| [`rmd sync --apply FILE`](cli/schedule-management.md#sync) | Save today's plan from a JSON file (`-` reads standard input) |
| [`rmd sync --clear`](cli/schedule-management.md#sync) | Remove today's plan |
| [`rmd suggest ls --json`](cli/task-management.md#suggest) | Print task types, open tasks, waiting suggestions, and your recent decisions |
| [`rmd suggest add TASK PRIORITY ...`](cli/task-management.md#suggest) | Suggest a task for you to review on the board |
| [`rmd suggest rm ID...`](cli/task-management.md#suggest) | Withdraw a suggestion that no longer applies |
| [`rmd switch <n>`](cli/schedule-management.md#switch) | Activate a config version and reload reminders |
| [`rmd update`](cli/schedule-management.md#update) | Reload reminders after writing files |
| [`rmd status`](cli/schedule-management.md#status) | Show what is active now and next |

The skills only write inside the config root that `rmd check` reports and
never delete existing `user_config_<n>` versions. The only task data they
write are suggestions (`tasks/suggestions.json`, through `rmd suggest`); your
task list changes only when you accept one.

## Privacy

:::info[Your files stay local]
Schedule Everything makes no AI calls and has no server. Your agent reads and
writes your local schedule files and sends what it reads to **its own model
provider**, under that agent's terms. Without an agent, everything else (the
CLI, reminders, and the browser workspace) works the same.
:::

## Plugin layout

```text
plugins/schedule-everything/
├── .claude-plugin/plugin.json       # Claude Code manifest
├── .codex-plugin/plugin.json        # Codex manifest
├── .antigravity-plugin/plugin.json  # Antigravity manifest
├── assets/logo.svg
└── skills/
    ├── build-schedule/SKILL.md (+ references/config-format.md)
    ├── plan-day/SKILL.md
    └── suggest-tasks/SKILL.md
```

The marketplace catalogs are at the repository root:
`.claude-plugin/marketplace.json` (Claude Code) and
`.agents/plugins/marketplace.json` (Codex).
