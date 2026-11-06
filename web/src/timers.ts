// Timers above the task board. Task timers count down to the finish-by time of timed tasks due by tonight
// (overdue ones count up); deadline timers count down to deadlines ending within the next 24 hours, and can be
// closed without touching the deadline. On top, today's timed work fills one bar with the progress of every
// timed task due today, finished ones included. Rendering is pure (state in, HTML out); render.ts ticks the
// clocks every second and binds the controls.
import { deadlineSwatch } from "./calendar";
import type { DeadlineItem, Snapshot, TaskItem } from "./types";
import { e, formatDate, icons, isoDay, parseLocal, sw, typeSwatch } from "./ui";

const DAY = 86_400_000;

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
export const progressOf = (task: Pick<TaskItem, "progress">) => Math.max(0, Math.min(100, Math.round(task.progress ?? 0)));

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

export type TodayPart = {description: string; progress: number; done: boolean; overdue: boolean};
export type TodayProgress = {value: number; parts: TodayPart[]; done: number; overdue: number};

/** Today's timed work: every timed task due by tonight, open or finished today. Finished tasks count as 100%,
 *  so finishing one never lowers the bar. */
export function todayProgress(s: Snapshot, now: number): TodayProgress {
  const end = tonight(now);
  const today = isoDay(new Date(now));
  const finished: TodayPart[] = (s.timedDone ?? [])
    .filter(item => item.endedAt.slice(0, 10) === today && parseLocal(item.due).getTime() < end)
    .map(item => ({description: item.description, progress: 100, done: true, overdue: false}));
  const open: TodayPart[] = s.tasks
    .filter(task => { const at = dueAt(task); return at !== null && at < end; })
    .sort((a, b) => dueAt(a)! - dueAt(b)!)
    .map(task => ({description: task.description, progress: progressOf(task), done: false, overdue: isOverdue(task, now)}));
  const parts = [...finished, ...open];
  const value = parts.length ? parts.reduce((sum, part) => sum + part.progress, 0) / (parts.length * 100) : 0;
  return {value, parts, done: finished.length, overdue: open.filter(part => part.overdue).length};
}

const pad = (value: number) => String(value).padStart(2, "0");
/** A running clock: `02:14:05`, or days and hours once it is a day or more. */
export function clock(ms: number): string {
  const total = Math.floor(Math.abs(ms) / 1000);
  const days = Math.floor(total / 86400);
  const hours = Math.floor(total % 86400 / 3600);
  if (days) return `${days}d ${hours}h`;
  return `${pad(hours)}:${pad(Math.floor(total % 3600 / 60))}:${pad(total % 60)}`;
}
/** A short span for task cards: `2h 14m`, `45m`, `3d 2h`. */
export function span(ms: number): string {
  const minutes = Math.floor(Math.abs(ms) / 60000);
  if (minutes < 1) return "<1m";
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor(minutes % 1440 / 60);
  return days ? `${days}d ${hours}h` : hours ? `${hours}h ${pad(minutes % 60)}m` : `${minutes}m`;
}
/** The text of a ticking element, by its `data-clock`: a "clock" or a "short" span of time, or the words around
 *  it, which change once the time has passed ("word": left or overdue; "by": by or was due; "due": Due or Was due). */
export function clockText(kind: string | undefined, at: number, now: number): string {
  const ahead = at > now;
  if (kind === "word") return ahead ? "left" : "overdue";
  if (kind === "by") return ahead ? "by" : "was due";
  if (kind === "due") return ahead ? "Due" : "Was due";
  return kind === "short" ? span(at - now) : clock(at - now);
}

/** When a timed task is due, relative to today: a time today, or the day and time. */
export function dueLabel(due: string, now: number): string {
  const day = due.slice(0, 10);
  const today = isoDay(new Date(now));
  const time = formatDate(due, {hour: "numeric", minute: "2-digit"});
  const offset = Math.round((parseLocal(`${day}T12:00`).getTime() - parseLocal(`${today}T12:00`).getTime()) / DAY);
  if (offset === 0) return time;
  if (offset === 1) return `tomorrow ${time}`;
  if (offset === -1) return `yesterday ${time}`;
  return formatDate(due, {weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit"});
}

export type TimerOptions = {snapshot: Snapshot; now: number; closed: ReadonlySet<string>};

export function timerStrip({snapshot: s, now, closed}: TimerOptions): string {
  const timers = collectTimers(s, now, closed);
  const tasks = timers.filter(timer => timer.kind === "task").length;
  const row = timers.length
    ? `<div class="timer-row" role="list" aria-label="${tasks} task ${tasks === 1 ? "timer" : "timers"} and ${timers.length - tasks} deadline ${timers.length - tasks === 1 ? "timer" : "timers"}">${timers.map((timer, i) => timer.kind === "task" ? taskTimer(timer, s, now, i) : deadlineTimer(timer, s, now, i)).join("")}</div>`
    : `<p class="timer-empty">${icons.clock}<span>No timers right now. Make a task timed from its card or with <b>New task</b>; deadlines show here in their last 24 hours.</span></p>`;
  return `<section class="timers" aria-label="Timers">${todayBar(todayProgress(s, now))}${row}</section>`;
}

function todayBar(today: TodayProgress): string {
  const percent = Math.round(today.value * 100);
  const total = today.parts.length;
  const meta = total
    ? [`${today.done} of ${total} done`, today.overdue ? `<span class="today-overdue">${today.overdue} overdue</span>` : ""].filter(Boolean).join(" · ")
    : "No timed tasks due today";
  const segments = total
    ? today.parts.map(part => `<i class="${part.done ? "is-done" : ""} ${part.overdue ? "is-overdue" : ""}" style="--p:${(part.progress / 100).toFixed(2)}" title="${e(`${part.description} · ${part.done ? "done" : `${part.progress}%`}`)}"></i>`).join("")
    : `<i class="is-blank" style="--p:0"></i>`;
  return `<div class="today-progress ${total ? "" : "is-idle"} ${percent === 100 ? "is-complete" : ""}">
      <div class="today-top"><span class="today-label">${icons.target}<span>Today’s timed work</span></span><span class="today-meta">${meta}</span><strong class="today-value">${percent}<small>%</small></strong></div>
      <div class="today-bar" role="progressbar" aria-label="Today’s timed work" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent}" aria-valuetext="${e(total ? `${percent}% · ${today.done} of ${total} timed tasks done` : "No timed tasks due today")}">${segments}</div>
    </div>`;
}

function taskTimer(timer: TaskTimer, s: Snapshot, now: number, order: number): string {
  const {task, index, at, overdue} = timer;
  const progress = progressOf(task);
  const name = e(task.description);
  const type = s.taskTypes[task.type] ?? task.typeName;
  return `<article class="timer timer-task ${sw(typeSwatch(s.taskTypes, task.type))} ${overdue ? "is-overdue" : ""}" role="listitem" data-at="${at}" data-kind="task" style="--i:${order}">
      <div class="timer-head"><span class="timer-kind">${icons.clock}<span>Task</span></span><span class="timer-type">${e(type || "")}</span></div>
      <h4 title="${name}">${name}</h4>
      <p class="timer-clock"><time datetime="${e(task.due ?? "")}" data-clock="clock">${clock(at - now)}</time><span class="timer-sub"><span data-clock="word">${clockText("word", at, now)}</span> · <span data-clock="by">${clockText("by", at, now)}</span> ${e(dueLabel(task.due!, now))}</span></p>
      ${progressControl(index, progress, task.description, "timer-progress")}
    </article>`;
}

function deadlineTimer(timer: DeadlineTimer, s: Snapshot, now: number, order: number): string {
  const {deadline, at, key} = timer;
  const name = e(deadline.event);
  return `<article class="timer timer-deadline ${sw(deadlineSwatch(deadline, s.taskTypes))}" role="listitem" data-at="${at}" data-kind="deadline" style="--i:${order}">
      <div class="timer-head"><span class="timer-kind">${icons.flag}<span>Deadline</span></span><button type="button" class="timer-close" data-timer-close="${e(key)}" aria-label="Close the timer for ${name}" title="Close this timer. The deadline stays as it is.">${icons.close}</button></div>
      <h4 title="${name}">${name}</h4>
      <p class="timer-clock"><time datetime="${e(deadline.deadline)}" data-clock="clock">${clock(at - now)}</time><span class="timer-sub"><span data-clock="word">left</span> · due ${deadline.daysLeft === 0 ? "today" : e(formatDate(deadline.deadline, {weekday: "short", month: "short", day: "numeric"}))}</span></p>
      ${deadline.typeName ? `<span class="timer-type">${e(deadline.typeName)}</span>` : ""}
    </article>`;
}

/** A slider for how far along a task is. Every copy of it (on the card and in its timer) carries the task's index. */
export function progressControl(index: number, progress: number, description: string, className: string): string {
  return `<label class="progress-control ${className} ${progress === 100 ? "is-full" : ""}"><span class="progress-label">Progress</span><input type="range" class="progress-range" name="progress" data-progress="${index}" min="0" max="100" step="5" value="${progress}" style="--p:${(progress / 100).toFixed(2)}" aria-label="${e(`Progress of ${description}`)}" aria-valuetext="${progress}%"><b>${progress}%</b></label>`;
}
