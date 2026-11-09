# Schedule Everything agent plugin

Build your weekly routine, plan each day's focus blocks, and get task
suggestions from your email and calendar with your coding agent. The plugin works in Claude Code, OpenAI Codex, and Google Antigravity,
and drives the local [Schedule Everything](https://github.com/AutoPhd-org/schedule-everything)
`rmd` CLI, so install that first.

| Skill | Use it to |
| --- | --- |
| `build-schedule` | Interview you, agree on a plan, write `settings.toml`, the odd/even week schedules, `habits.toml`, and `profile.md`, then validate with `rmd check` and reload reminders. Changes to an existing schedule become a new `user_config_<n>` version. |
| `plan-day` | Read today's focus blocks and open tasks with `rmd sync --json`, agree on a plan with you, and save it with `rmd sync --apply`. |
| `suggest-tasks` | Look through what it knows about you and your connected apps (email, calendar, ...) for work you have not captured, and save it with `rmd suggest add`. Suggestions wait on the task board, marked as suggestions, until you add or reject each one; the agent never adds tasks itself. Works on request or as a scheduled routine. |

## Install

Claude Code:

```bash
claude plugin marketplace add AutoPhd-org/schedule-everything
claude plugin install schedule-everything@schedule-everything
```

Codex:

```bash
codex plugin marketplace add AutoPhd-org/schedule-everything
codex plugin add schedule-everything@schedule-everything
```

Antigravity (from a clone of the repository):

```bash
git clone https://github.com/AutoPhd-org/schedule-everything.git
agy plugin install ./schedule-everything/plugins/schedule-everything
```

## Try

- "Build my weekly schedule"
- "Plan my focus blocks for today"
- "Move my workouts to the evenings"
- "Suggest tasks from my email and calendar"

Schedule Everything makes no AI calls itself. Your agent reads and writes your
local schedule files and sends what it reads to its own model provider.

## Layout

```
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
