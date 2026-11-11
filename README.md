<p align="center">
  <img src="documentation/static/img/logo.svg" alt="" width="88">
</p>

<h1 align="center">Schedule Everything</h1>

<p align="center">
  <strong>Your AI agent plans the week. Persistent reminders keep you on it.</strong><br>
  A local-first weekly routine, task list, and focus-block reminder app. One <code>rmd</code> binary gives you the CLI, the background reminder service, and a browser workspace.
</p>

<p align="center">
  <a href="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/rust.yml"><img src="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/rust.yml/badge.svg" alt="Rust"></a>
  <a href="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/browser-tests.yml"><img src="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/browser-tests.yml/badge.svg" alt="Browser tests"></a>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-6250d6.svg" alt="MIT license"></a>
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#the-browser-workspace">Workspace</a> ·
  <a href="#commands">Commands</a> ·
  <a href="https://autophd-org.github.io/schedule-everything/">Documentation</a> ·
  <a href="README_zh.md">中文</a>
</p>

<p align="center">
  <img src="documentation/static/img/hero.webp" alt="The Schedule Everything browser workspace: the J-mode overview, the weekly overview in dark mode, and P mode on a phone" width="100%">
</p>

## Why Schedule Everything

<table>
  <tr>
    <td width="33%" valign="top">
      <h3>🧠 Your agent plans</h3>
      Tell Claude Code, Codex, or Antigravity about your life. The <code>build-schedule</code> skill interviews you and writes your weekly routine; <code>plan-day</code> fills today's focus blocks with your real tasks; <code>suggest-tasks</code> finds work in your email and calendar and leaves it on your board for you to accept or reject.
    </td>
    <td width="33%" valign="top">
      <h3>⏰ Reminders that stick</h3>
      A background service sounds an alarm and shows a dialog at the start and end of every block until you respond. Deadlines, habits, and urgent tasks get their own nudges.
    </td>
    <td width="33%" valign="top">
      <h3>🔒 Local and plain text</h3>
      Your routine is TOML and your tasks and history are JSON, all on your computer. There is no account and no cloud service, and the app makes no AI calls itself: your agent talks to its own model provider.
    </td>
  </tr>
</table>

<details>
<summary><strong>Healthy defaults, not just a full calendar</strong></summary>

When your preferences leave gaps, `build-schedule` protects a few
evidence-informed defaults: at least 7 hours of sleep opportunity with regular
sleep and wake times, 150–300 minutes of moderate activity a week with
strength work on two or more days, short movement breaks in long sedentary
stretches, demanding work in your best-focus window, and daylight early in the
day. These are scheduling heuristics, not medical advice; your real
constraints always win.

- Watson NF et al. *Recommended Amount of Sleep for a Healthy Adult.* [AASM consensus](https://aasm.org/resources/pdf/pressroom/adult-sleep-duration-consensus.pdf)
- Sletten TL et al. *The importance of sleep regularity.* [Sleep Health, 2023](https://doi.org/10.1016/j.sleh.2023.07.016)
- World Health Organization. *Physical activity recommendations for adults.* [WHO](https://www.who.int/initiatives/behealthy/physical-activity)
- Albulescu P et al. *"Give me a break!"* [PLOS ONE, 2022](https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0272460)
- Figueiro MG et al. *The impact of daytime light exposures on sleep and mood in office workers.* [Sleep Health, 2017](https://doi.org/10.1016/j.sleh.2017.03.005)

</details>

## How it works

<p align="center">
  <img src="documentation/static/img/diagrams/how-it-works.svg" alt="You ask your AI coding agent; the Schedule Everything plugin runs rmd commands; rmd validates and writes local files that drive reminders and the browser workspace" width="100%">
</p>

The plugin's skills do the thinking, and `rmd` does the deterministic work
they need: `rmd check` validates your configuration and reports its paths,
`rmd sync --json` describes today's focus blocks and open tasks, and
`rmd sync --apply` saves today's plan. The reminder service (`rmd daemon`,
started at login) turns your files into alarms, and `rmd web` shows
everything in the browser.

<p align="center">
  <img src="documentation/static/img/diagrams/daily-loop.svg" alt="Build your routine once, then add tasks, plan today, focus with reminders, and look back" width="100%">
</p>

## Quick start

### 1. Install Schedule Everything

The installer builds `rmd` from source, so you need
[Rust](https://rustup.rs) first (`cargo` on your `PATH`). Then:

```bash
curl -fsSL https://raw.githubusercontent.com/AutoPhd-org/schedule-everything/main/install.sh | bash
```

Or from a clone:

```bash
git clone https://github.com/AutoPhd-org/schedule-everything.git
cd schedule-everything
./install.sh
```

The installer:

- builds `rmd` with `cargo` (from your clone, or from a copy of the
  repository it downloads) and puts it in `~/SCHEDULE_MANAGEMENT`. The first
  build takes a few minutes;
- creates your config root in `~/.schedule_management/config` with a first
  `user_config_0`, and asks for any required settings that are missing;
- registers the reminder service (a LaunchAgent on macOS, a systemd user
  service on Linux) and restarts it on the new binary when you upgrade.

Existing configuration is never overwritten. To update, run the installer
again.

### 2. Add the plugin to your AI agent

<table>
  <tr><th>Claude Code</th><th>OpenAI Codex</th><th>Google Antigravity</th></tr>
  <tr>
    <td valign="top"><pre lang="bash">claude plugin marketplace add AutoPhd-org/schedule-everything
claude plugin install schedule-everything@schedule-everything</pre></td>
    <td valign="top"><pre lang="bash">codex plugin marketplace add AutoPhd-org/schedule-everything
codex plugin add schedule-everything@schedule-everything</pre></td>
    <td valign="top"><pre lang="bash">git clone https://github.com/AutoPhd-org/schedule-everything.git
agy plugin install ./schedule-everything/plugins/schedule-everything</pre></td>
  </tr>
</table>

Inside a Claude Code session you can run the same steps as
`/plugin marketplace add` and `/plugin install`. The installer prints these
commands too, and the workspace's **Schedule** page has copy buttons for them.

### 3. Ask your agent

| Say | What happens |
| --- | --- |
| **"Build my weekly schedule"** | `build-schedule` asks about your work, constraints, energy, and habits, shows a plain-language summary, and only after you confirm writes `settings.toml`, `odd_weeks.toml`, `even_weeks.toml`, `habits.toml`, and `profile.md`. It validates them with `rmd check` and reloads reminders. |
| **"Plan my focus blocks for today"** | `plan-day` reads `rmd sync --json`, matches your most important tasks and nearest deadlines to today's pomodoro and potato blocks, confirms with you, and saves the plan with `rmd sync --apply`. |
| **"Move my workouts to the evenings"** | Changes become a new `user_config_<n>` version, validated with `rmd check --dir` and activated with `rmd switch <n>`. Earlier versions stay for rollback. |
| **"Suggest tasks from my email and calendar"** | `suggest-tasks` reads `rmd suggest ls --json`, looks through your connected apps for work you have not captured, and saves a few suggestions with `rmd suggest add`. They wait on the task board, marked as suggestions, until you add or reject each one. Run it on a schedule and new suggestions keep arriving. |

### 4. Open the workspace

```bash
rmd web
```

The workspace listens on `127.0.0.1` only and accepts API calls only with the
session token of the tab it opens. It reads and writes the same files as the
CLI, and running `rmd web` again reuses the open tab. To use it from your phone
as well, see [On your phone, too](#on-your-phone-too).

If you run your local services with [LaunchX](https://github.com/AutoPhd-org/launchx), register this repository with
`lx add` and the workspace becomes one of them: `rmd web` then hands over to
`lx open rmd`, and scripts and agents can read and change it over HTTP. See
[LaunchX](https://autophd-org.github.io/schedule-everything/docs/advanced/launchx).

## The browser workspace

Five pages: **Overview**, **Schedule**, **Routines & deadlines**,
**Activity**, and **Settings**.

### Overview: J mode and P mode

<p align="center">
  <img src="documentation/static/img/modes.webp" alt="J mode shows status, right now, deadlines, tasks, and the timeline; P mode shows only the tasks-by-type board" width="100%">
</p>

**J mode** is the structured day: overdue, due-today, high-importance, and
habit counts, the current and next block, upcoming deadlines, and today's
timeline. **P mode** is for flexible days: alarms for selected events pause
and the page keeps only your tasks. Flip the **J / P** switch in the header or
run `rmd mode j|p`.

- **Task board.** One group per task type. The **compact** layout puts groups
  with more important work first; **spread** places them by workload, the
  heaviest in the middle. Groups keep their places while you work. Add a task
  inside its group, give it a finish-by time, and track it with a progress
  slider or a checklist of subtasks. **Fold** turns each group into a summary
  of its importance, progress, and size. Task types are added, renamed, and
  deleted right on the board.
- **Suggestions from your agent.** Tasks an agent suggests with
  `rmd suggest add` wait in their type's group, dashed and labeled with who
  suggested them and why. They are not tasks yet: no reminder, count, or
  statistic includes them. **Add to my tasks** makes one a normal task,
  **Adjust** lets you change it first, and **Reject** turns it down so the
  agent does not suggest it again. Both can be undone.
- **Timers.** Dials above the board count down to timed tasks due by tonight
  (overdue ones count up) and to deadlines in their last 24 hours. One bar
  shows the progress of today's timed work, optionally weighted by importance.
- **Daily overview.** What you finished today and when, compared with
  yesterday and your usual day, plus your streak, habits, and the open work
  left.
- **Workload.** Every open task counts its importance out of 10 times the
  share still left (an importance-8 task counts 0.8 untouched and 0.4 half
  done). The theme's colors follow the total, from *All clear* through
  *Light*, *Steady*, and *Busy* to *Heavy*. Settings sets where the levels
  start, or turns the workload theme off.

### Schedule: plan with your agent

<p align="center">
  <img src="documentation/static/img/screens/schedule-agents.webp" alt="The Schedule page with today's timeline, today's plan, and tabbed plugin install commands for Claude Code, Codex, and Antigravity" width="100%">
</p>

Today's timeline shows each block with its planned task. The page also has the
plugin install commands with copy buttons, example prompts, and a button that
clears today's plan.

### Routines & deadlines: your year at a glance

<p align="center">
  <img src="documentation/static/img/screens/calendar.webp" alt="An interactive year calendar with colored deadline days, an open day card for November 21, and draggable upcoming deadline chips" width="82%">
</p>

Every deadline sits on a year calendar. Click a day to add one, make it repeat
weekly, every two weeks, or monthly, and pick one of eight marker colors.
Recolor or edit it from the day card, or drag its chip onto another day to
reschedule. Repeating deadlines show every occurrence and roll forward on
their own. Turn on **AI conferences** to add submission deadlines, rebuttal
periods, and conference dates for NeurIPS, ICLR, ICML, CVPR, ACL, ICRA, and
other major venues, in your time zone. Today's habits live on the same page.

### Activity: little wins add up

<p align="center">
  <img src="documentation/static/img/screens/activity-gallery.webp" alt="The Activity page with the weekly overview card, a rhythm grid of completed tasks per day, filters, and a gallery of finished tasks with generated artwork" width="100%">
</p>

Everything you finished, cancelled, or let go grows along a scrollable
timeline tree, one lane and color per task type. Prefer cards? The gallery
gives each task artwork in its type's color. Filter by outcome or type,
search, or click a day in the rhythm grid. Hover over any task to see when it
started, how long it took, and how that compares with your other tasks.

<table>
  <tr>
    <td width="50%" valign="top"><img src="documentation/static/img/screens/lightbox-dark.webp" alt="A task's detail card in dark mode, showing when it was added and finished and how its time compares with other tasks"></td>
    <td width="50%" valign="top"><img src="documentation/static/img/screens/weekly-dark.webp" alt="The weekly overview in dark mode: tasks completed with an eight-week trend, habit rings, streak, daily rhythm, effort by type, outcomes, habit check-ins, pace, and highlights"></td>
  </tr>
  <tr>
    <td valign="top"><strong>Every task has a story.</strong> Click a task for its timeline and pace.</td>
    <td valign="top"><strong>Your week in color.</strong> The weekly overview compares this week with last week and your recent average.</td>
  </tr>
</table>

### Plugins: fitness, guitar, Spanish, anything you track

A plugin is a task category with its own page and tracking. Its tasks are
ordinary tasks of its type, so they show on the board and in Activity, and you
choose in Settings whether it gets its own page, its own metrics in Activity,
a card in the timer row, and whether your AI agent may read it to suggest tasks.
The built-in **Fitness** plugin has a body figure that lights up the muscles
you trained this week, a workout builder with adjustable units (sets, reps,
weight, rest), a live session with a rest timer, personal records, and body
weight. Build your own trackers for practice time, new words, pages read, or
anything else, each with weekly goals. Activity can be viewed by category, so
your fitness history and metrics stand on their own.

### Settings

Plugins; board, workload, and progress preferences; reminder preferences; task and
deadline types; and an editor for your configuration files that checks every
save with the same rules as `rmd check` and refuses to overwrite newer edits.

### On your phone, too

<table>
  <tr>
    <td width="33%"><img src="documentation/static/img/screens/mobile-overview.webp" alt="The J-mode overview on a phone"></td>
    <td width="33%"><img src="documentation/static/img/screens/mobile-activity.webp" alt="The Activity page on a phone"></td>
    <td width="33%"><img src="documentation/static/img/screens/mobile-overview-p-dark.webp" alt="P mode in dark theme on a phone"></td>
  </tr>
</table>

Run `rmd web --phone` and scan the code it prints with your phone's camera. The
phone then uses the workspace on your computer, so it works on the same tasks,
plan, and history while you keep working at the computer. Nothing is copied or
synced, and the phone stays signed in.

```bash
rmd web --phone       # turn phone access on and show the pairing code
rmd web --no-phone    # turn it off and sign every phone out
```

The phone needs to be on the same Wi-Fi as the computer, or on
[Tailscale](https://tailscale.com) from anywhere, and the computer needs to be
awake. With phone access on, the server also listens on your network and
accepts requests addressed to the computer's IP address that carry the phone's
token. The link travels unencrypted over your local network, so use phone access
on networks you trust or over Tailscale. The reminder service starts the
workspace at login while phone access is on. Alarms still sound on the computer.

Light, dark, or system theme, remembered per browser. Motion follows your
system's reduced-motion setting.

## How today's schedule is built

<p align="center">
  <img src="documentation/static/img/diagrams/todays-schedule.svg" alt="The odd or even week file is chosen by ISO week, merged with common entries, then today's plan adds task titles to focus blocks" width="100%">
</p>

```toml
# odd_weeks.toml (even_weeks.toml has the same shape)
[wednesday]
"09:00" = "pomodoro"                                  # a [time_blocks] name: alarm at start and end
"14:00" = { block = "meeting", title = "Advisor 1:1" }

[common]                                              # every day; a weekday entry at the same time wins
"12:30" = "lunch_time"                                # a [time_points] name: one reminder
"20:00" = "potato"
```

Untitled `pomodoro` and `potato` blocks are the focus slots a daily plan can
fill. A plan changes only today; your weekly files never change.

## Commands

| Command | What it does |
| --- | --- |
| `rmd web [--port N] [--no-open] [--phone \| --no-phone]` | Open the browser workspace, reusing a running server and its tab; `--phone` lets your phone use it too |
| `rmd serve [--port N]` · `rmd state` | Run the workspace server in the foreground (LaunchX runs this), or print everything the workspace shows as JSON |
| `rmd status [-v]` | What is happening now and next, with today's planned task titles |
| `rmd add "task" 8 [type] [days]` | Add a task with importance 1–10, an optional type ID, and optionally postpone its urgent alarms by some days |
| `rmd ls [-t TYPE]` | List tasks by importance, optionally of one type |
| `rmd rm` · `rmd cancel` · `rmd drop` | Remove tasks by description or ID: finished (counts as done), added by mistake, or given up |
| `rmd history [n]` | Recent finished, cancelled, and dropped tasks |
| `rmd ddl` · `rmd ddl add <event> <M.D> [--repeat weekly\|biweekly\|monthly]` · `rmd ddl rm <event>…` | List, add, and remove deadlines, including repeating ones |
| `rmd track [id…]` | Record today's habits (asks about each one when no IDs are given) |
| `rmd sync [--json \| --apply FILE \| --clear]` | Show, save, or clear today's plan; `--json` and `--apply` are what `plan-day` uses |
| `rmd suggest [ls [--json]]` · `rmd suggest add "task" 8 [--type T] [--reason …]` · `rmd suggest rm <id>…` | List, add, or withdraw tasks suggested by an AI agent; you accept or reject them on the task board |
| `rmd check [--dir DIR] [--json]` | Validate a config directory and report its paths |
| `rmd mode [j\|p]` | Show or switch between J and P mode |
| `rmd switch <n>` | Activate `user_config_<n>` and reload reminders |
| `rmd update` · `rmd stop` | Reload reminders (after a `git pull` when the config root is a git checkout), or stop them |
| `rmd settings` · `rmd edit [file]` | Edit `settings.toml` in a terminal UI, or open a config file in your editor |
| `rmd view` · `rmd report weekly\|monthly [-d DATE]` | Export a schedule PDF or a productivity report |
| `rmd completion [bash\|zsh\|fish\|tcsh]` | Print a shell completion script |

## Configuration

```text
~/.schedule_management/config/
├── user_config_0/            # one complete version of your routine
│   ├── settings.toml         # time blocks, time points, reminder times, task types
│   ├── odd_weeks.toml        # schedule for odd ISO weeks
│   ├── even_weeks.toml       # schedule for even ISO weeks
│   ├── habits.toml           # daily habits
│   ├── ddl.json              # deadlines (with optional repeats and marker colors)
│   └── profile.md            # what your agent knows about you
├── user_config_1/            # a later version made by build-schedule
├── synced_schedule.toml      # today's plan
└── tasks/                    # tasks, history, habit records, mode
```

Edit any file by hand, in the workspace's configuration editor, or with
`rmd settings`, and run `rmd check` after manual edits. `REMINDER_CONFIG_DIR`
points `rmd` at a different config root.

## Documentation

- [Introduction](https://autophd-org.github.io/schedule-everything/docs/intro)
- [AI agent plugins](https://autophd-org.github.io/schedule-everything/docs/ai-agents)
- [Quick start](https://autophd-org.github.io/schedule-everything/docs/quick-start)
- [Installation](https://autophd-org.github.io/schedule-everything/docs/installation)
- [Browser workspace](https://autophd-org.github.io/schedule-everything/docs/browser)
- [Configuration](https://autophd-org.github.io/schedule-everything/docs/configuration/overview)
- [CLI reference](https://autophd-org.github.io/schedule-everything/docs/cli/overview)

## Development

| Path | What it holds |
| --- | --- |
| [`crates/rmd`](crates/rmd) | The `rmd` binary in Rust: CLI, reminder service, local web server, PDF reports |
| [`web`](web) | The browser workspace in TypeScript, bundled by Vite into `crates/rmd/web-static` |
| [`plugins/schedule-everything`](plugins/schedule-everything) | The agent plugin and its `build-schedule`, `plan-day`, and `suggest-tasks` skills |
| [`config`](config) | Templates the installer copies into a new config root |
| [`documentation`](documentation) | The Docusaurus documentation site |
| [`tests/golden`](tests/golden) | Recorded CLI and browser API cases that `cargo test` replays |

```bash
cargo build                     # target/debug/rmd
cargo test                      # unit tests and the golden cases
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings

bun install
bun run test                    # browser tests (Vitest; plain `bun test` is Bun's own runner)
bun run build                   # rebuild crates/rmd/web-static
```

JavaScript dependencies are managed with [Bun](https://bun.sh) (`bun.lock`);
the tools themselves run on Node.js 20 or newer. The built frontend is
compiled into the `rmd` binary, so commit `crates/rmd/web-static` with
frontend changes and rebuild `rmd` to serve them. To work on the frontend
with live reload, run `rmd web --port 8765 --no-open` and `bun run dev`.

The golden cases were recorded from an earlier Python implementation that
`rmd` replaced, and they pin its CLI output, browser API responses, and data
files. See [`tests/golden/README.md`](tests/golden/README.md) and
[`crates/rmd/PORTING.md`](crates/rmd/PORTING.md).

## License

Distributed under the **MIT License**. See [LICENSE](LICENSE).
