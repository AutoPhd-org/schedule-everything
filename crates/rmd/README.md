# rmd — Schedule Everything

`rmd` is the command-line app, background reminder service, and local browser
workspace of [Schedule Everything](https://github.com/AutoPhd-org/schedule-everything):
a local-first weekly routine, task list, and focus-block reminder app that AI
coding agents (Claude Code, Codex, Antigravity) plan for you.

- `rmd status`, `rmd ls`, `rmd add "task" 7`, `rmd ddl add hw 12.15`, `rmd track`
- `rmd sync --json` / `rmd sync --apply plan.json` — today's focus-block plan, for agents
- `rmd check --json` — validate the configuration
- `rmd web` — the browser workspace; `rmd serve` runs it in the foreground under LaunchX, and `rmd state` prints its state as JSON
- `rmd daemon` — the reminder service (started by a LaunchAgent or systemd unit)

Your routine is TOML and your tasks and history are JSON under
`~/.schedule_management/config`. See the
[documentation](https://autophd-org.github.io/schedule-everything/) for setup.
