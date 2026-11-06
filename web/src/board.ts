// Tasks-by-type board: one group per configured task type, ordered so groups with more important work come
// first. Each group either lists its tasks or folds into a summary of its metrics, and takes new tasks inline.
// The board is also where task types are managed: a tile at the end adds one, and each group's menu renames
// or deletes its type. Each task has a progress slider, and a timed task shows when it is due (or how long it
// has been overdue). Rendering is pure (state in, HTML out); render.ts owns the board state and binding.
import { ring } from "./charts";
import { dueAt, dueLabel, clockText, isOverdue, progressControl, progressOf } from "./timers";
import type { Snapshot, TaskItem } from "./types";
import { e, empty, hash, icons, sw, typeSwatch } from "./ui";

export type BoardView = "expanded" | "folded";
/** The inline form for a new task: which group it is open in, the chosen importance, and any unsent text. */
export type Composer = {type: string; priority: number; draft: string};
/** A type's tasks and the metrics its folded summary shows: average importance, average progress (0–100),
 *  and how many tasks are important, timed, overdue, or deferred. */
export type TaskGroup = {id: string; name: string; tasks: TaskItem[]; average: number | null; progress: number | null; high: number; timed: number; overdue: number; deferred: number};
export type BoardOptions = {
  snapshot: Snapshot;
  query: string;
  isFolded: (id: string) => boolean;
  composer: Composer | null;
  /** Description of a task that was just added, to greet it. */
  added: string | null;
  /** Group whose inline form just opened, to animate it in. */
  opening?: string | null;
  /** Group whose type menu (rename, delete) is open. */
  menu?: string | null;
  /** Group whose name is being edited. */
  renaming?: string | null;
  /** Group asking to confirm its deletion, and where its tasks and deadlines would go. */
  deleting?: {id: string; moveTo: string} | null;
  /** The form for a new type, when open. */
  newType?: {draft: string} | null;
  /** Group to greet after its type was added or renamed. */
  greet?: string | null;
  /** The time to measure timed tasks against; now by default. */
  now?: number;
};

const byId = (a: string, b: string) => a.localeCompare(b, undefined, {numeric: true});
const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;
/** The name used for a view transition, stable for a group across redraws. */
export const groupTransitionName = (id: string) => `tg-${hash(id).toString(36)}`;

/** One group per configured type, even without tasks, plus any type that only existing tasks still use.
 *  Groups are ordered by average importance (to one decimal, as shown), then by how many tasks they hold;
 *  groups without tasks come last in configured order. Tasks keep the server's order inside each group. */
export function groupTasks(snapshot: Snapshot, now = Date.now()): TaskGroup[] {
  const groups = new Map<string, TaskGroup>();
  const group = (id: string, name: string) => {
    if (!groups.has(id)) groups.set(id, {id, name, tasks: [], average: null, progress: null, high: 0, timed: 0, overdue: 0, deferred: 0});
    return groups.get(id)!;
  };
  for (const id of Object.keys(snapshot.taskTypes).sort(byId)) group(id, snapshot.taskTypes[id]);
  for (const task of snapshot.tasks) group(task.type, snapshot.taskTypes[task.type] ?? task.typeName ?? "Other").tasks.push(task);
  for (const item of groups.values()) {
    if (!item.tasks.length) continue;
    item.average = Math.round(item.tasks.reduce((sum, task) => sum + task.priority, 0) / item.tasks.length * 10) / 10;
    item.progress = Math.round(item.tasks.reduce((sum, task) => sum + progressOf(task), 0) / item.tasks.length);
    item.high = item.tasks.filter(task => task.priority >= 8).length;
    item.timed = item.tasks.filter(task => dueAt(task) !== null).length;
    item.overdue = item.tasks.filter(task => isOverdue(task, now)).length;
    item.deferred = item.tasks.filter(task => task.procrastinated).length;
  }
  return [...groups.values()].sort((a, b) => (b.average ?? -1) - (a.average ?? -1) || b.tasks.length - a.tasks.length || byId(a.id, b.id));
}

export function taskBoard(options: BoardOptions): string {
  const {snapshot, query} = options;
  const needle = query.toLowerCase();
  const all = groupTasks(snapshot, options.now);
  // A search shows only the groups with matches, listing them so the matching tasks can be seen.
  // Types without tasks wait on the shelf below the board (see typeShelf).
  const groups = needle ? all.map(group => ({...group, tasks: group.tasks.filter(task => task.description.toLowerCase().includes(needle))})).filter(group => group.tasks.length) : all.filter(group => group.tasks.length);
  if (!groups.length) return empty(needle ? "No tasks match your search." : "No open tasks. Add one to a type below, or use New task.");
  return groups.map((group, order) => groupCard(group, order, options, !needle && options.isFolded(group.id), !!needle)).join("");
}

/** Types without tasks, kept out of the board's way: each is a small pill that can take its first task (which
 *  moves it up to the board), be renamed, or be deleted. The shelf also adds new types. Hidden while searching. */
export function typeShelf(options: BoardOptions): string {
  if (options.query) return "";
  const waiting = groupTasks(options.snapshot, options.now).filter(group => !group.tasks.length);
  const heading = waiting.length
    ? `<header class="shelf-heading"><h3>Types without tasks <span class="count">${waiting.length}</span></h3><p>Add a task to one and it moves up to the board.</p></header>`
    : "";
  return `<section class="type-shelf ${waiting.length ? "" : "is-bare"}" aria-label="Types without tasks">${heading}<div class="shelf-items">${waiting.map(group => shelfItem(group, options)).join("")}${newTypeTile(options.newType ?? null)}</div></section>`;
}

function shelfItem(group: TaskGroup, options: BoardOptions): string {
  const {snapshot: s} = options;
  const id = e(group.id);
  const name = e(group.name);
  const deleting = options.deleting?.id === group.id ? options.deleting : null;
  const composing = options.composer?.type === group.id ? options.composer : null;
  const title = options.renaming === group.id
    ? `<input class="type-name-input" name="typeName" data-type-name-input="${id}" value="${name}" maxlength="60" autocomplete="off" aria-label="Name of this type" enterkeyhint="done">`
    : `<h4 data-type-title="${id}" title="Double-click to rename">${name}</h4>`;
  const open = !!(deleting || composing);
  const classes = ["shelf-item", sw(typeSwatch(s.taskTypes, group.id)), open ? "is-open" : "", options.greet === group.id ? "greet" : "", options.menu === group.id ? "menu-open" : ""].filter(Boolean).join(" ");
  return `<article class="${classes}" data-group="${id}" style="--vt:${groupTransitionName(group.id)}">
      <div class="shelf-head"><span class="shelf-icon" aria-hidden="true">${icons.tasks}</span>${title}<button type="button" class="shelf-add" data-compose="${id}" aria-label="Add a task to ${name}" title="Add a task">${icons.plus}</button>${typeMenu(group, s, null, options.menu === group.id)}</div>
      ${deleting ? deletePanel(group, s, deleting.moveTo) : composing ? `<div class="compose-slot ${options.opening === group.id ? "opening" : ""}">${composer(group, composing)}</div>` : ""}
    </article>`;
}

function groupCard(group: TaskGroup, order: number, options: BoardOptions, folded: boolean, searching: boolean): string {
  const {snapshot: s} = options;
  const count = group.tasks.length;
  const name = e(group.name);
  const id = e(group.id);
  const greeted = !!options.added && group.tasks.some(task => task.description === options.added);
  const deleting = !searching && options.deleting?.id === group.id ? options.deleting : null;
  const toggle = searching ? "" : `<button type="button" class="group-toggle" data-group-toggle="${id}" aria-expanded="${!folded}" aria-label="${folded ? "Show the tasks in" : "Fold"} ${name}" title="${folded ? "Show tasks" : "Fold into a summary"}">${icons.chevron}</button>`;
  const title = !searching && options.renaming === group.id
    ? `<input class="type-name-input" name="typeName" data-type-name-input="${id}" value="${name}" maxlength="60" autocomplete="off" aria-label="Name of this type" enterkeyhint="done">`
    : `<h3 data-type-title="${id}" ${searching ? "" : 'title="Double-click to rename"'}>${name}</h3>`;
  const body = deleting ? deletePanel(group, s, deleting.moveTo)
    : folded ? groupSummary(group, s.tasks.length)
    : count ? `<div class="task-cards">${group.tasks.map((task, position) => taskCard(task, s.tasks.indexOf(task), position, options.now ?? Date.now(), task.description === options.added)).join("")}</div>`
    : `<p class="group-empty">Nothing here yet.</p>`;
  const classes = ["task-group", sw(typeSwatch(s.taskTypes, group.id)), folded ? "is-folded" : "", count ? "" : "is-empty", greeted && folded ? "just-added" : "", options.greet === group.id ? "greet" : "", deleting ? "is-deleting" : "", options.menu === group.id && !searching ? "menu-open" : ""].filter(Boolean).join(" ");
  return `<article class="${classes}" data-group="${id}" style="--tilt:${(hash(group.id) % 5 - 2) * 0.6}deg; --shape:${hash(group.name) % 4}; --vt:${groupTransitionName(group.id)}; --g:${order}">
    <header class="group-heading"><span class="group-icon">${icons.tasks}</span>${title}${searching ? countBadge(count) : typeMenu(group, s, count, options.menu === group.id)}${toggle}</header>
    ${body}
    ${deleting ? "" : `<div class="compose-slot ${options.opening === group.id ? "opening" : ""}">${composer(group, options.composer?.type === group.id ? options.composer : null)}</div>`}
  </article>`;
}

const countBadge = (count: number) => `<span class="group-count" title="${plural(count, "task")}">${count}</span>`;

/** The task count (when given), which gives way to a menu button on hover or focus: rename or delete the type. */
function typeMenu(group: TaskGroup, s: Snapshot, count: number | null, open: boolean): string {
  const id = e(group.id);
  const name = e(group.name);
  // Deleting needs another type to remain for the tasks and deadlines.
  const lastType = group.id in s.taskTypes && Object.keys(s.taskTypes).length <= 1;
  return `<span class="group-badge ${open ? "is-open" : ""} ${count === null ? "no-count" : ""}">${count === null ? "" : countBadge(count)}<button type="button" class="group-more" data-type-menu="${id}" aria-haspopup="menu" aria-expanded="${open}" aria-label="Options for the type ${name}" title="Rename or delete this type">${icons.more}</button>
    ${open ? `<span class="type-menu" role="menu" aria-label="${name}"><button type="button" role="menuitem" data-type-rename="${id}">${icons.edit}<span>Rename</span></button><button type="button" role="menuitem" class="danger" data-type-delete="${id}" ${lastType ? 'disabled title="Keep at least one task type"' : ""}>${icons.trash}<span>Delete type</span></button></span>` : ""}</span>`;
}

/** Asks before deleting a type, and where its open tasks (required) and deadlines (optional) should go. */
function deletePanel(group: TaskGroup, s: Snapshot, moveTo: string): string {
  const name = e(group.name);
  const tasks = s.tasks.filter(task => task.type === group.id).length;
  const deadlines = s.deadlines.filter(item => item.type === group.id).length;
  const targets = Object.keys(s.taskTypes).filter(id => id !== group.id).sort(byId);
  const choice = targets.includes(moveTo) ? moveTo : tasks ? targets[0] : "";
  const what = [tasks ? plural(tasks, "task") : "", deadlines ? plural(deadlines, "deadline") : ""].filter(Boolean).join(" and ");
  const move = what ? `<label class="type-move"><span>Its ${what} ${tasks + deadlines === 1 ? "moves" : "move"} to</span><select name="moveTo" data-type-move>${tasks ? "" : `<option value="">No type</option>`}${targets.map(id => `<option value="${e(id)}" ${id === choice ? "selected" : ""}>${e(s.taskTypes[id])}</option>`).join("")}</select></label>`
    : `<p class="type-delete-note">No tasks or deadlines use it.</p>`;
  return `<div class="type-delete" role="group" aria-label="Delete the type ${name}">
      <p class="type-delete-title">Delete “${name}”?</p>
      ${move}
      <div class="type-delete-actions"><button type="button" class="button small delete-type" data-type-delete-confirm="${e(group.id)}">${icons.trash}<span>Delete type</span></button><button type="button" class="button small ghost" data-type-delete-cancel>Keep it</button></div>
    </div>`;
}

/** The last item on the shelf: a button that becomes a form for naming a new type. */
function newTypeTile(open: {draft: string} | null): string {
  if (!open) return `<div class="new-type" data-slot="new-type"><button type="button" class="new-type-button" data-type-new><span class="new-type-icon">${icons.plus}</span><span>New type</span></button></div>`;
  return `<div class="new-type is-open" data-slot="new-type"><form class="new-type-form" autocomplete="off">
      <span class="new-type-icon">${icons.tasks}</span>
      <input name="newTypeName" aria-label="Name of the new type" placeholder="Name a new type…" maxlength="60" value="${e(open.draft)}" enterkeyhint="done">
      <div class="new-type-actions"><button type="submit" class="composer-add">${icons.enter}<span>Add type</span></button><button type="button" class="composer-close" data-type-new-cancel aria-label="Close the new type form" title="Close (Esc)">${icons.close}</button></div>
    </form></div>`;
}

function taskCard(task: TaskItem, index: number, position: number, now: number, added: boolean): string {
  const at = dueAt(task);
  const overdue = at !== null && at <= now;
  const progress = progressOf(task);
  const name = e(task.description);
  const level = task.priority >= 8 ? "high" : task.priority >= 5 ? "medium" : "low";
  const classes = ["task-card", task.procrastinated ? "deferred" : "", task.priority >= 8 ? "high-priority" : "", at !== null ? "is-timed" : "", overdue ? "is-overdue" : "", progress === 100 ? "is-full" : "", added ? "just-added" : ""].filter(Boolean).join(" ");
  // A timed task shows when it is due and how long is left, or how long it has been overdue; render.ts ticks it.
  const timer = at !== null
    ? `<p class="task-detail task-timer">${icons.clock}<span><span data-clock="due">${clockText("due", at, now)}</span> ${e(dueLabel(task.due!, now))} · <span data-clock="short">${clockText("short", at, now)}</span> <span data-clock="word">${clockText("word", at, now)}</span></span></p>`
    : "";
  const deferred = task.procrastinated ? `<p class="task-detail">${icons.hourglass}${e(`Deferred${task.procrastinateDays === null ? "" : ` · ${task.procrastinateDays} days`}`)}</p>` : "";
  const timing = at !== null
    ? `<button type="button" class="neutral" data-task="${index}" data-operation="timer">Change the finish time…</button><button type="button" class="neutral" data-task="${index}" data-operation="untime">Stop timing · make it a normal task</button>`
    : `<button type="button" class="neutral" data-task="${index}" data-operation="timer">Make it a timed task…</button>`;
  return `<div class="${classes}" style="--i:${position}" ${at !== null ? `data-at="${at}" data-kind="task"` : ""}><div class="task-main"><button type="button" class="complete-button" data-task="${index}" data-operation="deleted" aria-label="Complete ${name}" title="Complete task">${icons.check}</button><h4>${name}</h4></div><div class="task-meta"><span class="priority p-${level}">${icons.flag} Importance ${task.priority}/10</span><span class="energy" style="--p:${task.priority / 10}" aria-hidden="true"></span><button class="text-button edit" type="button" data-task="${index}" data-operation="edit" aria-label="Edit ${name}">Edit</button></div>${timer}${deferred}${progressControl(index, progress, task.description, "task-progress")}<details class="task-more"><summary>More options</summary>${timing}<button type="button" data-task="${index}" data-operation="cancelled">Cancel · added by mistake</button><button type="button" data-task="${index}" data-operation="dropped">Drop · no longer pursuing</button></details></div>`;
}

/** A folded group: average importance as a ring, every task as a dot on a 1–10 strip, how far along its tasks
 *  are on average, its share of the list, and how many tasks are important, timed, overdue, or deferred. */
function groupSummary(group: TaskGroup, total: number): string {
  if (!group.tasks.length) return `<div class="tg-summary is-empty"><p>No tasks yet.</p></div>`;
  const average = group.average ?? 0;
  const stacks = new Map<number, number>();
  const dots = [...group.tasks].sort((a, b) => a.priority - b.priority).map((task, i) => {
    const priority = Math.max(1, Math.min(10, Math.round(task.priority)));
    const stack = stacks.get(priority) ?? 0;
    stacks.set(priority, stack + 1);
    return `<i class="${task.procrastinated ? "deferred" : ""}" style="--x:${((priority - 1) / 9).toFixed(3)}; --stack:${Math.min(stack, 3)}; --d:${i}"></i>`;
  }).join("");
  const share = total ? group.tasks.length / total : 0;
  const chips = [
    group.high ? `<span class="tg-chip high">${icons.flag}${group.high} important</span>` : "",
    group.overdue ? `<span class="tg-chip overdue">${icons.clock}${group.overdue} overdue</span>` : "",
    group.timed > group.overdue ? `<span class="tg-chip">${icons.clock}${group.timed - group.overdue} timed</span>` : "",
    group.deferred ? `<span class="tg-chip deferred">${icons.hourglass}${group.deferred} deferred</span>` : ""
  ].join("");
  const progress = group.progress ?? 0;
  const finished = group.tasks.filter(task => progressOf(task) === 100).length;
  const spread = group.tasks.map(task => task.priority);
  return `<div class="tg-summary">
      ${ring({value: average / 10, size: 66, label: `Average importance ${average} out of 10`, center: average.toFixed(1), tone: ""})}
      <div class="tg-stats"><span class="tg-label">Average importance</span>
        <div class="tg-strip" role="img" aria-label="${e(`${plural(group.tasks.length, "task")} rated from ${Math.min(...spread)} to ${Math.max(...spread)} out of 10`)}">${dots}<b class="tg-avg" style="--x:${((Math.max(1, Math.min(10, average)) - 1) / 9).toFixed(3)}"></b></div>
        <span class="tg-scale" aria-hidden="true"><span>1</span><span>10</span></span></div>
      <div class="tg-progress ${progress === 100 ? "is-full" : ""}" role="img" aria-label="${e(`Average progress ${progress}%${finished ? `, ${finished} of ${plural(group.tasks.length, "task")} at 100%` : ""}`)}"><span class="tg-label">Progress</span><span class="tg-progress-bar" style="--w:${(progress / 100).toFixed(3)}" aria-hidden="true"></span><b>${progress}%</b>${finished ? `<span class="tg-progress-note">${finished} at 100%</span>` : ""}</div>
      <div class="tg-meta"><span class="tg-share"><span class="tg-share-bar" style="--w:${share.toFixed(3)}" aria-hidden="true"></span>${Math.round(share * 100)}% of open tasks</span>${chips}</div>
    </div>`;
}

function composer(group: TaskGroup, open: Composer | null): string {
  if (!open) return `<button type="button" class="add-inline" data-compose="${e(group.id)}">${icons.plus}<span>Add a task</span></button>`;
  const priority = open.priority;
  return `<form class="composer" data-composer="${e(group.id)}" autocomplete="off">
      <input name="composerText" aria-label="New task in ${e(group.name)}" placeholder="What needs doing?" maxlength="500" value="${e(open.draft)}" enterkeyhint="done">
      <div class="composer-row">
        <label class="composer-importance"><span>Importance</span><input type="range" class="importance-range" name="composerPriority" min="1" max="10" step="1" value="${priority}" style="--p:${((priority - 1) / 9).toFixed(3)}" aria-label="Importance, 1 to 10"><b>${priority}</b></label>
        <button type="submit" class="composer-add" title="Add task (Enter)">${icons.enter}<span>Add</span></button>
        <button type="button" class="composer-close" data-compose-close aria-label="Close the new task form" title="Close (Esc)">${icons.close}</button>
      </div>
    </form>`;
}

const observers = new WeakMap<HTMLElement, ResizeObserver>();
/** Where each group went last time, per board kind, so redraws of the same groups keep their columns. */
const placements = new Map<string, {order: string; columns: number; columnOf: Map<string, number>}>();

/** Masonry that keeps reading order. The board is a grid of short rows: each group, in order, goes to the
 *  shortest column and spans as many rows as its height needs. Once placed, a group keeps its column while
 *  it grows or shrinks in place (a form opening, a task added, one group folding), so nothing jumps across
 *  the board; `fresh` places every group anew, as after folding or expanding them all. */
export function masonry(board: HTMLElement, fresh = false): void {
  observers.get(board)?.disconnect();
  const items = [...board.children] as HTMLElement[];
  const reset = () => items.forEach(item => { item.style.removeProperty("grid-row"); item.style.removeProperty("grid-column"); });
  // Folded summaries are all about the same height, so they sit in plain rows (see .is-uniform).
  if (board.classList.contains("is-uniform")) { reset(); return; }
  const kind = board.classList.contains("flow-board") ? "flow" : "grid";
  const ids = items.map((item, i) => item.dataset.group ?? item.dataset.slot ?? `#${i}`);
  const order = ids.join("\n");
  const place = (reuse: boolean) => {
    const style = getComputedStyle(board);
    const unit = parseFloat(style.gridAutoRows);
    const columns = style.gridTemplateColumns.split(" ").filter(Boolean).length;
    if (!(unit > 0) || !columns || !items.some(item => item.offsetHeight)) return;
    const saved = placements.get(kind);
    const columnOf = reuse && saved?.order === order && saved.columns === columns ? saved.columnOf : new Map<string, number>();
    // Read every height before writing any placement, so the layout is computed once.
    const spans = items.map(item => Math.max(1, Math.ceil((item.offsetHeight + (parseFloat(getComputedStyle(item).marginBottom) || 0)) / unit)));
    const heights = new Array<number>(columns).fill(0);
    items.forEach((item, i) => {
      if (item.classList.contains("empty")) {
        // A message, such as for a search without matches, spans the whole board.
        const top = Math.max(...heights);
        item.style.gridColumn = "1 / -1";
        item.style.gridRow = `${top + 1} / span ${spans[i]}`;
        heights.fill(top + spans[i]);
        return;
      }
      let column = columnOf.get(ids[i]);
      if (column === undefined) { column = heights.indexOf(Math.min(...heights)); columnOf.set(ids[i], column); }
      item.style.gridColumn = String(column + 1);
      item.style.gridRow = `${heights[column] + 1} / span ${spans[i]}`;
      heights[column] += spans[i];
    });
    placements.set(kind, {order, columns, columnOf});
  };
  place(!fresh);
  if (typeof ResizeObserver !== "function") return;
  // A group changing height keeps its column; a board that gains or loses a column places everything anew.
  const observer = new ResizeObserver(() => place(true));
  for (const element of [board, ...items]) observer.observe(element);
  observers.set(board, observer);
}

export function foldSwitch(view: BoardView): string {
  const folded = view === "folded";
  return `<div class="fold-switch ${folded ? "is-folded" : ""}" role="group" aria-label="Task board view"><span class="fold-thumb" aria-hidden="true"></span><button type="button" data-board-view="expanded" aria-pressed="${!folded}" title="Show every task">${icons.expand}<span>Expand</span></button><button type="button" data-board-view="folded" aria-pressed="${folded}" title="Fold each group into a summary">${icons.fold}<span>Fold</span></button></div>`;
}
