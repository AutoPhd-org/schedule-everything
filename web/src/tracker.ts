// The tracker plugin: what users build for themselves (guitar practice, Spanish, reading). It records the metrics
// they define (minutes, numbers with a unit, or a note such as the song or the lesson), with an optional weekly
// goal for each number, and a practice timer that fills in the minutes. Every logged session is also a finished
// task of the plugin's type, so it shows in the activity history; its planned tasks are ordinary tasks.
//
// Its data (`plugin.state`): `metrics` ({id, name, unit, kind: "minutes" | "number" | "text", goal, and `total`:
// how a week adds up, "sum", "max" for the best, or "avg"}) and `timer`
// (when the practice timer started, while it runs). Its log: `entry` entries ({values: {metric: value}, note})
// and `task` entries (a task of its type completed from the board).
import { ring } from "./charts";
import { t, tn } from "./i18n";
import {
  bars, localStamp, num, pluginIcon, recentWeeks, saveState, stopwatch, tile, view, weekStart, type PluginHost, type PluginModule
} from "./plugin-kit";
import type { PluginItem, PluginLogEntry, Snapshot } from "./types";
import { addDays, e, empty, formatDate, icons, isoDay } from "./ui";

export type MetricKind = "minutes" | "number" | "text";
/** How a week of a number adds up: its total (minutes, pages), its best (tempo), or its average. */
export type MetricTotal = "sum" | "max" | "avg";
export type Metric = {id: string; name: string; unit: string; kind: MetricKind; goal?: number | null; total?: MetricTotal};

export const metricsOf = (p: PluginItem): Metric[] => (Array.isArray(p.state.metrics) ? p.state.metrics : [])
  .filter((metric: Metric) => metric && typeof metric.id === "string" && typeof metric.name === "string")
  .map((metric: Metric) => ({...metric, kind: ["minutes", "number", "text"].includes(metric.kind) ? metric.kind : "number", unit: metric.kind === "minutes" ? t("min") : String(metric.unit ?? ""), total: metric.kind === "number" && (metric.total === "max" || metric.total === "avg") ? metric.total : "sum"}));
const numeric = (p: PluginItem) => metricsOf(p).filter(metric => metric.kind !== "text");
const entries = (p: PluginItem) => p.log.filter(entry => entry.kind === "entry" || entry.kind === "task");
const valueOf = (entry: PluginLogEntry, metric: Metric) => Number(entry.data?.values?.[metric.id]) || 0;
const runningSince = (p: PluginItem): number | null => Number(p.state.timer?.startedAt) || null;

/** A metric over some entries: their total, their best, or the average of those that record it. */
function combine(list: PluginLogEntry[], metric: Metric): number {
  const values = list.map(entry => valueOf(entry, metric)).filter(value => value > 0);
  if (!values.length) return 0;
  if (metric.total === "max") return Math.max(...values);
  if (metric.total === "avg") return values.reduce((sum, value) => sum + value, 0) / values.length;
  return values.reduce((sum, value) => sum + value, 0);
}
/** This week's value of a metric: its total, best, or average. */
function weekTotal(p: PluginItem, metric: Metric, today: string): number {
  const monday = weekStart(today);
  return combine(entries(p).filter(entry => entry.at.slice(0, 10) >= monday), metric);
}
/** Days in a row with something logged, ending today or yesterday. */
function dayStreak(p: PluginItem, today: string): number {
  const days = new Set(entries(p).map(entry => entry.at.slice(0, 10)));
  let day = days.has(today) ? today : addDays(today, -1);
  let count = 0;
  while (days.has(day)) { count++; day = addDays(day, -1); }
  return count;
}
const amount = (metric: Metric, value: number) => metric.unit ? `${num(value)} ${metric.unit}` : num(value);

function goals(p: PluginItem, today: string, size = 104): string {
  const withGoals = numeric(p).filter(metric => Number(metric.goal) > 0);
  const monday = weekStart(today);
  const thisWeek = entries(p).filter(entry => entry.at.slice(0, 10) >= monday).length;
  const rings = withGoals.map(metric => {
    const total = weekTotal(p, metric, today);
    return `<div class="trk-goal">${ring({value: total / Number(metric.goal), size, label: t("{metric}: {done} of {goal} this week", {metric: metric.name, done: amount(metric, total), goal: amount(metric, Number(metric.goal))}), center: `${num(total, 0)}<small>/${num(Number(metric.goal), 0)}</small>`, tone: "sw-aqua"})}<span>${e(metric.name)}${metric.unit ? ` · ${e(metric.unit)}` : ""}</span></div>`;
  }).join("");
  return `<div class="trk-goals">${rings}<div class="pk-tiles">${tile(t("Streak"), String(dayStreak(p, today)), tn("day", "days", dayStreak(p, today)))}${tile(t("This week"), String(thisWeek), tn("session", "sessions", thisWeek))}${tile(t("All time"), String(entries(p).length), tn("session", "sessions", entries(p).length))}</div></div>`;
}
function charts(p: PluginItem, today: string): string {
  const weeks = recentWeeks(today, 8);
  const labels = weeks.map(week => formatDate(week, {month: "numeric", day: "numeric"}));
  return numeric(p).map(metric => {
    const values = weeks.map(week => combine(entries(p).filter(entry => weekStart(entry.at) === week), metric));
    const how = metric.total === "max" ? t("Best of each week, the last 8 weeks.") : metric.total === "avg" ? t("Average of each week, the last 8 weeks.") : t("Per week, the last 8 weeks.");
    return `<article class="card trk-chart"><div class="section-heading compact"><div><h3>${e(metric.name)}</h3><p class="muted">${how}</p></div></div>${bars({values, labels, tips: values.map((value, i) => `${labels[i]} · ${amount(metric, value)}`), goal: Number(metric.goal) || null, label: metric.name, tone: "sw-aqua"})}</article>`;
  }).join("");
}
function recent(p: PluginItem, limit: number, deletable: boolean): string {
  const list = [...entries(p)].sort((a, b) => b.at.localeCompare(a.at)).slice(0, limit);
  const metrics = metricsOf(p);
  if (!list.length) return empty(t("Nothing logged yet. Your sessions will be listed here."));
  return `<ol class="trk-entries">${list.map((entry, i) => {
    const parts = metrics.map(metric => {
      const raw = entry.data?.values?.[metric.id];
      if (raw === undefined || raw === null || raw === "") return "";
      return metric.kind === "text" ? `<span class="trk-text">${e(String(raw))}</span>` : `<span><b>${num(Number(raw))}</b> ${e(metric.unit || metric.name)}</span>`;
    }).filter(Boolean).join("");
    const label = entry.kind === "task" ? `<span class="trk-task">${icons.check}${e(entry.task ?? "")}</span>` : parts || `<span class="muted">${t("Logged")}</span>`;
    return `<li style="--i:${i}"><time>${e(formatDate(entry.at, {weekday: "short", month: "short", day: "numeric"}))}</time><div class="trk-entry-body">${label}${entry.data?.note ? `<small>${e(entry.data.note)}</small>` : ""}</div>${deletable ? `<button type="button" class="icon-button danger" data-trk-delete="${e(entry.id)}" aria-label="${t("Delete this entry")}">${icons.close}</button>` : ""}</li>`;
  }).join("")}</ol>`;
}

type View = {prefill: Record<string, string>};
const viewOf = (p: PluginItem) => view<View>(p.id, () => ({prefill: {}}));

function logForm(p: PluginItem): string {
  const v = viewOf(p);
  const since = runningSince(p);
  const fields = metricsOf(p).map(metric => metric.kind === "text"
    ? `<label>${e(metric.name)}<input name="m-${e(metric.id)}" maxlength="120" value="${e(v.prefill[metric.id] ?? "")}"></label>`
    : `<label>${e(metric.name)}<span class="trk-number"><input type="number" name="m-${e(metric.id)}" min="0" max="100000" step="any" inputmode="decimal" value="${e(v.prefill[metric.id] ?? "")}">${metric.unit ? `<small>${e(metric.unit)}</small>` : ""}</span></label>`).join("");
  const timer = since
    ? `<div class="trk-timer is-running"><span class="live-dot"></span><time data-since="${since}">${stopwatch(Date.now() - since)}</time><button type="button" class="button small primary" data-trk-stop>${icons.check}<span>${t("Stop and fill in")}</span></button><button type="button" class="text-button" data-trk-cancel>${t("Cancel")}</button></div>`
    : `<div class="trk-timer"><button type="button" class="button small" data-trk-start>${icons.clock}<span>${t("Start the practice timer")}</span></button></div>`;
  return `<article class="card trk-log"><div class="section-heading compact"><div><h2>${icons.plus} ${t("Log a session")}</h2><p class="muted">${t("It also counts as a finished task in your activity.")}</p></div></div>${timer}<form class="trk-form" autocomplete="off">${fields}<label>${t("Note")}<input name="note" maxlength="200" placeholder="${t("How did it go?")}" value="${e(v.prefill.note ?? "")}"></label><button type="submit" class="button primary">${icons.check}<span>${t("Log it")}</span></button></form></article>`;
}
function plannedCard(p: PluginItem, host: PluginHost): string {
  const s: Snapshot = host.snapshot();
  const planned = s.tasks.map((task, index) => ({task, index})).filter(({task}) => task.type === p.type);
  return `<article class="card trk-planned"><div class="section-heading compact"><div><h3>${icons.tasks} ${t("Tasks")} <span class="count">${planned.length}</span></h3><p class="muted">${t("They are tasks in the {type} group on the board.", {type: e(p.name)})}</p></div></div>
    <form class="trk-task-form" autocomplete="off"><input name="trkTask" maxlength="500" placeholder="${t("Something to practice or do…")}" aria-label="${t("New task")}"><button type="submit" class="button small primary">${icons.plus}<span>${t("Add")}</span></button></form>
    ${planned.length ? `<div class="task-cards plugin-tasks">${planned.map(({task, index}, i) => host.taskCard(task, index, i)).join("")}</div>` : empty(t("No tasks planned. Add one above; it shows up on the board."))}</article>`;
}

async function stopTimer(p: PluginItem, host: PluginHost) {
  const since = runningSince(p);
  if (!since) return;
  const minutes = Math.max(1, Math.round((Date.now() - since) / 60000));
  const metric = metricsOf(p).find(item => item.kind === "minutes");
  const v = viewOf(p);
  if (metric) v.prefill[metric.id] = String(minutes);
  await saveState(host, p, {timer: null}, {now: true});
  host.navigate(`plugin:${p.id}`);
  host.notify(metric ? tn("{count} minute of practice. Fill in the rest and log it.", "{count} minutes of practice. Fill in the rest and log it.", minutes) : tn("{count} minute of practice.", "{count} minutes of practice.", minutes));
}
function bindTimerControls(scope: HTMLElement, p: PluginItem, host: PluginHost) {
  scope.querySelectorAll<HTMLButtonElement>("[data-trk-start]").forEach(btn => btn.onclick = () => void host.action(async () => {
    await saveState(host, p, {timer: {startedAt: Date.now()}}, {now: true});
    host.redraw();
  }));
  scope.querySelectorAll<HTMLButtonElement>("[data-trk-stop]").forEach(btn => btn.onclick = () => void host.action(() => stopTimer(p, host)));
  scope.querySelectorAll<HTMLButtonElement>("[data-trk-cancel]").forEach(btn => btn.onclick = () => void host.action(async () => {
    await saveState(host, p, {timer: null}, {now: true});
    host.redraw();
  }));
  scope.querySelectorAll<HTMLButtonElement>("[data-plugin-session]").forEach(btn => btn.onclick = () => host.navigate(`plugin:${p.id}`));
}

export const tracker: PluginModule = {
  kind: "tracker",
  page(p, host) {
    const today = host.snapshot().today.date;
    const metrics = metricsOf(p);
    return `<section class="card trk-hero">${goals(p, today, 112)}${metrics.length ? "" : `<p class="muted">${t("This plugin records no metrics yet. Add some in Settings → Plugins.")}</p>`}</section>
      <div class="trk-grid">${logForm(p)}${plannedCard(p, host)}</div>
      <div class="trk-charts">${charts(p, today)}</div>
      <article class="card trk-recent"><div class="section-heading compact"><h3>${icons.activity} ${t("Recent sessions")}</h3></div>${recent(p, 12, true)}</article>`;
  },
  bindPage(scope, p, host) {
    bindTimerControls(scope, p, host);
    const form = scope.querySelector<HTMLFormElement>(".trk-form");
    const v = viewOf(p);
    if (form) {
      form.addEventListener("input", event => {
        const input = event.target as HTMLInputElement;
        if (input.name === "note") v.prefill.note = input.value;
        else if (input.name.startsWith("m-")) v.prefill[input.name.slice(2)] = input.value;
      });
      form.onsubmit = event => {
        event.preventDefault();
        const data = new FormData(form);
        const values: Record<string, number | string> = {};
        for (const metric of metricsOf(p)) {
          const raw = String(data.get(`m-${metric.id}`) ?? "").trim();
          if (!raw) continue;
          values[metric.id] = metric.kind === "text" ? raw : Math.max(0, Number(raw) || 0);
        }
        const note = String(data.get("note") ?? "").trim();
        if (!Object.keys(values).length && !note) {
          form.classList.remove("shake");
          void form.offsetWidth;
          form.classList.add("shake");
          return;
        }
        const text = metricsOf(p).find(metric => metric.kind === "text" && values[metric.id]);
        const minutesMetric = metricsOf(p).find(metric => metric.kind === "minutes" && values[metric.id]);
        const minutes = minutesMetric ? Number(values[minutesMetric.id]) : 0;
        const description = text ? `${p.name}: ${values[text.id]}` : p.name;
        const end = new Date();
        void host.action(async () => {
          const result = await host.request<{undo: Record<string, unknown>}>("plugin_log", {id: p.id, kind: "entry", data: note ? {values, note} : {values}, asTask: true, description: description.slice(0, 500), priority: 5, at: localStamp(end), startedAt: localStamp(new Date(end.getTime() - Math.min(minutes, 24 * 60) * 60000))});
          v.prefill = {};
          await host.reload();
          host.offerUndo(t("Logged a session of {name}.", {name: p.name}), async () => { await host.request("plugin_log_delete", result.undo); await host.reload(); });
        });
      };
    }
    const taskForm = scope.querySelector<HTMLFormElement>(".trk-task-form");
    if (taskForm) taskForm.onsubmit = event => {
      event.preventDefault();
      const input = taskForm.querySelector<HTMLInputElement>('[name="trkTask"]')!;
      const description = input.value.trim();
      if (!description || !p.type) { input.focus(); return; }
      if (host.snapshot().tasks.some(task => task.description === description) && !window.confirm(t("“{task}” is already on your list. Replace it with this one?", {task: description}))) return;
      void host.action(async () => {
        await host.request("task_add", {description, priority: 5, type: p.type, postpone: 0});
        await host.reload();
        host.notify(t("Added “{task}” to {type}.", {task: description, type: p.name}));
      });
    };
    scope.querySelectorAll<HTMLButtonElement>("[data-trk-delete]").forEach(btn => btn.onclick = () => {
      if (!window.confirm(t("Delete this entry from the log?"))) return;
      void host.action(async () => {
        await host.request("plugin_log_delete", {id: p.id, entry: btn.dataset.trkDelete});
        await host.reload();
        host.notify(t("The entry is off the log."));
      });
    });
  },
  activity(p, now) {
    const today = isoDay(new Date(now));
    return `<section class="plugin-activity trk-activity"><article class="card trk-hero">${goals(p, today, 96)}</article><div class="trk-charts">${charts(p, today)}</div><article class="card trk-recent"><div class="section-heading compact"><h3>${icons.activity} ${t("Recent sessions")}</h3></div>${recent(p, 8, false)}</article></section>`;
  },
  timer(p, s, now) {
    const since = runningSince(p);
    const icon = pluginIcon(p);
    const main = numeric(p).find(metric => Number(metric.goal) > 0);
    const head = `<div class="timer-top"><span class="timer-dial plugin-dial" aria-hidden="true"><span class="dial-icon">${icon}</span></span><div class="timer-heading"><div class="timer-head"><span class="timer-kind">${icon}<span>${e(p.name)}</span></span>${since ? `<span class="timer-flag">${icons.zap}<span>${t("Live")}</span></span>` : ""}</div>`;
    if (since) return `<article class="timer timer-plugin plugin-tracker is-live" role="listitem">${head}<h4>${t("Practicing")}</h4></div></div><p class="timer-clock"><time data-since="${since}">${stopwatch(now - since)}</time><span class="timer-sub">${t("since {time}", {time: formatDate(localStamp(new Date(since)), {hour: "numeric", minute: "2-digit"})})}</span></p><div class="timer-foot"><button type="button" class="timer-complete" data-trk-stop>${icons.check}<span>${t("Stop and log")}</span></button></div></article>`;
    const total = main ? weekTotal(p, main, s.today.date) : 0;
    const goal = main ? Number(main.goal) : 0;
    return `<article class="timer timer-plugin plugin-tracker" role="listitem">${head}<h4>${main ? e(t("{done} of {goal} this week", {done: amount(main, total), goal: amount(main, goal)})) : e(p.name)}</h4></div></div><p class="timer-clock plugin-clock-text">${main ? `<span class="timer-progress-mini wide"><i style="--p:${Math.min(1, total / goal).toFixed(3)}"></i></span>` : ""}<span class="timer-sub">${tn("{count}-day streak", "{count}-day streak", dayStreak(p, s.today.date))}</span></p><div class="timer-foot"><button type="button" class="timer-complete" data-trk-start>${icons.clock}<span>${t("Start practice")}</span></button></div></article>`;
  },
  bindTimer: bindTimerControls,
  describeData() { return ""; }
};
