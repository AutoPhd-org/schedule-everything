// Tasks-by-type board: one group per configured task type. Each group either lists its tasks or folds into a
// summary of its metrics, and takes new tasks inline. Each task has a progress slider, or a checklist of
// subtasks that sets its progress, and a timed task shows when it is due (or how long it has been overdue).
// The board is also where task types are managed: a tile at the end of the shelf adds one, and each group's
// menu renames or deletes its type. Tasks an AI agent suggested wait in their type's group too, drawn apart from
// the user's own (dashed and labeled with who suggested them and why) and left out of every count, until the user
// adds one to their tasks or rejects it.
//
// Two layouts: compact packs the groups in reading order, those with more important work first; spread places
// them by workload (workload.ts), the heaviest in the middle and lighter ones farther out, so the distance
// between two groups shows how different their loads are. Either way the groups keep their places while the
// page stays open: a new task, a finished group, or a group that gets its first task never reshuffles the
// board under the cursor. They are placed afresh when the page is opened again (see boardOrder).
// Rendering is pure (state in, HTML out); render.ts owns the board state and binding.
import { ring } from "./charts";
import type { BoardLayout } from "./prefs";
import { dueAt, dueLabel, clockText, isOverdue, progressControl, progressOf } from "./timers";
import type { Snapshot, SuggestionItem, TaskItem } from "./types";
import { e, empty, formatDate, hash, icons, isoDay, sw, typeSwatch } from "./ui";
import { describeLoad, workload } from "./workload";

export type BoardView = "expanded" | "folded";
/** The inline form for a new task: which group it is open in, the chosen importance, and any unsent text. */
export type Composer = {type: string; priority: number; draft: string};
/** The field for a new subtask: which task (by description) it is open on, and any unsent text. */
export type SubtaskComposer = {task: string; draft: string};
/** A type's tasks and the metrics its folded summary shows: average importance, average progress (0–100),
 *  how many tasks are important, timed, overdue, or deferred, and how many subtasks there are and are done.
 *  `suggestions` are the tasks agents suggested for this type; no metric counts them. */
export type TaskGroup = {id: string; name: string; tasks: TaskItem[]; suggestions: SuggestionItem[]; average: number | null; progress: number | null; high: number; timed: number; overdue: number; deferred: number; steps: number; stepsDone: number};
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
  /** The groups on the board, in the order they keep while the page stays open (see boardGroups). */
  order?: readonly string[] | null;
  layout?: BoardLayout;
  /** The new-subtask field, when open. */
  subtaskComposer?: SubtaskComposer | null;
};

const byId = (a: string, b: string) => a.localeCompare(b, undefined, {numeric: true});
const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;
/** The name used for a view transition, stable for a group across redraws. */
export const groupTransitionName = (id: string) => `tg-${hash(id).toString(36)}`;

/** One group per configured type, even without tasks, plus any type that only existing tasks (or suggestions)
 *  still use. Groups are ordered by average importance (to one decimal, as shown), then by how many tasks they
 *  hold; groups without tasks come last in configured order. Tasks and suggestions keep the server's order. */
export function groupTasks(snapshot: Snapshot, now = Date.now()): TaskGroup[] {
  const groups = new Map<string, TaskGroup>();
  const group = (id: string, name: string) => {
    if (!groups.has(id)) groups.set(id, {id, name, tasks: [], suggestions: [], average: null, progress: null, high: 0, timed: 0, overdue: 0, deferred: 0, steps: 0, stepsDone: 0});
    return groups.get(id)!;
  };
  for (const id of Object.keys(snapshot.taskTypes).sort(byId)) group(id, snapshot.taskTypes[id]);
  for (const task of snapshot.tasks) group(task.type, snapshot.taskTypes[task.type] ?? task.typeName ?? "Other").tasks.push(task);
  for (const item of snapshot.suggestions ?? []) group(item.type, snapshot.taskTypes[item.type] ?? item.typeName ?? "Other").suggestions.push(item);
  for (const item of groups.values()) {
    if (!item.tasks.length) continue;
    item.average = Math.round(item.tasks.reduce((sum, task) => sum + task.priority, 0) / item.tasks.length * 10) / 10;
    item.progress = Math.round(item.tasks.reduce((sum, task) => sum + progressOf(task), 0) / item.tasks.length);
    item.high = item.tasks.filter(task => task.priority >= 8).length;
    item.timed = item.tasks.filter(task => dueAt(task) !== null).length;
    item.overdue = item.tasks.filter(task => isOverdue(task, now)).length;
    item.deferred = item.tasks.filter(task => task.procrastinated).length;
    item.steps = item.tasks.reduce((sum, task) => sum + (task.subtasks?.length ?? 0), 0);
    item.stepsDone = item.tasks.reduce((sum, task) => sum + (task.subtasks?.filter(step => step.done).length ?? 0), 0);
  }
  return [...groups.values()].sort((a, b) => (b.average ?? -1) - (a.average ?? -1) || b.tasks.length - a.tasks.length || byId(a.id, b.id));
}

/** Whether a group belongs on the board: it has tasks, or suggestions waiting for review. */
const busy = (group: TaskGroup) => group.tasks.length > 0 || group.suggestions.length > 0;

/** The order of the groups on the board as the page opens: the groups with tasks or suggestions, in compact
 *  order, or for the spread layout from the heaviest workload down (equal ones keep their compact order). */
export function boardOrder(snapshot: Snapshot, layout: BoardLayout = "compact", now = Date.now()): string[] {
  const groups = groupTasks(snapshot, now).filter(busy);
  if (layout === "spread") {
    const load = new Map(groups.map(group => [group.id, workload(group.tasks, snapshot.today.date).points]));
    groups.sort((a, b) => load.get(b.id)! - load.get(a.id)!);
  }
  return groups.map(group => group.id);
}

/** The groups on the board. With an `order` (taken when the page opened, plus groups that joined since), every
 *  group in it stays in its place, even once its tasks are all done, and groups that got their first task or
 *  suggestion since join at the end. Without one, the groups with tasks or suggestions, in compact order. */
export function boardGroups(snapshot: Snapshot, order: readonly string[] | null | undefined, now = Date.now()): TaskGroup[] {
  const all = groupTasks(snapshot, now);
  if (!order) return all.filter(busy);
  const found = new Map(all.map(group => [group.id, group]));
  const listed = new Set(order);
  return [...order.flatMap(id => found.get(id) ?? []), ...all.filter(group => busy(group) && !listed.has(group.id))];
}

export function taskBoard(options: BoardOptions): string {
  const {snapshot, query} = options;
  const needle = query.toLowerCase();
  const onBoard = boardGroups(snapshot, options.order, options.now);
  // A search shows only the groups with matches, listing them so the matching tasks can be seen.
  // Types without tasks wait on the shelf below the board (see typeShelf).
  const matches = (text: string) => text.toLowerCase().includes(needle);
  const groups = needle ? onBoard.map(group => ({...group, tasks: group.tasks.filter(task => matches(task.description)), suggestions: group.suggestions.filter(item => matches(item.description))})).filter(busy) : onBoard;
  if (!groups.length) return empty(needle ? "No tasks match your search." : "No open tasks. Add one to a type below, or use New task.");
  return groups.map((group, order) => groupCard(group, order, options, !needle && options.isFolded(group.id), !!needle)).join("");
}

/** Types without tasks, kept out of the board's way: each is a small pill that can take its first task (which
 *  moves it up to the board), be renamed, or be deleted. The shelf also adds new types. Hidden while searching. */
export function typeShelf(options: BoardOptions): string {
  if (options.query) return "";
  const onBoard = new Set(boardGroups(options.snapshot, options.order, options.now).map(group => group.id));
  const waiting = groupTasks(options.snapshot, options.now).filter(group => !busy(group) && !onBoard.has(group.id));
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
  const now = options.now ?? Date.now();
  const greeted = !!options.added && group.tasks.some(task => task.description === options.added);
  const deleting = !searching && options.deleting?.id === group.id ? options.deleting : null;
  const load = workload(group.tasks, s.today.date);
  const toggle = searching || !count ? "" : `<button type="button" class="group-toggle" data-group-toggle="${id}" aria-expanded="${!folded}" aria-label="${folded ? "Show the tasks in" : "Fold"} ${name}" title="${folded ? "Show tasks" : "Fold into a summary"}">${icons.chevron}</button>`;
  const title = !searching && options.renaming === group.id
    ? `<input class="type-name-input" name="typeName" data-type-name-input="${id}" value="${name}" maxlength="60" autocomplete="off" aria-label="Name of this type" enterkeyhint="done">`
    : `<h3 data-type-title="${id}" ${searching ? "" : 'title="Double-click to rename"'}>${name}</h3>`;
  const writing = options.subtaskComposer;
  const offers = group.suggestions;
  const body = deleting ? deletePanel(group, s, deleting.moveTo)
    : !count ? (offers.length ? "" : cleared())
    : folded ? groupSummary(group, s.tasks.length)
    : `<div class="task-cards">${group.tasks.map((task, position) => taskCard(task, s.tasks.indexOf(task), position, now, task.description === options.added, writing?.task === task.description ? writing : null)).join("")}</div>`;
  // Suggestions wait below the tasks; a folded group only says how many there are, and opens to show them.
  const review = deleting || !offers.length ? "" : folded && count ? suggestionNote(group) : suggestionList(group, count, now);
  // In the spread layout each group wears its workload, which is what places it.
  const badge = options.layout === "spread" ? `<span class="group-load load-${load.level}" data-tip="${e(describeLoad(load))}">${icons.gauge}<b>${load.points.toFixed(1)}</b></span>` : "";
  const classes = ["task-group", sw(typeSwatch(s.taskTypes, group.id)), folded && count ? "is-folded" : "", count || offers.length ? "" : "is-cleared", offers.length ? "has-suggestions" : "", greeted && folded ? "just-added" : "", options.greet === group.id ? "greet" : "", deleting ? "is-deleting" : "", options.menu === group.id && !searching ? "menu-open" : ""].filter(Boolean).join(" ");
  return `<article class="${classes}" data-group="${id}" data-load="${load.points}" style="--tilt:${(hash(group.id) % 5 - 2) * 0.6}deg; --shape:${hash(group.name) % 4}; --vt:${groupTransitionName(group.id)}; --g:${order}">
    ${badge}<header class="group-heading"><span class="group-icon">${icons.tasks}</span>${title}${searching ? (count ? countBadge(count) : "") : typeMenu(group, s, count || !offers.length ? count : null, options.menu === group.id)}${toggle}</header>
    ${body}${review}
    ${deleting ? "" : `<div class="compose-slot ${options.opening === group.id ? "opening" : ""}">${composer(group, options.composer?.type === group.id ? options.composer : null)}</div>`}
  </article>`;
}

/** A group whose tasks were all finished since the page opened keeps its place until the page is opened again. */
const cleared = () => `<div class="group-cleared"><span class="cleared-icon" aria-hidden="true">${icons.party}</span><p><strong>All done here.</strong><span>This type moves to the shelf the next time you open the page.</span></p></div>`;

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

function taskCard(task: TaskItem, index: number, position: number, now: number, added: boolean, writing: SubtaskComposer | null): string {
  const at = dueAt(task);
  const overdue = at !== null && at <= now;
  const progress = progressOf(task);
  const name = e(task.description);
  const steps = task.subtasks ?? [];
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
  // Without subtasks, a button beside the slider breaks the task down; from then on the subtasks set its progress.
  const breakDown = steps.length || writing ? "" : `<button type="button" class="subtask-start" data-subtask-open="${index}" aria-label="${e(`Add subtasks to ${task.description}`)}" title="Break it into subtasks. Progress then follows them.">${icons.subtaskAdd}</button>`;
  return `<div class="${classes}" style="--i:${position}" ${at !== null ? `data-at="${at}" data-kind="task"` : ""}><div class="task-main"><button type="button" class="complete-button" data-task="${index}" data-operation="deleted" aria-label="Complete ${name}" title="Complete task">${icons.check}</button><h4><span class="task-title">${name}</span></h4></div><div class="task-meta"><span class="priority p-${level}">${icons.flag} Importance ${task.priority}/10</span><span class="energy" style="--p:${task.priority / 10}" aria-hidden="true"></span><button class="text-button edit" type="button" data-task="${index}" data-operation="edit" aria-label="Edit ${name}">Edit</button></div>${timer}${deferred}${steps.length || writing ? subtaskList(task, index, writing) : ""}<div class="task-progress-row">${progressControl(index, task, "task-progress")}${breakDown}</div><details class="task-more"><summary>More options</summary>${timing}<button type="button" data-task="${index}" data-operation="cancelled">Cancel · added by mistake</button><button type="button" data-task="${index}" data-operation="dropped">Drop · no longer pursuing</button></details></div>`;
}

/** A task's checklist: check a subtask off, remove it, or add another; the task's progress follows. */
function subtaskList(task: TaskItem, index: number, writing: SubtaskComposer | null): string {
  const steps = task.subtasks ?? [];
  const items = steps.map((step, i) => `<li class="subtask ${step.done ? "is-done" : ""}" style="--s:${i}"><label class="subtask-label"><input type="checkbox" data-subtask="${index}" data-step="${i}" ${step.done ? "checked" : ""}><span class="subtask-box" aria-hidden="true">${icons.check}</span><span class="subtask-text">${e(step.text)}</span></label><button type="button" class="subtask-remove" data-subtask-remove="${index}" data-step="${i}" aria-label="${e(`Remove the subtask ${step.text}`)}" title="Remove">${icons.close}</button></li>`).join("");
  const field = writing
    ? `<form class="subtask-form" data-subtask-form="${index}" autocomplete="off"><span class="subtask-form-icon" aria-hidden="true">${icons.next}</span><input name="subtaskText" maxlength="200" placeholder="${steps.length ? "Another subtask…" : "First subtask…"}" value="${e(writing.draft)}" aria-label="${e(`New subtask for ${task.description}`)}" enterkeyhint="done"><button type="submit" class="subtask-submit" aria-label="Add the subtask" title="Add (Enter)">${icons.enter}</button><button type="button" class="subtask-close" data-subtask-close aria-label="Close the new subtask field" title="Close (Esc)">${icons.close}</button></form>`
    : `<button type="button" class="subtask-add" data-subtask-open="${index}">${icons.plus}<span>Add a subtask</span></button>`;
  const allDone = steps.length > 0 && steps.every(step => step.done);
  return `<div class="subtasks ${allDone ? "is-done" : ""}">${steps.length ? `<ul class="subtask-list" aria-label="${e(`Subtasks of ${task.description}`)}">${items}</ul>` : ""}${field}</div>`;
}

/** A folded group's suggestions: how many wait for review, as a button that opens the group to show them. */
function suggestionNote(group: TaskGroup): string {
  const count = group.suggestions.length;
  const agents = [...new Set(group.suggestions.map(item => item.agent))];
  return `<button type="button" class="suggestion-note" data-group-toggle="${e(group.id)}" aria-label="${e(`Show ${plural(count, "suggested task")} from ${agents.join(" and ")} in ${group.name}`)}">${icons.spark}<span><b>${count}</b> suggested by ${e(agents.join(", "))}</span><span class="suggestion-note-go">Review ${icons.arrow}</span></button>`;
}

/** The suggestions in a group, under a label that says they are not tasks yet. */
function suggestionList(group: TaskGroup, tasks: number, now: number): string {
  const count = group.suggestions.length;
  const label = `<p class="suggestion-label">${icons.spark}<span>${count === 1 ? "Suggested for you" : `${count} suggested for you`}</span><span class="suggestion-hint">Not on your list until you add ${count === 1 ? "it" : "them"}</span></p>`;
  return `<section class="suggestion-list ${tasks ? "" : "is-alone"}" aria-label="${e(`Suggested tasks in ${group.name}`)}">${label}${group.suggestions.map((item, position) => suggestionCard(item, position, now)).join("")}</section>`;
}

/** A suggested task: who suggested it and where they found it, what it would be, why, and the choice to add it to
 *  the user's tasks (as it is, or adjusted first) or reject it. */
export function suggestionCard(item: SuggestionItem, position: number, now = Date.now()): string {
  const name = e(item.description);
  const level = item.priority >= 8 ? "high" : item.priority >= 5 ? "medium" : "low";
  const due = item.due ? `<span class="suggestion-fact">${icons.clock}Finish by ${e(dueLabel(item.due, now))}</span>` : "";
  const later = item.alarmFrom && item.alarmFrom > isoDay(new Date(now)) ? `<span class="suggestion-fact">${icons.hourglass}Reminders from ${e(formatDate(item.alarmFrom, {month: "short", day: "numeric"}))}</span>` : "";
  const from = item.source ? ` <span class="suggestion-source">· from ${e(item.source)}</span>` : "";
  return `<article class="suggestion" data-suggestion="${e(item.id)}" style="--i:${position}">
      <p class="suggestion-from">${icons.spark}<span>Suggested by <b>${e(item.agent)}</b>${from}</span></p>
      <h4 class="suggestion-title">${name}</h4>
      <div class="suggestion-meta"><span class="priority p-${level}">${icons.flag} Importance ${item.priority}/10</span>${due}${later}</div>
      ${item.reason ? `<p class="suggestion-reason">${e(item.reason)}</p>` : ""}
      <div class="suggestion-actions">
        <button type="button" class="suggestion-accept" data-suggestion-accept="${e(item.id)}" aria-label="${e(`Add ${item.description} to your tasks`)}">${icons.check}<span>Add to my tasks</span></button>
        <button type="button" class="suggestion-adjust" data-suggestion-adjust="${e(item.id)}" aria-label="${e(`Adjust ${item.description}, then add it`)}" title="Change it, then add it">${icons.edit}<span>Adjust</span></button>
        <button type="button" class="suggestion-reject" data-suggestion-reject="${e(item.id)}" aria-label="${e(`Reject ${item.description}`)}" title="Not for me">${icons.close}<span>Reject</span></button>
      </div>
    </article>`;
}

/** A folded group: average importance as a ring, every task as a dot on a 1–10 strip, how far along its tasks
 *  are on average, its share of the list, and how many tasks are important, timed, overdue, or deferred, and
 *  how many of their subtasks are done. */
function groupSummary(group: TaskGroup, total: number): string {
  if (!group.tasks.length) return cleared();
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
    group.deferred ? `<span class="tg-chip deferred">${icons.hourglass}${group.deferred} deferred</span>` : "",
    group.steps ? `<span class="tg-chip steps">${icons.subtasks}${group.stepsDone}/${group.steps} subtasks</span>` : ""
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

// ----- Layout ------------------------------------------------------------------

const observers = new WeakMap<HTMLElement, ResizeObserver>();
/** Each group's column in the compact layout, per board kind, so redraws keep the groups where they were. */
const placements = new Map<string, {columns: number; columnOf: Map<string, number>}>();
const kindOf = (board: HTMLElement) => board.classList.contains("flow-board") ? "flow" : "grid";

/** Masonry that keeps reading order. The board is a grid of short rows: each group, in order, goes to the
 *  shortest column and spans as many rows as its height needs. Once placed, a group keeps its column while
 *  it grows or shrinks in place (a form opening, a task added, one group folding), and a group that joins
 *  goes to the shortest column, so nothing jumps across the board; `fresh` places every group anew, as when
 *  the page opens or after folding or expanding them all. */
export function masonry(board: HTMLElement, fresh = false): void {
  observers.get(board)?.disconnect();
  const items = [...board.children] as HTMLElement[];
  const reset = () => items.forEach(item => { item.style.removeProperty("grid-row"); item.style.removeProperty("grid-column"); });
  // Folded summaries are all about the same height, so they sit in plain rows (see .is-uniform).
  if (board.classList.contains("is-uniform")) { reset(); return; }
  const kind = kindOf(board);
  const ids = items.map((item, i) => item.dataset.group ?? item.dataset.slot ?? `#${i}`);
  const place = (reuse: boolean) => {
    // The resolved track list also counts the extra columns that groups placed for a wider board create, so a
    // board that narrowed would keep them. Read it with the groups unplaced; they are placed again below, before
    // anything is painted.
    reset();
    const style = getComputedStyle(board);
    const unit = parseFloat(style.gridAutoRows);
    const columns = style.gridTemplateColumns.split(" ").filter(Boolean).length;
    if (!(unit > 0) || !columns || !items.some(item => item.offsetHeight)) return;
    const saved = placements.get(kind);
    const columnOf = reuse && saved?.columns === columns ? saved.columnOf : new Map<string, number>();
    // Read every height before writing any placement, so the layout is computed only once more.
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
      if (column === undefined || column >= columns) { column = heights.indexOf(Math.min(...heights)); columnOf.set(ids[i], column); }
      item.style.gridColumn = String(column + 1);
      item.style.gridRow = `${heights[column] + 1} / span ${spans[i]}`;
      heights[column] += spans[i];
    });
    placements.set(kind, {columns, columnOf});
  };
  place(!fresh);
  if (typeof ResizeObserver !== "function") return;
  // A group changing height keeps its column; a board that gains or loses a column places everything anew.
  const observer = new ResizeObserver(() => place(true));
  for (const element of [board, ...items]) observer.observe(element);
  observers.set(board, observer);
}

type Spot = {x: number; y: number};
type Box = Spot & {w: number; h: number};
/** Where each group sits in the spread layout, per board kind, and what the places were measured against. */
const spreads = new Map<string, {width: number; max: number; reach: number; center: Spot; spots: Map<string, Spot>}>();
const GAP = 22;
/** Narrower boards stack the groups in one column, the gap before each growing with how much lighter it is. */
const STACK_BELOW = 700;
const GOLDEN = Math.PI * (3 - Math.sqrt(5));
const apart = (a: Box, b: Box) => a.x + a.w + GAP <= b.x || b.x + b.w + GAP <= a.x || a.y + a.h + GAP <= b.y || b.y + b.h + GAP <= a.y;

/** Look for room for a box along a spiral that starts `radius` from the center, so it lands as near that
 *  distance as the boxes already placed allow, and never past the board's sides. */
function findRoom(center: Spot, radius: number, angle: number, w: number, h: number, width: number, placed: Box[]): Spot {
  for (let step = 0; step < 6000; step++) {
    const turn = step * 0.2;
    const r = radius + turn * 5;
    const x = Math.round(center.x + r * Math.cos(angle + turn) - w / 2);
    const y = Math.round(center.y + r * Math.sin(angle + turn) - h / 2);
    if (x < 0 || x + w > width) continue;
    if (placed.every(box => apart({x, y, w, h}, box))) return {x, y};
  }
  return {x: Math.max(0, Math.round(center.x - w / 2)), y: Math.max(0, ...placed.map(box => box.y + box.h + GAP))};
}

/** The spread layout. Groups carry their workload (data-load). The heaviest sits in the middle; every other
 *  group aims for a distance from it that grows with how much lighter it is, the lightest reaching the board's
 *  edge, and takes the nearest free spot along a spiral from there. Rings behind the groups mark a third, two
 *  thirds, and all of that reach. Places are kept while groups grow or shrink: a group that grows pushes the
 *  groups below it down, and a group that joins finds room the same way. `fresh`, or a change in the board's
 *  width, places every group anew. */
export function spreadOut(board: HTMLElement, fresh = false): void {
  observers.get(board)?.disconnect();
  const items = [...board.children] as HTMLElement[];
  const groups = items.filter(item => item.dataset.group);
  const kind = kindOf(board);
  let measured = -1;
  const place = (anew: boolean) => {
    const width = board.clientWidth;
    measured = width;
    const ready = groups.length > 0 && width > 0 && groups.some(item => item.offsetHeight);
    board.classList.toggle("is-placed", ready);
    board.classList.toggle("is-stacked", ready && width < STACK_BELOW);
    if (!ready) { board.style.removeProperty("height"); return; }
    const loads = groups.map(item => Number(item.dataset.load) || 0);
    if (width < STACK_BELOW) {
      board.style.removeProperty("height");
      const max = Math.max(...loads, 0.1);
      groups.forEach((item, i) => {
        item.style.removeProperty("left");
        item.style.removeProperty("top");
        item.style.setProperty("--gap-before", `${i ? Math.round(Math.max(0, loads[i - 1] - loads[i]) / max * 72) : 0}px`);
      });
      return;
    }
    // Read every size before writing any place.
    const sizes = groups.map(item => ({w: item.offsetWidth, h: item.offsetHeight}));
    let saved = spreads.get(kind);
    if (anew || !saved || saved.width !== width) {
      const max = Math.max(...loads, 0);
      const reach = Math.max(width / 2, 320);
      const center = {x: width / 2, y: 0};
      const spots = new Map<string, Spot>();
      const placed: Box[] = [];
      groups.forEach((item, i) => {
        const distance = max > 0 ? (max - loads[i]) / max : 0;
        const spot = findRoom(center, distance * reach, i * GOLDEN - Math.PI / 2, sizes[i].w, sizes[i].h, width, placed);
        placed.push({...spot, ...sizes[i]});
        spots.set(item.dataset.group!, spot);
      });
      // Start at the top of the board.
      const top = Math.min(...placed.map(box => box.y));
      for (const spot of spots.values()) spot.y -= top;
      saved = {width, max, reach, center: {x: center.x, y: center.y - top}, spots};
      spreads.set(kind, saved);
    }
    const {spots} = saved;
    // Keep each group's place, pushed down below any group above it that grew into it.
    const boxes: Box[] = [];
    const spotOf = (i: number) => spots.get(groups[i].dataset.group!);
    const known = groups.map((_, i) => i).filter(i => spotOf(i))
      .sort((a, b) => spotOf(a)!.y - spotOf(b)!.y || spotOf(a)!.x - spotOf(b)!.x);
    const at = new Map<number, Spot>();
    for (const i of known) {
      const {x, y} = spotOf(i)!;
      const {w, h} = sizes[i];
      const top = boxes.filter(box => x < box.x + box.w + GAP && box.x < x + w + GAP).reduce((lowest, box) => Math.max(lowest, box.y + box.h + GAP), y);
      boxes.push({x, y: top, w, h});
      at.set(i, {x, y: top});
    }
    // A group that joined since finds room near the distance its workload calls for.
    groups.forEach((item, i) => {
      if (at.has(i)) return;
      const distance = saved!.max > 0 ? Math.max(0, saved!.max - loads[i]) / saved!.max : 0;
      const spot = findRoom(saved!.center, distance * saved!.reach, i * GOLDEN - Math.PI / 2, sizes[i].w, sizes[i].h, width, boxes);
      spots.set(item.dataset.group!, spot);
      boxes.push({...spot, ...sizes[i]});
      at.set(i, spot);
    });
    groups.forEach((item, i) => {
      item.style.left = `${at.get(i)!.x}px`;
      item.style.top = `${at.get(i)!.y}px`;
      item.style.removeProperty("--gap-before");
    });
    board.style.height = `${Math.max(...boxes.map(box => box.y + box.h)) + 8}px`;
    board.style.setProperty("--cx", `${Math.round(saved.center.x)}px`);
    board.style.setProperty("--cy", `${Math.round(saved.center.y)}px`);
    board.style.setProperty("--reach", `${Math.round(saved.reach)}px`);
  };
  place(fresh);
  if (typeof ResizeObserver !== "function") return;
  // A group changing size keeps its place; a board that changes width places everything anew.
  const observer = new ResizeObserver(() => place(board.clientWidth !== measured));
  for (const element of [board, ...groups]) observer.observe(element);
  observers.set(board, observer);
}

export function foldSwitch(view: BoardView): string {
  const folded = view === "folded";
  return `<div class="fold-switch ${folded ? "is-folded" : ""}" role="group" aria-label="Task board view"><span class="fold-thumb" aria-hidden="true"></span><button type="button" data-board-view="expanded" aria-pressed="${!folded}" title="Show every task">${icons.expand}<span>Expand</span></button><button type="button" data-board-view="folded" aria-pressed="${folded}" title="Fold each group into a summary">${icons.fold}<span>Fold</span></button></div>`;
}

export function layoutSwitch(layout: BoardLayout): string {
  const spread = layout === "spread";
  return `<div class="layout-toggle ${spread ? "is-spread" : ""}" role="group" aria-label="Task board layout"><span class="fold-thumb" aria-hidden="true"></span><button type="button" data-board-layout="compact" aria-pressed="${!spread}" title="Compact · groups packed in rows, those with more important work first">${icons.grid}<span>Compact</span></button><button type="button" data-board-layout="spread" aria-pressed="${spread}" title="Spread · groups placed by workload, the heaviest in the middle and lighter ones farther out">${icons.orbit}<span>Spread</span></button></div>`;
}
