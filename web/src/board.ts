// Tasks-by-type board: one group per configured task type, ordered so groups with more important work come
// first. Each group either lists its tasks or folds into a summary of its metrics, and takes new tasks inline.
// Rendering is pure (state in, HTML out); render.ts owns the board state and event binding.
import { ring } from "./charts";
import type { Snapshot, TaskItem } from "./types";
import { e, empty, hash, icons, sw, typeSwatch } from "./ui";

export type BoardView = "expanded" | "folded";
/** The inline form for a new task: which group it is open in, the chosen importance, and any unsent text. */
export type Composer = {type: string; priority: number; draft: string};
export type TaskGroup = {id: string; name: string; tasks: TaskItem[]; average: number | null; high: number; later: number; deferred: number};
export type BoardOptions = {
  snapshot: Snapshot;
  query: string;
  isFolded: (id: string) => boolean;
  composer: Composer | null;
  /** Description of a task that was just added, to greet it. */
  added: string | null;
  /** Group whose inline form just opened, to animate it in. */
  opening?: string | null;
};

const byId = (a: string, b: string) => a.localeCompare(b, undefined, {numeric: true});
const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;
/** Reminders for a postponed task start on a later day. */
export const isLater = (task: TaskItem, today: string) => !!task.alarmFrom && task.alarmFrom > today;
/** The name used for a view transition, stable for a group across redraws. */
export const groupTransitionName = (id: string) => `tg-${hash(id).toString(36)}`;

/** One group per configured type, even without tasks, plus any type that only existing tasks still use.
 *  Groups are ordered by average importance (to one decimal, as shown), then by how many tasks they hold;
 *  groups without tasks come last in configured order. Tasks keep the server's order inside each group. */
export function groupTasks(snapshot: Snapshot): TaskGroup[] {
  const groups = new Map<string, TaskGroup>();
  const group = (id: string, name: string) => {
    if (!groups.has(id)) groups.set(id, {id, name, tasks: [], average: null, high: 0, later: 0, deferred: 0});
    return groups.get(id)!;
  };
  for (const id of Object.keys(snapshot.taskTypes).sort(byId)) group(id, snapshot.taskTypes[id]);
  for (const task of snapshot.tasks) group(task.type, snapshot.taskTypes[task.type] ?? task.typeName ?? "Other").tasks.push(task);
  for (const item of groups.values()) {
    if (!item.tasks.length) continue;
    item.average = Math.round(item.tasks.reduce((sum, task) => sum + task.priority, 0) / item.tasks.length * 10) / 10;
    item.high = item.tasks.filter(task => task.priority >= 8).length;
    item.later = item.tasks.filter(task => isLater(task, snapshot.today.date)).length;
    item.deferred = item.tasks.filter(task => task.procrastinated).length;
  }
  return [...groups.values()].sort((a, b) => (b.average ?? -1) - (a.average ?? -1) || b.tasks.length - a.tasks.length || byId(a.id, b.id));
}

export function taskBoard(options: BoardOptions): string {
  const {snapshot, query} = options;
  const needle = query.toLowerCase();
  const all = groupTasks(snapshot);
  // A search shows only the groups with matches, listing them so the matching tasks can be seen.
  const groups = needle ? all.map(group => ({...group, tasks: group.tasks.filter(task => task.description.toLowerCase().includes(needle))})).filter(group => group.tasks.length) : all;
  if (!groups.length) return empty(needle ? "No tasks match your search." : "Add a task type in Settings to start your board.");
  return groups.map((group, order) => groupCard(group, order, options, !needle && options.isFolded(group.id), !!needle)).join("");
}

function groupCard(group: TaskGroup, order: number, options: BoardOptions, folded: boolean, searching: boolean): string {
  const {snapshot: s} = options;
  const count = group.tasks.length;
  const name = e(group.name);
  const greeted = !!options.added && group.tasks.some(task => task.description === options.added);
  const toggle = searching ? "" : `<button type="button" class="group-toggle" data-group-toggle="${e(group.id)}" aria-expanded="${!folded}" aria-label="${folded ? "Show the tasks in" : "Fold"} ${name}" title="${folded ? "Show tasks" : "Fold into a summary"}">${icons.chevron}</button>`;
  const body = folded ? groupSummary(group, s.tasks.length)
    : count ? `<div class="task-cards">${group.tasks.map((task, position) => taskCard(task, s.tasks.indexOf(task), position, s.today.date, task.description === options.added)).join("")}</div>`
    : `<p class="group-empty">Nothing here yet.</p>`;
  const classes = ["task-group", sw(typeSwatch(s.taskTypes, group.id)), folded ? "is-folded" : "", count ? "" : "is-empty", greeted && folded ? "just-added" : ""].filter(Boolean).join(" ");
  return `<article class="${classes}" data-group="${e(group.id)}" style="--tilt:${(hash(group.id) % 5 - 2) * 0.6}deg; --shape:${hash(group.name) % 4}; --vt:${groupTransitionName(group.id)}; --g:${order}">
    <header class="group-heading"><span class="group-icon">${icons.tasks}</span><h3>${name}</h3><span class="group-count" title="${plural(count, "task")}">${count}</span>${toggle}</header>
    ${body}
    <div class="compose-slot ${options.opening === group.id ? "opening" : ""}">${composer(group, options.composer?.type === group.id ? options.composer : null)}</div>
  </article>`;
}

function taskCard(task: TaskItem, index: number, position: number, today: string, added: boolean): string {
  const days = task.alarmFrom ? Math.round((Date.parse(task.alarmFrom) - Date.parse(today)) / 86400000) : 0;
  const detail = days > 0 ? `Reminders ${days === 1 ? "tomorrow" : `in ${days} days`} · ${task.alarmFrom}` : task.procrastinated ? `Deferred${task.procrastinateDays === null ? "" : ` · ${task.procrastinateDays} days`}` : "";
  const level = task.priority >= 8 ? "high" : task.priority >= 5 ? "medium" : "low";
  const classes = ["task-card", task.procrastinated ? "deferred" : "", days > 0 ? "postponed" : task.priority >= 8 ? "high-priority" : "", added ? "just-added" : ""].filter(Boolean).join(" ");
  return `<div class="${classes}" style="--i:${position}"><div class="task-main"><button type="button" class="complete-button" data-task="${index}" data-operation="deleted" aria-label="Complete ${e(task.description)}" title="Complete task">${icons.check}</button><h4>${e(task.description)}</h4></div><div class="task-meta"><span class="priority p-${level}">${icons.flag} Importance ${task.priority}/10</span><span class="energy" style="--p:${task.priority / 10}" aria-hidden="true"></span><button class="text-button edit" type="button" data-task="${index}" data-operation="edit" aria-label="Edit ${e(task.description)}">Edit</button></div>${detail ? `<p class="task-detail">${icons.clock}${e(detail)}</p>` : ""}<details class="task-more"><summary>More options</summary><button type="button" data-task="${index}" data-operation="cancelled">Cancel · added by mistake</button><button type="button" data-task="${index}" data-operation="dropped">Drop · no longer pursuing</button></details></div>`;
}

/** A folded group: average importance as a ring, every task as a dot on a 1–10 strip, its share of the list,
 *  and how many tasks are important, waiting for a later day, or deferred. No task details. */
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
    group.later ? `<span class="tg-chip">${icons.clock}${group.later} later</span>` : "",
    group.deferred ? `<span class="tg-chip deferred">${icons.hourglass}${group.deferred} deferred</span>` : ""
  ].join("");
  const spread = group.tasks.map(task => task.priority);
  return `<div class="tg-summary">
      ${ring({value: average / 10, size: 66, label: `Average importance ${average} out of 10`, center: average.toFixed(1), tone: ""})}
      <div class="tg-stats"><span class="tg-label">Average importance</span>
        <div class="tg-strip" role="img" aria-label="${e(`${plural(group.tasks.length, "task")} rated from ${Math.min(...spread)} to ${Math.max(...spread)} out of 10`)}">${dots}<b class="tg-avg" style="--x:${((Math.max(1, Math.min(10, average)) - 1) / 9).toFixed(3)}"></b></div>
        <span class="tg-scale" aria-hidden="true"><span>1</span><span>10</span></span></div>
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
  const ids = items.map((item, i) => item.dataset.group ?? `#${i}`);
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
      if (!item.dataset.group) {
        // Anything else, such as an empty-search message, spans the whole board.
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
