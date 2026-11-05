---
sidebar_position: 4
---

# Browser workspace

Open Schedule Everything in your default browser:

```bash
rmd web
```

The server runs on your computer and reads the same local files as `rmd`.
Closing the launching terminal does not stop the server. After launch, you can
manage your workspace without returning to the terminal. A fresh configuration
opens a setup prompt; missing or malformed schedule files can be repaired in the
browser.

## What you can do

| Area | Operations |
| --- | --- |
| Overview | **J mode:** see overdue and due-today counts, available high-importance tasks, habit progress, the current/next schedule blocks, nearest deadlines, and today's timeline. **P mode:** only the tasks-by-type board. In both modes, search all tasks grouped by type; add, edit, postpone, complete, cancel, or drop tasks |
| Schedule | View today's timeline; generate, revise, discard, and accept sync proposals; build or modify a weekly schedule through a conversation; download the schedule PDF |
| Routines & deadlines | Record today's habits; plan deadlines on an interactive year calendar (click a day to add, choose a marker color, drag to reschedule); add, edit, and remove deadlines with shared task types and urgency labels |
| Activity | Browse a gallery of completed, cancelled, and dropped tasks with filters, search, and a daily rhythm grid; open any task for its full story; open the weekly overview to compare a week with earlier weeks |
| Settings | Edit shared task/deadline types, TOML schedules, habits, profile, and optional model settings; switch schedule versions and modes; reload or stop reminders |

## J mode and P mode

The **J / P** toggle in the header (or `rmd mode j|p`) switches modes for the
reminder service and the browser together. J mode is the structured day: the
overview shows status counts, the current and next schedule blocks, upcoming
deadlines, the task board, and today's timeline. P mode is flexible and
task-first: the overview keeps only the tasks-by-type board, laid out as a loose
collage, and the workspace uses a warmer palette. Other pages are the same in
both modes.

## Year calendar

**Routines & deadlines** opens with a calendar for the current year. Days with
deadlines are tinted with the deadline's marker color; a day with several
deadlines shows a dot for each. Use the arrows to change year and **Today** to
return.

- Click a day to open its card. Name a deadline, pick a color and type, and
  choose **Add**. Past days are read-only.
- In a day's card, click a color swatch beside a deadline to recolor it (click
  the active color again to go back to the type color), or edit or remove it.
- Drag a chip from **Upcoming**, or a day that holds a single deadline, onto
  another day to move the deadline. Its color and type are kept.
- Click a color in the legend to highlight deadlines with that color.

Marker colors are stored as a `color` field (`blue`, `orange`, `aqua`, `yellow`,
`magenta`, `green`, `violet`, or `red`) in `ddl.json`. Deadlines without a color
use their type's color. The add/edit dialog also has a color picker; **Auto**
clears the color.

## Activity gallery and weekly overview

The gallery shows every finished task as a card with generated artwork in its
type's color. More important completed tasks get larger cards; cancelled and
dropped tasks get compact cards. Filter by outcome or type, search by name, sort
by date, importance, or time to finish, or click a square in the rhythm grid to
see one day. Open a card for when it was added and finished, how long it took,
and how that compares with your other completed tasks. Use the arrows or
arrow keys to move between cards.

The weekly overview replaces the weekly and monthly PDF reports in the browser.
Open it from the **Weekly overview** card and use the arrows to move between
weeks. It compares the week with the previous week and with the average of up
to four earlier weeks that have recorded activity:

- tasks completed, with an eight-week trend
- habit consistency (this week and last week as nested rings) and a habit
  check-in grid
- a streak of consecutive days with a completed task
- completed tasks per day by type, with last week shown as outlines
- completed tasks per type against your usual amount
- how tasks left your list (completed, cancelled, dropped) and tasks added vs.
  closed
- median time from adding a task to completing it, and the average importance
  of completed tasks
- the most important tasks you finished

Days follow your computer's local time zone. The CLI still generates weekly and
monthly PDF reports.

## Reading your day

In J mode, the overview puts overdue deadlines, deadlines due today, available tasks with
importance 8–10, and habit completion at the top. Postponed tasks do not count as
available until their reminder date. Red identifies overdue or invalid deadlines,
amber identifies deadlines due today or within three days, and the accent color highlights
high-importance tasks. Each status also has a text label. The current and next
schedule blocks appear beside the three nearest deadlines; open **Routines &
deadlines** to see every deadline.

Deadlines can use any type configured under **Settings → Task & deadline
types**. Choose the type in the add/edit dialog; its name appears beside the
deadline date in both views. Existing deadlines remain **Uncategorized** until
you assign a type. Editing just the event or date preserves its category.

Use the sun (**Light**), moon (**Dark**), or monitor (**System**) buttons in the
header to change appearance on any page. System is the default and follows
changes to your operating system's appearance. An explicit choice persists in
this browser, and switching themes preserves any form you are editing.

Tasks preserve their importance (1–10), type, postponed reminder date, and
procrastination status. The board groups the same tasks returned by `rmd ls`
without imposing a display limit. Inside each group, the existing ordering is
preserved: procrastinated tasks, available tasks, then postponed tasks, with
importance descending within each section. The overview refreshes every 30 seconds when you are not editing.
The CLI table and task IDs are
unchanged. Completing a task matches `rmd rm`; cancel and drop remain distinct
from completion in history and reports.

The schedule assistant accepts text or image timetable attachments up to 5 MB.
Files are copied to temporary local storage for the AI request and removed
afterward. Attachments are included in subsequent turns of the same conversation.

AI features use the same pi installation, account authentication, and model
configuration as `rmd setup` and `rmd sync`. Install and authenticate pi before
using those features; manual task, schedule, and settings editing works without
AI. Initial setup presents a summary for explicit confirmation before generating
files, then presents the proposed files for acceptance. Schedule modifications
create a new `user_config_n` version when accepted.

## Reusing an open tab

A second `rmd web` invocation contacts the existing server for the active config
root and signals its open workspace tab instead of opening another one. The tab
requests focus, but browser settings may prevent automatic foregrounding.
Closing the tab leaves the server running; relaunching opens a tab again.
Use **Settings → Close browser service** to shut down the local browser server.
Different config roots have separate servers and workspaces.

```bash
rmd web --port 8765   # choose the port for a new server
rmd web --no-open     # start/reuse the server and print a private launch link
```

The port flag applies only when starting a server. Treat the launch link printed
by `--no-open` as private: its fragment contains the local session token. Normal
launch removes the token from the address bar once the session is established.
The local `.browser.json` registry and `.browser.log` are runtime files; exclude
`.browser*` from version control if you version your config root.

## Local storage and access

The server binds only to `127.0.0.1`. It checks the host, request origin, and
session token before accepting API calls. It does not upload the local database.
AI requests send scheduling context to the provider selected through pi, just
as the CLI does. Browser settings edits are validated before saving and reject
stale revisions. Saving changes reloads the installer-managed reminder service
when available; standalone installs report when that helper is absent.

Native desktop application bundles and desktop widgets have been removed.
Existing local data needs no migration. Previously installed application/widget
copies can be removed normally; they are no longer updated by this project.
The background reminder service and its system notifications remain available.

## Developing the browser interface

```bash
npm ci
npm run build
npm test
uv run pytest -q
uv run rmd web
```

Frontend source is in `web/`; `npm run build` writes assets into
`src/schedule_management/web/static/`. These assets are bundled with the Python
package, so end users do not need Node.js or a frontend development server.
Commit the rebuilt assets with frontend changes. For frontend development, run
`rmd web --port 8765 --no-open`, then `npm run dev`; open the development URL
with the printed `#token=...` fragment. The Vite proxy routes local API requests
to port 8765.
