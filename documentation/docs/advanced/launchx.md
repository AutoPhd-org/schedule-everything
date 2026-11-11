---
title: LaunchX
description: Run the browser workspace under LaunchX, the launcher for local services, and read or drive it over HTTP with its state document and command API.
---

# LaunchX

[LaunchX](#what-launchx-is) is a launcher for self-hosted services that run on
your computer. The browser workspace is a LaunchX service at the highest level
of its contract (level 2, "open"): LaunchX starts and stops it, shows its health
next to your other services, puts the page behind one sign-in at
`http://rmd.localhost:7600/`, and lets scripts, agents and other services read
its state and change it over HTTP.

Nothing changes when LaunchX is not installed: `rmd web` works as described in
[Browser workspace](../browser.md).

## What LaunchX is

[LaunchX](https://github.com/AutoPhd-org/launchx) is one program, `lx`. Its daemon starts each registered service with an
assigned port and a secret token, checks its health, restarts it if it fails,
and runs a gateway on `127.0.0.1:7600` that signs your browser in once and
passes each request on with the service's token. Services never handle browser
sign-in themselves.

## Set up

Register this repository's manifest, `launchx.toml`, then open the workspace:

```bash
lx add /path/to/schedule-everything
lx open rmd
```

`lx open rmd` starts the workspace if it is not running and opens a signed-in
tab. While LaunchX runs, a plain `rmd web` does the same thing: it hands over to
`lx open rmd`. With an option (`--port`, `--no-open`, `--if-running`,
`--phone`, `--no-phone`), `rmd web` still manages its own server, because those
options are about that server.

The manifest in the repository runs the development build of the checkout
(`target/debug/rmd`, built with `cargo build -p rmd`). For an installed copy,
use `run = ["rmd", "serve"]` and `cli = "rmd"`.

`lx check rmd` starts a test copy on a spare port and verifies it against the
contract.

The reminder service (`rmd daemon`) is not part of the manifest yet. It keeps
running under its own LaunchAgent or systemd unit.

## How the workspace runs under LaunchX

LaunchX runs `rmd serve`. It finds the LaunchX settings in its environment:

| Variable | Use |
|---|---|
| `LAUNCHX` | Present: run in managed mode. |
| `LAUNCHX_SERVICE` | The service id the health report gives (`rmd`). |
| `LAUNCHX_HOST`, `LAUNCHX_PORT` | Where to listen. |
| `LAUNCHX_TOKEN` | The bearer token every request must carry. |

In managed mode the server:

- Requires `Authorization: Bearer <token>` on every request, to every path,
  including the page itself. Anything else gets `401`. The gateway adds the
  header, so the page never sees the token.
- Skips everything the gateway does for it: the Host and Origin checks, the
  sign-in cookie, and phone access. Phone access stays a feature of `rmd web`
  on its own.
- Does not write `.browser.json`, detach, or replace itself after an update.
  Restart it with `lx restart rmd` after rebuilding.
- Logs one line to standard output when it starts, never the token, and exits
  with code 0 on SIGTERM.

The page's **Stop** button stops the service; LaunchX then shows it as stopped
and offers to start it again.

Without `LAUNCHX`, `rmd serve` runs the workspace in the foreground on
`127.0.0.1` (`--port` picks the port) and prints a private link. `rmd web` does
not know about such a server.

## HTTP interface

Programs reach the service directly, not through the gateway. LaunchX records
the address and token of every running service in
`~/.launchx/run/<id>.json`:

```json
{"id": "rmd", "pid": 4242, "url": "http://127.0.0.1:53001", "token": "…", "ui": "http://rmd.localhost:7600/"}
```

Send the token as `Authorization: Bearer <token>` with every request.

| Request | Returns |
|---|---|
| `GET /api/health` | The health report. |
| `GET /api/state` | The state document. |
| `POST /api` | Runs one command: the action API. |
| `GET /events` | The page's own event stream (`ready`, `activate`, a `heartbeat` every 2 seconds). It does not announce changes. |
| `GET /downloads/<key>` | A PDF made by the `export_pdf` command. |
| `GET /`, `GET /assets/…` | The page. |

### Health

```json
{"id": "rmd", "version": "0.10.0", "status": "ok", "summary": "11 open tasks · now: Deep work at 09:00"}
```

`status` is `degraded`, with a `detail` line, while the schedule configuration
does not load. The workspace still works then; it is where you repair the
configuration. `summary` counts open tasks and suggestions waiting for review,
and names the current or next event.

### State document

`GET /api/state` returns everything the page shows when it loads, computed from
your files on every request, so a change made with the CLI shows up at once. It
contains no tokens. It carries an `ETag`; send it back as `If-None-Match` to get
`304 Not Modified` while nothing changed. `rmd state` prints the same document.

| Key | Contents |
|---|---|
| `workspace` | Config sets (`configIds`, `activeId`), `rootDir`, `mode` (`j` or `p`), `language`. |
| `config` | Paths of the active configuration files, and `error` when they do not load. |
| `today` | `date`, `weekday`, week `parity`. |
| `schedule` | Today's `events`, the `current` and `next` event, `timeToNext`, whether today is skipped, whether a plan from `rmd sync` applies. |
| `tasks` | Open tasks, highest priority first. |
| `deadlines` | Deadlines with their status. |
| `habits` | Habits and whether each is done today. |
| `taskTypes`, `taskTypeColors` | Task types by id, and their colours. |
| `history` | The five latest finished tasks. |
| `timedDone` | Timed tasks finished today. |
| `suggestions` | Tasks suggested by agents, waiting for review. |
| `plugins` | Plugins with their settings, state and log. |
| `views` | The commands of `POST /api` behind the views the document leaves out, because they take a parameter or are large: `daily_overview`, `weekly_overview`, `task_history`, `conference_calendar`, `config_read`. |

Within a major version, fields are only added.

### Action API

Every change the page makes is one `POST /api` request; there are no other
endpoints that change data. The body is a command and its payload:

```json
{"command": "task_add", "payload": {"description": "Prepare talk", "priority": 8, "type": "3"}}
```

The answer is always status `200` with `{"ok": true, "data": …}`, or
`{"ok": false, "error": {"code", "message", "details"}}` for a command that
failed. An unknown command fails with the code `unknown_command`.

| Area | Commands |
|---|---|
| Read | `status_snapshot`, `workspace_info`, `daily_overview` (`date`), `weekly_overview` (`date`), `task_history` (`count` or `all`), `conference_calendar` (`year`), `config_read` (`file`), `settings_get_task_types` |
| Tasks | `task_add` (`description`, `priority`, `type`, `postpone`, `due`), `task_update` (`originalDescription` and the fields to change), `task_progress` (`description`, `progress`), `task_subtasks` (`description`, `subtasks`), `task_delete` (`description`, `action`: `deleted`, `cancelled` or `dropped`), `task_restore` |
| Task types | `task_type_save` (`name`, `id`), `task_type_delete` (`id`, `moveTo`), `settings_set_task_types` (`taskTypes`) |
| Deadlines | `deadline_add` (`event`, `date`, `type`, `color`, `repeat`), `deadline_update` (`originalEvent` and the fields to change), `deadline_delete` (`event`) |
| Habits | `habit_mark` (`habitIds`) |
| Suggestions | `suggestion_accept`, `suggestion_reject`, `suggestion_undo` (`id`) |
| Plugins | `plugin_save`, `plugin_delete`, `plugin_restore`, `plugin_state` (`set`), `plugin_log` (`kind`, `data`), `plugin_log_delete` |
| Plan | `sync_clear` |
| Configuration | `config_save` (`file`, `content`, `activeId`, `revision`), `language_set` (`language`), `service_action` (`action`: `update`, `stop`, `switch` with `configId`, or `mode` with `mode`) |
| Export | `export_pdf` (`kind`, `date`): answers with a `/downloads/…` link |
| Server | `server_shutdown`: stops the service |

The recorded cases in `tests/golden` show a request and the exact answer for
each command. `rmd __api '<request>'` runs one request from the command line
without a server.

## From the command line

The CLI and the workspace work on the same files, so they never disagree:

| Command | Does |
|---|---|
| `rmd serve` | Runs the workspace server in the foreground; LaunchX runs this. |
| `rmd state` | Prints the state document. |
| `rmd --version` | Prints the version the health report gives. |
| `rmd status`, `rmd ls`, `rmd ddl`, `rmd sync --json`, `rmd suggest ls --json`, `rmd check --json` | Read parts of the state, as text or JSON. |
