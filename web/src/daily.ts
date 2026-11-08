// Daily overview on the Overview page: your day so far. What you finished and when (hour by hour, by type),
// against yesterday and your usual day; how much open work is left (workload.ts) and how far along today's work
// is; your streak and habits; and the most important things you finished. The finished-work numbers come from
// the server (daily_overview); workload, progress, and habits come from the snapshot, so they follow every
// change at once. Folded, the panel keeps to one line. Rendering is pure; render.ts loads the numbers and binds
// the fold button.
import { artwork, delta, ring } from "./charts";
import { progressOf, todayProgress } from "./timers";
import type { DailyOverview, Snapshot, TaskItem } from "./types";
import { e, formatDate, formatHours, icons, sw, typeSwatch } from "./ui";
import { describeLoad, isPostponed, LOAD_LEVELS, LOAD_SCALE, levelInfo, taskLoad, WORKLOAD_DEFINITION, workload } from "./workload";

export type DayPanelOptions = {overview: DailyOverview | null; snapshot: Snapshot; now: number; weighted: boolean; folded: boolean};

const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;
const sum = (counts: Record<string, number>) => Object.values(counts).reduce((total, count) => total + count, 0);

/** The average progress of the open tasks that count today, each weighted by its importance when `weighted`. */
export function openProgress(tasks: readonly TaskItem[], today: string, weighted: boolean): number | null {
  const counted = tasks.filter(task => !isPostponed(task, today));
  const weight = (task: TaskItem) => weighted ? Math.max(1, Math.min(10, task.priority)) : 1;
  const total = counted.reduce((sum, task) => sum + weight(task), 0);
  return total ? counted.reduce((sum, task) => sum + weight(task) * progressOf(task), 0) / (total * 100) : null;
}

export function dayPanel({overview, snapshot: s, now, weighted, folded}: DayPanelOptions): string {
  // Numbers for another day (the page stayed open past midnight) wait for the next load.
  const day = overview && overview.date === s.today.date ? overview : null;
  const load = workload(s.tasks, s.today.date);
  const info = levelInfo(load.level);
  const timed = todayProgress(s, now, weighted);
  const percent = (value: number) => `${Math.round(value * 100)}%`;
  const head = `<header class="day-head">
      <div class="day-title"><p class="eyebrow">${icons.spark} DAILY OVERVIEW</p><h2>Your day so far<span class="accent-dot">.</span></h2></div>
      <span class="pill">${e(formatDate(s.today.date, {weekday: "long", month: "long", day: "numeric"}))}</span>
      <button type="button" class="day-fold icon-button" data-day-fold aria-expanded="${!folded}" aria-controls="day-body" title="${folded ? "Show the daily overview" : "Fold into one line"}">${icons.chevron}<span class="sr-only">${folded ? "Show the daily overview" : "Fold the daily overview"}</span></button>
    </header>`;
  if (folded) {
    const chips = [
      `<span class="day-chip">${icons.check}<b>${day ? day.completed : "–"}</b> finished</span>`,
      `<span class="day-chip load-${load.level}">${info.icon}<b>${load.points.toFixed(1)}</b> ${e(info.label.toLowerCase())} load</span>`,
      timed.parts.length ? `<span class="day-chip">${icons.target}<b>${percent(timed.value)}</b> of timed work</span>` : "",
      day?.streak ? `<span class="day-chip">${icons.flame}<b>${day.streak}</b>-day streak</span>` : ""
    ].join("");
    return `<section id="day-panel" class="day-panel is-folded" aria-label="Daily overview">${head}<p class="day-strip">${chips}</p></section>`;
  }
  return `<section id="day-panel" class="day-panel" aria-label="Daily overview">${head}
      <div class="day-grid" id="day-body">${finishedTile(day, s, now)}${loadTile(s)}${progressTile(s, now, weighted)}${highlightsTile(day, s)}${streakTile(day, s)}</div>
    </section>`;
}

function finishedTile(day: DailyOverview | null, s: Snapshot, now: number): string {
  if (!day) return `<article class="dp-tile dp-hero loading-shimmer" aria-busy="true"><p class="dp-label">${icons.check}Finished today</p><p class="muted">Gathering your day…</p></article>`;
  const comparisons = [
    day.yesterday === null ? "" : delta(day.completed, day.yesterday, {label: "yesterday"}),
    day.usual === null ? "" : delta(day.completed, day.usual, {label: "your usual day", digits: 1})
  ].join("");
  const outcomes = [
    `<span class="dp-chip">${icons.plus}${day.added} added</span>`,
    day.cancelled ? `<span class="dp-chip sw-yellow">${icons.ban}${day.cancelled} cancelled</span>` : "",
    day.dropped ? `<span class="dp-chip sw-magenta">${icons.dropped}${day.dropped} dropped</span>` : "",
    day.medianLeadHours === null ? "" : `<span class="dp-chip" title="From adding a task to finishing it, the middle value today">${icons.hourglass}${formatHours(day.medianLeadHours)} to finish</span>`
  ].join("");
  return `<article class="dp-tile dp-hero"><p class="dp-label">${icons.check}Finished today</p>
      <div class="dp-number"><strong>${day.completed}</strong>${comparisons ? `<div class="dp-deltas">${comparisons}</div>` : `<p class="muted">Your first day here. Tomorrow you can compare.</p>`}</div>
      ${hourChart(day, s, now)}
      <div class="dp-outcomes">${outcomes}</div>
    </article>`;
}

/** Tasks finished in each hour of the day, stacked by type; hours still to come are faint. */
function hourChart(day: DailyOverview, s: Snapshot, now: number): string {
  const current = day.isToday ? new Date(now).getHours() : 24;
  const max = Math.max(1, ...day.hours.map(sum));
  const types = [...new Set(day.hours.flatMap(hour => Object.keys(hour)))].sort((a, b) => a.localeCompare(b, undefined, {numeric: true}));
  const name = (id: string) => day.taskTypes[id] ?? s.taskTypes[id] ?? id;
  const hourName = (hour: number) => formatDate(`${s.today.date}T${String(hour).padStart(2, "0")}:00`, {hour: "numeric"});
  const bars = day.hours.map((counts, hour) => {
    const total = sum(counts);
    const segments = types.filter(id => counts[id]).map(id => `<i class="${sw(typeSwatch(s.taskTypes, id))}" style="--h:${(counts[id] / max).toFixed(3)}"></i>`).join("");
    const tip = `${hourName(hour)}: ${total ? `${plural(total, "task")} finished${types.length > 1 ? ` (${types.filter(id => counts[id]).map(id => `${name(id)} ${counts[id]}`).join(", ")})` : ""}` : hour > current ? "still to come" : "nothing finished"}`;
    return `<span class="hr ${hour > current ? "future" : ""} ${hour === current ? "is-now" : ""}" style="--i:${hour}" data-tip="${e(tip)}">${segments}</span>`;
  }).join("");
  const legend = types.length ? `<div class="legend">${types.map(id => `<span class="${sw(typeSwatch(s.taskTypes, id))}"><i></i>${e(name(id))}</span>`).join("")}</div>` : "";
  return `<div class="hours" role="img" aria-label="${e(`Tasks finished per hour: ${day.hours.map((counts, hour) => sum(counts) ? `${hourName(hour)} ${sum(counts)}` : "").filter(Boolean).join(", ") || "none yet"}`)}">${bars}</div>
    <div class="hours-axis" aria-hidden="true">${[0, 6, 12, 18].map(hour => `<span>${e(hourName(hour))}</span>`).join("")}</div>${legend}`;
}

/** A half-dial from no workload to past Heavy, with ticks where each level begins, and the load by type. */
function loadTile(s: Snapshot): string {
  const load = workload(s.tasks, s.today.date);
  const info = levelInfo(load.level);
  const point = (value: number, radius: number) => {
    const angle = Math.PI * (1 - Math.min(value, LOAD_SCALE) / LOAD_SCALE);
    return [60 + radius * Math.cos(angle), 62 - radius * Math.sin(angle)].map(n => n.toFixed(1));
  };
  const ticks = LOAD_LEVELS.slice(1).map(level => {
    const start = LOAD_LEVELS[LOAD_LEVELS.indexOf(level) - 1].below;
    const [x1, y1] = point(start, 44);
    const [x2, y2] = point(start, 56);
    return `<line class="gauge-tick" x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}"/>`;
  }).join("");
  const byType = new Map<string, number>();
  for (const task of s.tasks) byType.set(task.type, (byType.get(task.type) ?? 0) + taskLoad(task, s.today.date));
  const types = [...byType].filter(([, points]) => points > 0).sort((a, b) => b[1] - a[1]);
  const typeName = (id: string) => s.taskTypes[id] ?? s.tasks.find(task => task.type === id)?.typeName ?? id;
  const split = types.length ? `<div class="load-split" role="img" aria-label="${e(`Workload by type: ${types.map(([id, points]) => `${typeName(id)} ${points.toFixed(1)}`).join(", ")}`)}">${types.map(([id, points]) => `<i class="${sw(typeSwatch(s.taskTypes, id))}" style="flex-grow:${points.toFixed(2)}" data-tip="${e(`${typeName(id)}: ${points.toFixed(1)}`)}"></i>`).join("")}</div>
      <div class="legend">${types.slice(0, 4).map(([id, points]) => `<span class="${sw(typeSwatch(s.taskTypes, id))}"><i></i>${e(typeName(id))} <b>${points.toFixed(1)}</b></span>`).join("")}</div>` : "";
  return `<article class="dp-tile dp-load load-${load.level}"><p class="dp-label">${icons.gauge}Workload <span class="dp-help" data-tip="${e(WORKLOAD_DEFINITION)}" tabindex="0" aria-label="${e(WORKLOAD_DEFINITION)}">?</span></p>
      <div class="gauge" role="img" aria-label="${e(describeLoad(load))}"><svg viewBox="0 0 120 70" aria-hidden="true"><path class="gauge-track" d="M10 62 A50 50 0 0 1 110 62"/><path class="gauge-fill" d="M10 62 A50 50 0 0 1 110 62" pathLength="100" stroke-dasharray="${(Math.min(load.points, LOAD_SCALE) / LOAD_SCALE * 100).toFixed(2)} 100"/>${ticks}</svg>
        <div class="gauge-read"><span class="gauge-icon">${info.icon}</span><strong>${load.points.toFixed(1)}</strong><span>${e(info.label)}</span></div></div>
      <p class="dp-note">${load.counted ? `${plural(load.counted, "open task")} · importance ${load.importance} on average · ${load.done}% done` : "No open tasks."}</p>
      ${split}<p class="dp-fine">${e(info.note)}</p>
    </article>`;
}

function progressTile(s: Snapshot, now: number, weighted: boolean): string {
  const timed = todayProgress(s, now, weighted);
  const open = openProgress(s.tasks, s.today.date, weighted);
  const steps = s.tasks.flatMap(task => task.subtasks ?? []);
  const checked = steps.filter(step => step.done).length;
  const percent = (value: number | null) => value === null ? "–" : `${Math.round(value * 100)}%`;
  return `<article class="dp-tile dp-progress"><p class="dp-label">${icons.target}Progress</p>
      <div class="dp-rings">
        <div class="dp-ring">${ring({value: timed.parts.length ? timed.value : null, size: 86, label: `Timed work due today: ${timed.parts.length ? percent(timed.value) : "none"}`, tone: "sw-violet"})}<span>Timed work due today</span></div>
        <div class="dp-ring">${ring({value: open, size: 86, label: `All open tasks: ${percent(open)} done on average`, tone: "sw-aqua"})}<span>All open tasks</span></div>
      </div>
      ${steps.length ? `<p class="dp-note">${icons.subtasks}${checked} of ${plural(steps.length, "subtask")} checked off</p>` : ""}
      <p class="dp-fine">${weighted ? `${icons.scale}Weighted by importance` : "Every task counts the same"} · <button type="button" class="text-button" data-page="settings">Change</button></p>
    </article>`;
}

function highlightsTile(day: DailyOverview | null, s: Snapshot): string {
  const items = day?.highlights ?? [];
  const list = items.length
    ? `<div class="highlights">${items.map((item, i) => `<div class="highlight ${sw(typeSwatch(s.taskTypes, item.type))}" style="--i:${i}"><div class="hl-art">${artwork(item.description, typeSwatch(s.taskTypes, item.type))}<span class="hl-rank">${i + 1}</span></div><div><strong>${e(item.description)}</strong><small>${e(formatDate(item.endedAt, {hour: "numeric", minute: "2-digit"}))} · importance ${item.priority}</small></div></div>`).join("")}</div>`
    : `<p class="dp-empty">${day ? "Finish a task and it shows up here, the most important first." : "Loading…"}</p>`;
  return `<article class="dp-tile dp-highlights"><p class="dp-label">${icons.trophy}Today’s highlights</p>${list}</article>`;
}

function streakTile(day: DailyOverview | null, s: Snapshot): string {
  const streak = day?.streak ?? 0;
  const done = s.habits.filter(habit => habit.completed).length;
  const habits = s.habits.length
    ? `<div class="dp-habits">${ring({value: done / s.habits.length, size: 54, label: `${done} of ${s.habits.length} habits done today`, center: `${done}/${s.habits.length}`, tone: "sw-green"})}<span>${done === s.habits.length ? "Every habit done today." : `${s.habits.length - done} ${s.habits.length - done === 1 ? "habit" : "habits"} to go.`}</span></div>`
    : `<p class="dp-fine">Add habits in Settings to track them here.</p>`;
  return `<article class="dp-tile dp-streak ${streak ? "lit" : ""}"><p class="dp-label">${icons.flame}Streak</p>
      <div class="streak-flame">${icons.flame}<strong>${day ? streak : "–"}</strong></div><p class="dp-fine">${streak === 1 ? "day" : "days"} in a row with a finished task</p>${habits}</article>`;
}
