// Timers above the task board. Task timers count down to the finish-by time of timed tasks due by tonight
// (overdue ones count up); deadline timers count down to deadlines ending within the next 24 hours, and can be
// closed without touching the deadline. Each timer has a dial: a ring that drains as its time runs out, around
// an icon for how close it is (a running clock, an hourglass, an alarm clock in the last hour, a ringing bell
// once overdue, a party popper when the work is all done). A task timer also offers the task's next subtask.
// On top, today's timed work fills one bar with the progress of every timed task due today, finished ones
// included; it can weigh each task by its importance. Rendering is pure (state in, HTML out); render.ts ticks
// the clocks every second and binds the controls.
import { deadlineSwatch } from "./calendar";
import { msg, t, tn } from "./i18n";
import type { DeadlineItem, Snapshot, TaskItem } from "./types";
import { e, formatDate, icons, isoDay, liveClock, parseLocal, sw, typeSwatch } from "./ui";

const DAY = 86_400_000;
const HOUR = 3_600_000;

/** When a timed task should be finished, in local time; normal tasks have no time. */
export const dueAt = (task: Pick<TaskItem, "due">): number | null => task.due ? parseLocal(task.due).getTime() : null;
/** A deadline runs to the end of its day. */
export function deadlineEnd(deadline: DeadlineItem): number | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(deadline.deadline)) return null;
  const end = parseLocal(`${deadline.deadline}T00:00`);
  end.setDate(end.getDate() + 1);
  return end.getTime();
}
/** Midnight at the end of the day `now` falls in. */
export function tonight(now: number): number {
  const end = new Date(now);
  end.setHours(24, 0, 0, 0);
  return end.getTime();
}
export const isOverdue = (task: TaskItem, now: number) => { const at = dueAt(task); return at !== null && at <= now; };
/** How far along a task is, 0–100: the share of its subtasks done while it has any, or else its own progress. */
export function progressOf(task: Pick<TaskItem, "progress" | "subtasks">): number {
  const steps = task.subtasks ?? [];
  if (steps.length) return Math.round(steps.filter(step => step.done).length * 100 / steps.length);
  return Math.max(0, Math.min(100, Math.round(task.progress ?? 0)));
}

/** Closing a deadline's timer is remembered by its name and date, so the next time it repeats it shows again. */
export const timerKey = (deadline: DeadlineItem) => JSON.stringify([deadline.event, deadline.deadline]);
/** Closed timers worth remembering: those whose deadline has not yet passed. */
export function pruneClosed(keys: Iterable<string>, today: string): string[] {
  return [...keys].filter(key => {
    try {
      const [, day] = JSON.parse(key);
      return typeof day === "string" && day >= today;
    } catch { return false; }
  });
}

export type TaskTimer = {kind: "task"; at: number; task: TaskItem; index: number; overdue: boolean};
export type DeadlineTimer = {kind: "deadline"; at: number; deadline: DeadlineItem; index: number; key: string};
export type Timer = TaskTimer | DeadlineTimer;

/** Timed tasks due by tonight (overdue ones too, until they are finished) and deadlines ending within 24
 *  hours that were not closed, soonest first. */
export function collectTimers(s: Snapshot, now: number, closed: ReadonlySet<string>): Timer[] {
  const end = tonight(now);
  const tasks = s.tasks.flatMap((task, index): Timer[] => {
    const at = dueAt(task);
    return at !== null && at < end ? [{kind: "task", at, task, index, overdue: at <= now}] : [];
  });
  const deadlines = s.deadlines.flatMap((deadline, index): Timer[] => {
    const at = deadlineEnd(deadline);
    const key = timerKey(deadline);
    return at !== null && at > now && at - now <= DAY && !closed.has(key) ? [{kind: "deadline", at, deadline, index, key}] : [];
  });
  return [...tasks, ...deadlines].sort((a, b) => a.at - b.at);
}

/** How close a timer is: plenty of time, a few hours left (three for a task, six for a deadline), the last
 *  hour, or past due. A task whose work is all done, and not yet overdue, is ready to complete instead. */
export type Urgency = "calm" | "soon" | "close" | "overdue" | "ready";
export function urgency(kind: Timer["kind"], at: number, now: number, ready = false): Urgency {
  const left = at - now;
  if (kind === "task" && left <= 0) return "overdue";
  if (kind === "task" && ready) return "ready";
  return left <= HOUR ? "close" : left <= (kind === "task" ? 3 : 6) * HOUR ? "soon" : "calm";
}
/** The share of a timer's runway still left, from 1 down to 0: a deadline's last 24 hours, or a task's day
 *  up to its finish time. */
export function runway(kind: Timer["kind"], at: number, now: number): number {
  const day = new Date(at);
  day.setHours(0, 0, 0, 0);
  const span = kind === "deadline" ? DAY : Math.max(HOUR, at - day.getTime());
  return Math.max(0, Math.min(1, (at - now) / span));
}
export const dialDash = (left: number) => `${(left * 100).toFixed(2)} 100`;

export type TodayPart = {description: string; progress: number; priority: number; done: boolean; overdue: boolean};
export type TodayProgress = {value: number; parts: TodayPart[]; done: number; overdue: number; weighted: boolean};

/** Today's timed work: every timed task due by tonight, open or finished today. Finished tasks count as 100%,
 *  so finishing one never lowers the bar. Weighted, each task counts by its importance (an importance-9 task
 *  three times as much as an importance-3 one); otherwise every task counts the same. */
export function todayProgress(s: Snapshot, now: number, weighted = false): TodayProgress {
  const end = tonight(now);
  const today = isoDay(new Date(now));
  const finished: TodayPart[] = (s.timedDone ?? [])
    .filter(item => item.endedAt.slice(0, 10) === today && parseLocal(item.due).getTime() < end)
    .map(item => ({description: item.description, progress: 100, priority: item.priority || 5, done: true, overdue: false}));
  const open: TodayPart[] = s.tasks
    .filter(task => { const at = dueAt(task); return at !== null && at < end; })
    .sort((a, b) => dueAt(a)! - dueAt(b)!)
    .map(task => ({description: task.description, progress: progressOf(task), priority: task.priority, done: false, overdue: isOverdue(task, now)}));
  const parts = [...finished, ...open];
  const weight = (part: TodayPart) => weighted ? partWeight(part) : 1;
  const total = parts.reduce((sum, part) => sum + weight(part), 0);
  const value = total ? parts.reduce((sum, part) => sum + weight(part) * part.progress, 0) / (total * 100) : 0;
  return {value, parts, done: finished.length, overdue: open.filter(part => part.overdue).length, weighted};
}
const partWeight = (part: TodayPart) => Math.max(1, Math.min(10, part.priority));

const pad = (value: number) => String(value).padStart(2, "0");
/** A running clock: `02:14:05`, or days and hours once it is a day or more. */
export function clock(ms: number): string {
  const total = Math.floor(Math.abs(ms) / 1000);
  const days = Math.floor(total / 86400);
  const hours = Math.floor(total % 86400 / 3600);
  if (days) return t("{days}d {hours}h", {days, hours});
  return `${pad(hours)}:${pad(Math.floor(total % 3600 / 60))}:${pad(total % 60)}`;
}
/** A short span for task cards: `2h 14m`, `45m`, `3d 2h`. */
export function span(ms: number): string {
  const minutes = Math.floor(Math.abs(ms) / 60000);
  if (minutes < 1) return t("<1m");
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor(minutes % 1440 / 60);
  return days ? t("{days}d {hours}h", {days, hours}) : hours ? t("{hours}h {minutes}m", {hours, minutes: pad(minutes % 60)}) : t("{n}m", {n: minutes});
}
/** The text of a ticking element, by its `data-clock`: a "clock" or a "short" span of time, or the words around
 *  it, which change once the time has passed ("word": left or overdue; "by": by or was due; "due": Due or Was due). */
export function clockText(kind: string | undefined, at: number, now: number): string {
  const ahead = at > now;
  if (kind === "word") return ahead ? t("left") : t("overdue");
  if (kind === "by") return ahead ? t("by") : t("was due");
  if (kind === "due") return ahead ? t("Due") : t("Was due");
  return kind === "short" ? span(at - now) : clock(at - now);
}

/** When a timed task is due, relative to today: a time today, or the day and time. */
export function dueLabel(due: string, now: number): string {
  const day = due.slice(0, 10);
  const today = isoDay(new Date(now));
  const time = formatDate(due, {hour: "numeric", minute: "2-digit"});
  const offset = Math.round((parseLocal(`${day}T12:00`).getTime() - parseLocal(`${today}T12:00`).getTime()) / DAY);
  if (offset === 0) return time;
  if (offset === 1) return t("tomorrow {time}", {time});
  if (offset === -1) return t("yesterday {time}", {time});
  return formatDate(due, {weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit"});
}

/** `extras` are cards plugins add to the row (plugins.ts), after the timers. */
export type TimerOptions = {snapshot: Snapshot; now: number; closed: ReadonlySet<string>; weighted?: boolean; extras?: string[]};

export function timerStrip({snapshot: s, now, closed, weighted = false, extras = []}: TimerOptions): string {
  const timers = collectTimers(s, now, closed);
  const tasks = timers.filter(timer => timer.kind === "task").length;
  const row = timers.length || extras.length
    ? `<div class="timer-row" role="list" aria-label="${t("{taskTimers} and {deadlineTimers}", {taskTimers: tn("{count} task timer", "{count} task timers", tasks), deadlineTimers: tn("{count} deadline timer", "{count} deadline timers", timers.length - tasks)})}">${timers.map((timer, i) => timer.kind === "task" ? taskTimer(timer, s, now, i) : deadlineTimer(timer, s, now, i)).join("")}${extras.join("")}</div>`
    : `<p class="timer-empty"><span class="timer-empty-icon">${icons.coffee}</span><span>${t("No timers right now. Make a task timed from its card or with <b>New task</b>; deadlines show here in their last 24 hours.")}</span></p>`;
  return `<section class="timers" aria-label="${t("Timers")}">${todayBar(todayProgress(s, now, weighted))}${row}</section>`;
}

function todayBar(today: TodayProgress): string {
  const percent = Math.round(today.value * 100);
  const total = today.parts.length;
  const meta = total
    ? [`<span class="today-done">${icons.check}${t("{done} of {total} done", {done: today.done, total})}</span>`, today.overdue ? `<span class="today-overdue">${icons.ringing}${t("{count} overdue", {count: today.overdue})}</span>` : ""].filter(Boolean).join(" · ")
    : t("No timed tasks due today");
  const segments = total
    ? today.parts.map(part => `<i class="${part.done ? "is-done" : ""} ${part.overdue ? "is-overdue" : ""}" style="--p:${(part.progress / 100).toFixed(2)}; flex-grow:${today.weighted ? partWeight(part) : 1}" title="${e(`${part.description} · ${part.done ? t("done") : `${part.progress}%`}${today.weighted ? ` · ${t("importance {n}", {n: part.priority})}` : ""}`)}"></i>`).join("")
    : `<i class="is-blank" style="--p:0"></i>`;
  const weighting = today.weighted ? `<span class="today-weighted" data-tip="${t("Each task counts by its importance: an importance-9 task moves the bar three times as much as an importance-3 one. Change it in Settings.")}">${icons.scale}<span>${t("By importance")}</span></span>` : "";
  return `<div class="today-progress ${total ? "" : "is-idle"} ${percent === 100 ? "is-complete" : ""}">
      <div class="today-top"><span class="today-label">${percent === 100 ? icons.trophy : icons.target}<span>${t("Today’s timed work")}</span></span><span class="today-meta">${meta}</span>${weighting}<strong class="today-value">${percent}<small>%</small></strong></div>
      <div class="today-bar" role="progressbar" aria-label="${t("Today’s timed work")}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent}" aria-valuetext="${e(total ? `${t("{percent}% · {done} of {total} timed tasks done", {percent, done: today.done, total})}${today.weighted ? ` · ${t("weighted by importance")}` : ""}` : t("No timed tasks due today"))}">${segments}</div>
    </div>`;
}

const DIAL_ICONS: Record<Urgency, string> = {calm: liveClock, soon: icons.hourglass, close: icons.alarm, overdue: icons.ringing, ready: icons.party};
const FLAGS: Partial<Record<Urgency, {icon: string; label: string}>> = {close: {icon: icons.alarm, label: msg("Last hour")}, overdue: {icon: icons.ringing, label: msg("Overdue")}, ready: {icon: icons.party, label: msg("All done")}};

/** The ring that drains as time runs out, around an icon for how close the timer is. */
function dial(kind: Timer["kind"], at: number, now: number, level: Urgency): string {
  const image = kind === "deadline" && level !== "close" ? icons.flag : DIAL_ICONS[level];
  return `<span class="timer-dial" aria-hidden="true"><svg viewBox="0 0 48 48"><circle class="dial-track" cx="24" cy="24" r="20.5"/><circle class="dial-time" cx="24" cy="24" r="20.5" pathLength="100" stroke-dasharray="${dialDash(runway(kind, at, now))}" data-dial/></svg><span class="dial-icon">${image}</span></span>`;
}
const flag = (level: Urgency) => { const shown = FLAGS[level]; return shown ? `<span class="timer-flag">${shown.icon}<span>${t(shown.label)}</span></span>` : ""; };

function taskTimer(timer: TaskTimer, s: Snapshot, now: number, order: number): string {
  const {task, index, at} = timer;
  const progress = progressOf(task);
  const level = urgency("task", at, now, progress === 100);
  const name = e(task.description);
  const type = s.taskTypes[task.type] ?? task.typeName;
  const complete = level === "ready" ? `<button type="button" class="timer-complete" data-task="${index}" data-operation="deleted" aria-label="${t("Complete {name}", {name})}">${icons.check}<span>${t("Complete")}</span></button>` : "";
  return `<article class="timer timer-task ${sw(typeSwatch(s.taskTypes, task.type))} urgency-${level} ${level === "overdue" ? "is-overdue" : ""}" role="listitem" data-at="${at}" data-kind="task" data-urgency="${level}" ${progress === 100 ? "data-ready" : ""} style="--i:${order}">
      <div class="timer-top">${dial("task", at, now, level)}<div class="timer-heading"><div class="timer-head"><span class="timer-kind">${icons.tasks}<span>${t("Task")}</span></span>${flag(level)}<span class="timer-type" title="${e(type || "")}">${e(type || "")}</span></div><h4 title="${name}">${name}</h4></div></div>
      <p class="timer-clock"><time datetime="${e(task.due ?? "")}" data-clock="clock">${clock(at - now)}</time><span class="timer-sub">${icons.due}<span data-clock="word">${clockText("word", at, now)}</span> · <span data-clock="by">${clockText("by", at, now)}</span> ${e(dueLabel(task.due!, now))}</span></p>
      ${timerSteps(task, index)}
      <div class="timer-foot">${progressControl(index, task, "timer-progress")}${complete}</div>
    </article>`;
}

/** A task's subtasks on its timer: how many are done, and the next one, ready to check off. */
function timerSteps(task: TaskItem, index: number): string {
  const steps = task.subtasks ?? [];
  if (!steps.length) return "";
  const done = steps.filter(step => step.done).length;
  const next = steps.findIndex(step => !step.done);
  const count = `<span class="timer-steps-count" title="${t("{done} of {total} subtasks done", {done, total: steps.length})}">${icons.subtasks}<b>${done}/${steps.length}</b></span>`;
  if (next < 0) return `<div class="timer-steps is-done">${count}<span class="timer-next-done">${t("Every subtask is done")}</span></div>`;
  return `<div class="timer-steps">${count}<label class="timer-next" title="${t("Check off the next subtask")}"><input type="checkbox" data-subtask="${index}" data-step="${next}" aria-label="${e(t("Done: {text}", {text: steps[next].text}))}"><span class="subtask-box" aria-hidden="true">${icons.check}</span><span class="timer-next-text"><small>${t("Next")}</small>${e(steps[next].text)}</span></label></div>`;
}

function deadlineTimer(timer: DeadlineTimer, s: Snapshot, now: number, order: number): string {
  const {deadline, at, key} = timer;
  const level = urgency("deadline", at, now);
  const name = e(deadline.event);
  return `<article class="timer timer-deadline ${sw(deadlineSwatch(deadline, s.taskTypes))} urgency-${level}" role="listitem" data-at="${at}" data-kind="deadline" data-urgency="${level}" style="--i:${order}">
      <div class="timer-top">${dial("deadline", at, now, level)}<div class="timer-heading"><div class="timer-head"><span class="timer-kind">${icons.flag}<span>${t("Deadline")}</span></span>${flag(level)}<button type="button" class="timer-close" data-timer-close="${e(key)}" aria-label="${t("Close the timer for {name}", {name})}" title="${t("Close this timer. The deadline stays as it is.")}">${icons.close}</button></div><h4 title="${name}">${name}</h4></div></div>
      <p class="timer-clock"><time datetime="${e(deadline.deadline)}" data-clock="clock">${clock(at - now)}</time><span class="timer-sub">${icons.due}<span data-clock="word">${t("left")}</span> · ${deadline.daysLeft === 0 ? t("due today") : t("due {date}", {date: e(formatDate(deadline.deadline, {weekday: "short", month: "short", day: "numeric"}))})}</span></p>
      ${deadline.typeName ? `<span class="timer-type" title="${e(deadline.typeName)}">${e(deadline.typeName)}</span>` : ""}
    </article>`;
}

/** How far along a task is. Without subtasks it is a slider; every copy of it (on the card and in its timer)
 *  carries the task's index. With subtasks it is a meter that follows them. */
export function progressControl(index: number, task: Pick<TaskItem, "description" | "progress" | "subtasks">, className: string): string {
  const progress = progressOf(task);
  const steps = task.subtasks ?? [];
  const full = progress === 100 ? "is-full" : "";
  if (steps.length) {
    const done = steps.filter(step => step.done).length;
    const summary = tn("{done} of {count} subtask done", "{done} of {count} subtasks done", steps.length, {done});
    return `<div class="progress-control is-auto ${className} ${full}"><span class="progress-label">${t("Progress")}</span><span class="progress-meter" role="progressbar" aria-label="${e(t("Progress of {name}", {name: task.description}))}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${progress}" aria-valuetext="${e(t("{percent}%, {summary}", {percent: progress, summary}))}" style="--p:${(progress / 100).toFixed(2)}"></span><b>${progress}%</b><span class="progress-auto" data-tip="${e(t("Follows its subtasks: {summary}", {summary}))}">${icons.subtasks}</span></div>`;
  }
  return `<label class="progress-control ${className} ${full}"><span class="progress-label">${t("Progress")}</span><input type="range" class="progress-range" name="progress" data-progress="${index}" min="0" max="100" step="5" value="${progress}" style="--p:${(progress / 100).toFixed(2)}" aria-label="${e(t("Progress of {name}", {name: task.description}))}" aria-valuetext="${progress}%"><b>${progress}%</b></label>`;
}
