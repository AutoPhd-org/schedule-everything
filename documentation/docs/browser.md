---
sidebar_position: 5
description: The local browser workspace opened by rmd web — overview, schedule, year calendar, activity tree and gallery, weekly overview, and settings.
---

# Browser workspace

Open Schedule Everything in your default browser:

```bash
rmd web
```

The server runs on your computer and reads the same local files as `rmd`.
Closing the launching terminal does not stop the server. After launch, you can
manage your workspace without returning to the terminal. When the schedule
files are missing or invalid, a banner offers **Build a schedule with your AI
agent** (which opens the Schedule page) and **Edit configuration**; malformed
files can be repaired in the browser.

<Figure
  src="img/screens/overview-j.webp"
  darkSrc="img/screens/overview-j-dark.webp"
  eager
  alt="The J-mode overview: status orbs for overdue, due today, high importance, and habits; the right-now card; next deadlines; and tasks by type"
  caption="The overview in J mode. Switch the site theme to see the workspace's dark appearance."
/>

## What you can do

| Area | Operations |
| --- | --- |
| Overview | **J mode:** see overdue and due-today counts, available high-importance tasks, habit progress, the current/next schedule blocks, nearest deadlines, and today's timeline. **P mode:** only the tasks-by-type board. In both modes, search all tasks grouped by type; add a task right inside its group or from **New task**; fold the groups into summaries; edit, postpone, complete, cancel, or drop tasks |
| Schedule | View today's timeline; see and clear today's plan; get the agent plugin install commands and example prompts; download the schedule PDF |
| Routines & deadlines | Record today's habits; plan deadlines and repeating events on an interactive year calendar (click a day to add, repeat weekly, every two weeks, or monthly, choose a marker color, drag to reschedule, and optionally show AI conference dates); add, edit, and remove deadlines with shared task types and urgency labels |
| Activity | Scroll a timeline tree of completed, cancelled, and dropped tasks, or switch to a gallery; filter, search, and use the daily rhythm grid; hover over any task for its story; open the weekly overview to compare a week with earlier weeks |
| Settings | Edit shared task/deadline types, settings, odd/even week schedules, habits, deadlines, and profile; switch schedule versions and modes; reload or stop reminders |

## J mode and P mode

The **J / P** toggle in the header (or `rmd mode j|p`) switches modes for the
reminder service and the browser together. J mode is the structured day: the
overview shows status counts, the current and next schedule blocks, upcoming
deadlines, the task board, and today's timeline. P mode is flexible and
task-first: the overview keeps only the tasks-by-type board, laid out as a loose
collage, and the workspace uses a warmer palette. Other pages are the same in
both modes.

<Figure
  src="img/modes.webp"
  alt="J mode shows status, right now, deadlines, tasks, and the timeline; P mode shows only the tasks-by-type board in a darker, warmer palette"
/>

## Year calendar

**Routines & deadlines** opens with a calendar for the current year. Days with
deadlines are tinted with the deadline's marker color; a day with several
deadlines shows a dot for each. Use the arrows to change year and **Today** to
return.

<Figure
  src="img/screens/calendar.webp"
  maxWidth={640}
  alt="The year calendar with colored deadline days, an open day card for November 21 with two deadlines and color swatches, and the Upcoming list of draggable chips"
  caption="Click a day to open its card; drag an Upcoming chip onto another day to move it."
/>

- Click a day to open its card. Name a deadline, pick a color, a type, and how
  it repeats, and choose **Add**. Past days are read-only.
- **Repeat** can be **Weekly**, **Every 2 weeks**, or **Monthly**. The calendar
  shows every occurrence through the end of the year you are viewing: the next
  one in full color, later ones lighter with a dashed ring. **Upcoming**, the
  overview, and reminders use the next occurrence, and once it has passed the
  deadline moves on to the following one instead of being removed. Change or
  stop the repetition in the edit dialog.
- In a day's card, click a color swatch beside a deadline to recolor it (click
  the active color again to go back to the type color), or edit or remove it.
- Drag a chip from **Upcoming**, or a day that holds a single deadline, onto
  another day to move the deadline. Its color, type, and repetition are kept; a
  repeating event continues from the new day.
- Click a color in the legend to highlight deadlines with that color.

Marker colors are stored as a `color` field (`blue`, `orange`, `aqua`, `yellow`,
`magenta`, `green`, `violet`, or `red`) in `ddl.json`. Deadlines without a color
use their type's color. The add/edit dialog also has a color picker; **Auto**
clears the color. Repeating events store `repeat` (`weekly`, `biweekly`, or
`monthly`); monthly ones also store `repeat_day`, so an event on the 31st returns
to the 31st after a shorter month.

### AI conference dates

Turn on **AI conferences** above the calendar to see this year's dates for
NeurIPS, ICLR, ICML, CVPR, ICCV, ECCV, ACL, EMNLP, COLM, AAAI, IJCAI, ICRA,
IROS, RSS, and CoRL alongside your own deadlines. It is off until you turn it on.

- A day with an abstract or paper submission deadline gets a small flag in the
  conference's color. The days a conference takes place get a colored bar along
  the bottom of each day.
- Colors follow the field: violet for machine learning, blue for vision, aqua for
  language, orange for general AI, and magenta for robotics.
- Use the arrow beside the button to choose which conferences to show. **All**
  and **None** select or clear every conference. Your choice is remembered in
  this browser.
- Deadlines are converted to your time zone, so a 23:59 AoE deadline can land on
  the next day. A day's card shows both your local time and the original time,
  links to the conference website, and has a **My deadlines** button that copies
  an upcoming submission deadline into your own deadlines.
- The side panel lists the next conference dates; click one to jump to that day.

The dates come from the community-maintained
[Hugging Face AI Deadlines](https://github.com/huggingface/ai-deadlines) list
and may change after a conference announces them. The workspace downloads them
only after you turn the layer on, and then at most once a week; **Refresh** in
the picker downloads them now. They are cached in `cache/ai_conferences.json`
inside your config root (`~/.schedule_management/config/` by default; about
15 KB), so the calendar works offline with the last copy. Set
`REMINDER_CONFERENCE_CACHE_PATH` to keep the cache somewhere else.

## Activity tree, gallery, and weekly overview

The Activity page opens on a tree: a vertical time axis with your newest task
at the top. Scroll it to travel back in time. Each task is a node on a branch
off the trunk, and each task type keeps its own lane and color on either side.
Bigger dots are more important tasks; hollow dots are cancelled, and dashed
ones were dropped. The trunk takes the color of whichever type dominates the
tasks around it, so long stretches of one kind of work stand out at a glance.

A band across the middle of the tree marks where you are looking. Tasks inside
it light up, and the header names the dates in view and which type leads them.
The map beside the tree shows your whole history with a window over the current
view: press or drag on it to jump. The days in view are outlined on the rhythm
grid, and clicking a day there brings the tree to it. With a task selected,
press the up and down arrow keys to step between tasks. Your choice of tree or
gallery is remembered in this browser.

The gallery shows every finished task as a card with generated artwork in its
type's color. More important completed tasks get larger cards; cancelled and
dropped tasks get compact cards. Filter by outcome or type, search by name, sort
by date, importance, or time to finish, or click a square in the rhythm grid to
see one day. Hover over a card, or over any task in the tree, for when it was
added and finished, how long it took, and how that compares with your other
completed tasks; keyboard focus shows the same details. Click a task to open
its full story, and use the arrows or arrow keys to move between tasks there.

<Figure
  src="img/screens/activity-gallery.webp"
  alt="The Activity page: the weekly overview card, the rhythm grid of completed tasks per day, outcome and type filters, and gallery cards with generated artwork"
/>

<Figure
  src="img/screens/lightbox-dark.webp"
  alt="A task's detail card in dark mode: added and completed times, time to finish, importance 9 of 10, and how its time compares with 75 completed tasks"
  caption="Click any task for its full story."
/>

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

<Figure
  src="img/screens/weekly.webp"
  darkSrc="img/screens/weekly-dark.webp"
  maxWidth={720}
  alt="The weekly overview: tasks completed with an eight-week trend, habit consistency rings, streak, daily rhythm, effort by type, outcomes, habit check-ins, pace, and highlights"
/>

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

Messages that confirm an action stay at the top of the page until you dismiss
them with their **×** button or **Esc**.

The layout adapts to narrow windows down to phone width: the sidebar becomes
an icon rail and cards stack.

<FigureRow columns={3}>
  <Figure src="img/screens/mobile-overview.webp" alt="The J-mode overview at phone width" caption="J-mode overview" />
  <Figure src="img/screens/mobile-overview-p-dark.webp" alt="P mode in the dark theme at phone width" caption="P mode, dark" />
  <Figure src="img/screens/mobile-activity.webp" alt="The Activity page at phone width" caption="Activity" />
</FigureRow>

Tasks preserve their importance (1–10), type, postponed reminder date, and
procrastination status. The board groups the same tasks returned by `rmd ls`
without imposing a display limit, and shows a group for every type in
**Settings → Task & deadline types**, even before it has tasks. Groups with
more important work come first: they are ordered by the average importance of
their tasks, then by how many tasks they hold, and empty groups come last.
Inside each group, the existing ordering is preserved: procrastinated tasks,
available tasks, then postponed tasks, with importance descending within each
section. The overview refreshes every 30 seconds when you are not editing.

To add a task to a group, click **Add a task** at the bottom of the group, type
it, set its importance, and press **Enter**. The form stays open for the next
task; **Esc** closes it. **New task** in the heading still opens the full form
with a postponed start.

**Fold** in the board's heading turns each group into a summary: its average
importance as a ring, every task as a dot on a 1–10 scale, its share of your
open tasks, and how many tasks are important (8 or more), waiting for a later
day, or deferred. **Expand** brings back every task, and the arrow in a group's
header folds or opens just that group. Your choice is remembered in this
browser; a search always lists the matching tasks.
The CLI table and task IDs are
unchanged. Completing a task matches `rmd rm`; cancel and drop remain distinct
from completion in history and reports.

## AI agent plugins

Schedule building and daily planning happen in your own coding agent through
the Schedule Everything plugin for Claude Code, OpenAI Codex, or Google
Antigravity. The **Plan with your AI agent** card on the Schedule page has a tab
for each agent with its install commands (each with a copy button) and example
prompts such as "Build my weekly schedule" and "Plan my focus blocks for
today". See [AI agent plugins](ai-agents.md).

<Figure
  src="img/screens/schedule-agents.webp"
  alt="The Schedule page with today's timeline, the today's plan card, and tabbed plugin install commands for Claude Code, Codex, and Antigravity"
/>

When today has a plan, a **Today's plan** card appears and each planned focus
block in the timeline shows its task. **Clear today's plan** removes it, like
`rmd sync --clear`; your weekly routine files are not changed. Task, schedule,
and settings editing in the browser works without an agent.

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
session token before accepting API calls. It does not upload the local database
and makes no AI requests. If you use an agent plugin, the agent sends what it
reads to its own model provider. Browser settings edits are validated before saving and reject
stale revisions. Saving changes reloads the installer-managed reminder service
when available; standalone installs report when that helper is absent.

Native desktop application bundles and desktop widgets have been removed.
Existing local data needs no migration. Previously installed application/widget
copies can be removed normally; they are no longer updated by this project.
The background reminder service and its system notifications remain available.

## Developing the browser interface

```bash
bun install
bun run build
bun run test
uv run pytest -q
uv run rmd web
```

Use `bun run test`, not `bun test`: the project's tests run on Vitest, while
`bun test` starts Bun's own test runner. Dependencies are locked in `bun.lock`;
the build tools run on Node.js 20 or newer.

Frontend source is in `web/`; `bun run build` writes assets into
`src/schedule_management/web/static/`. These assets are bundled with the Python
package, so end users do not need Node.js or a frontend development server.
Commit the rebuilt assets with frontend changes. For frontend development, run
`rmd web --port 8765 --no-open`, then `bun run dev`; open the development URL
with the printed `#token=...` fragment. The Vite proxy routes local API requests
to port 8765.
