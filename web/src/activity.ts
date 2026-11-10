// Activity page: an interactive gallery of finished tasks and a visual weekly overview with history comparisons.
// Rendering is pure (state in, HTML out); render.ts owns data loading and event binding.
import { artwork, delta, pctText, ring } from "./charts";
import { msg, t, tn } from "./i18n";
import type { HistoryItem, WeekDay, WeeklyOverview } from "./types";
import { addDays, e, empty, formatDate, formatHours, icons, parseLocal, sw, typeSwatch } from "./ui";

export type HistoryStatus = "all" | "completed" | "cancelled" | "dropped";
export type HistorySort = "newest" | "oldest" | "priority" | "longest";
export type HistoryLayout = "tree" | "gallery";
export type GalleryFilter = {status: HistoryStatus; type: string | null; query: string; day: string | null; sort: HistorySort};
export const defaultGalleryFilter = (): GalleryFilter => ({status: "all", type: null, query: "", day: null, sort: "newest"});

/** Outcome names as message keys: draw them through t(). */
export const STATUS_LABEL = {completed: msg("Completed"), cancelled: msg("Cancelled"), dropped: msg("Dropped")} as const;
export const STATUS_ICON = {completed: icons.check, cancelled: icons.ban, dropped: icons.dropped} as const;
const OUTCOME_TONE = {completed: "sw-aqua", cancelled: "sw-yellow", dropped: "sw-magenta"} as const;
type Outcome = keyof typeof STATUS_LABEL;

export const statusOf = (item: HistoryItem): Outcome => (item.status === "cancelled" || item.status === "dropped") ? item.status : "completed";
export const leadHours = (item: HistoryItem) => Math.max(0, (parseLocal(item.endedAt).getTime() - parseLocal(item.startedAt).getTime()) / 3600000);
const weekStartOf = (iso: string) => addDays(iso, -((parseLocal(iso).getDay() + 6) % 7));
const short = (iso: string) => formatDate(iso, {month: "short", day: "numeric"});
const weekday = (iso: string, style: "short" | "long" = "short") => formatDate(iso, {weekday: style});

export function filterHistory(items: HistoryItem[], filter: GalleryFilter): HistoryItem[] {
  const query = filter.query.trim().toLowerCase();
  const matches = items.filter(item =>
    (filter.status === "all" || statusOf(item) === filter.status)
    && (!filter.type || item.type === filter.type)
    && (!query || item.description.toLowerCase().includes(query))
    && (!filter.day || item.endedAt.slice(0, 10) === filter.day));
  const newest = (a: HistoryItem, b: HistoryItem) => b.endedAt.localeCompare(a.endedAt);
  const sorters: Record<HistorySort, (a: HistoryItem, b: HistoryItem) => number> = {
    newest,
    oldest: (a, b) => -newest(a, b),
    priority: (a, b) => b.priority - a.priority || newest(a, b),
    longest: (a, b) => leadHours(b) - leadHours(a) || newest(a, b)
  };
  return matches.sort(sorters[filter.sort]);
}

export function groupHistory(items: HistoryItem[], today: string, sort: HistorySort): {label: string; items: HistoryItem[]}[] {
  if (sort === "priority" || sort === "longest") return items.length ? [{label: sort === "priority" ? t("Most important first") : t("Longest journeys first"), items}] : [];
  const thisWeek = weekStartOf(today);
  const lastWeek = addDays(thisWeek, -7);
  const groups = new Map<string, HistoryItem[]>();
  for (const item of items) {
    const day = item.endedAt.slice(0, 10);
    const label = day >= thisWeek ? t("This week") : day >= lastWeek ? t("Last week") : formatDate(day, {month: "long", year: "numeric"});
    if (!groups.has(label)) groups.set(label, []);
    groups.get(label)!.push(item);
  }
  return [...groups].map(([label, entries]) => ({label, items: entries}));
}

const priorityDots = (priority: number) => `<span class="dots" role="img" aria-label="${t("Importance {priority} of 10", {priority})}">${Array.from({length: 10}, (_, i) => `<i class="${i < priority ? "on" : ""}"></i>`).join("")}</span>`;

function galleryCard(item: HistoryItem, index: number, taskTypes: Record<string, string>): string {
  const status = statusOf(item);
  const tone = typeSwatch(taskTypes, item.type ?? null);
  const size = status !== "completed" ? "s" : item.priority >= 8 ? "l" : "m";
  return `<article class="gallery-card size-${size} status-${status} ${sw(tone)}" data-history="${index}" tabindex="0" role="button" aria-haspopup="dialog" aria-label="${e(item.description)} · ${t(STATUS_LABEL[status])} ${e(short(item.endedAt))}">
    <div class="gc-art">${artwork(item.description + item.startedAt, tone, status !== "completed")}<span class="gc-status">${STATUS_ICON[status]}${t(STATUS_LABEL[status])}</span></div>
    <div class="gc-body"><h4>${e(item.description)}</h4><p class="gc-meta"><time datetime="${e(item.endedAt)}">${e(short(item.endedAt))}</time><span>${icons.hourglass}${formatHours(leadHours(item))}</span></p><div class="gc-foot"><span class="gc-type"><i></i>${e(item.typeName ?? t("Task"))}</span>${priorityDots(item.priority)}</div></div>
    <span class="gc-shine" aria-hidden="true"></span></article>`;
}

/** A GitHub-style rhythm grid of completed tasks; clicking a day filters the gallery. */
export function rhythm(items: HistoryItem[], today: string, selected: string | null, weeks = 22): string {
  const counts = new Map<string, number>();
  for (const item of items) if (statusOf(item) === "completed") counts.set(item.endedAt.slice(0, 10), (counts.get(item.endedAt.slice(0, 10)) ?? 0) + 1);
  const start = addDays(weekStartOf(today), -(weeks - 1) * 7);
  const level = (count: number) => count === 0 ? 0 : count === 1 ? 1 : count === 2 ? 2 : count <= 4 ? 3 : 4;
  let total = 0;
  let best: [string, number] | null = null;
  const months: string[] = [];
  const cells: string[] = [];
  for (let w = 0; w < weeks; w++) {
    const monday = addDays(start, w * 7);
    const firstOfMonth = Array.from({length: 7}, (_, d) => addDays(monday, d)).find(day => day.endsWith("-01"));
    months.push(`<span>${w === 0 || firstOfMonth ? e(formatDate(firstOfMonth ?? monday, {month: "short"})) : ""}</span>`);
    for (let d = 0; d < 7; d++) {
      const day = addDays(monday, d);
      if (day > today) { cells.push(`<span class="heat-cell future" style="--d:${w}"></span>`); continue; }
      const count = counts.get(day) ?? 0;
      total += count;
      if (count && (!best || count > best[1])) best = [day, count];
      const label = t("{day}: {count} completed", {day: short(day), count});
      cells.push(`<button type="button" class="heat-cell l${level(count)} ${day === selected ? "selected" : ""} ${day === today ? "is-today" : ""}" style="--d:${w}" data-heat-day="${day}" data-tip="${e(label)}" aria-label="${e(label)}" aria-pressed="${day === selected}"></button>`);
    }
  }
  return `<div class="rhythm" style="--weeks:${weeks}"><div class="rhythm-months" aria-hidden="true">${months.join("")}</div><div class="rhythm-grid">${cells.join("")}</div></div>
    <div class="rhythm-foot"><span>${t("{total} completed in {weeks} weeks", {total, weeks})}${best ? ` · ${t("busiest {date}", {date: e(short(best[0]))})}` : ""}</span><span class="heat-legend" aria-hidden="true">${t("Less")}<i class="heat-cell l0"></i><i class="heat-cell l1"></i><i class="heat-cell l2"></i><i class="heat-cell l3"></i><i class="heat-cell l4"></i>${t("More")}</span></div>`;
}

export function weekEntry(overview: WeeklyOverview | null): string {
  if (!overview) return `<div class="week-entry loading-shimmer" aria-busy="true"><span class="eyebrow">${t("WEEKLY OVERVIEW")}</span><strong class="we-title">${t("Gathering your week…")}</strong></div>`;
  const max = Math.max(1, ...overview.days.map(day => day.completed));
  return `<button type="button" class="week-entry" data-action="open-week"><span class="eyebrow">${icons.spark} ${t("WEEKLY OVERVIEW")}</span><strong class="we-title">${e(short(overview.weekStart))} – ${e(short(overview.weekEnd))}</strong>
    <span class="we-big">${overview.current.completed}<small>${t("completed")}</small></span>${delta(overview.current.completed, overview.previous.completed, {label: t("last week")})}
    <span class="mini-bars" aria-hidden="true">${overview.days.map(day => `<i class="${day.future ? "future" : ""}" style="--h:${day.completed / max}"></i>`).join("")}</span>
    <span class="we-cta">${t("Open your week")} ${icons.arrow}</span><span class="we-blob" aria-hidden="true"></span></button>`;
}

/** The history page: weekly entry and rhythm on top, then filters and the history itself, drawn as a tree or a gallery.
 *  `region` is the rendered history (see galleryGroups and tree.ts's treeView). With a `categoryBar` (plugins.ts),
 *  Activity can be viewed by one category (`filter.type`): `categoryPanel` then takes the weekly entry's place,
 *  and the rhythm counts only that category. */
export function galleryView(options: {items: HistoryItem[] | null; overview: WeeklyOverview | null; filter: GalleryFilter; layout: HistoryLayout; region: string; today: string; taskTypes: Record<string, string>; categoryBar?: string; categoryPanel?: string}): string {
  const {items, overview, filter, layout, today, taskTypes} = options;
  if (!items) return `<section class="activity-top">${weekEntry(overview)}<article class="card rhythm-card loading-shimmer" aria-busy="true"><h2>${t("Your rhythm")}</h2><p class="muted">${t("Loading your history…")}</p></article></section>`;
  const byCategory = !!options.categoryBar && !!filter.type;
  const counts = {all: items.length, completed: 0, cancelled: 0, dropped: 0};
  for (const item of items) counts[statusOf(item)]++;
  const usedTypes = [...new Set(items.map(item => item.type).filter((type): type is string => !!type))].sort((a, b) => a.localeCompare(b, undefined, {numeric: true}));
  const typeName = (id: string) => taskTypes[id] ?? items.find(item => item.type === id)?.typeName ?? id;
  const tree = layout === "tree";
  const rhythmCard = `<article class="card rhythm-card"><div class="section-heading compact"><div><h2>${t("Your rhythm")}</h2><p class="muted">${tree ? t("Each square is a day. Pick one to jump there in the tree.") : t("Each square is a day. Pick one to see what you finished.")}</p></div></div>${rhythm(byCategory ? items.filter(item => item.type === filter.type) : items, today, filter.day)}</article>`;
  const top = byCategory
    ? `<div id="category-panel" class="category-panel">${options.categoryPanel ?? ""}</div><section class="activity-top is-category">${rhythmCard}</section>`
    : `<section class="activity-top">${weekEntry(overview)}${rhythmCard}</section>`;
  return `${options.categoryBar ?? ""}${top}
    <div class="gallery-filters" role="toolbar" aria-label="${t("Filter task history")}">
      <div class="layout-switch" role="group" aria-label="${t("History layout")}">${([["tree", t("Tree"), icons.tree], ["gallery", t("Gallery"), icons.gallery]] as const).map(([id, label, image]) => `<button type="button" data-layout="${id}" aria-pressed="${layout === id}">${image}<span>${label}</span></button>`).join("")}</div>
      <div class="chip-row" role="group" aria-label="${t("Outcome")}">${(["all", "completed", "cancelled", "dropped"] as const).map(status => `<button type="button" class="chip ${status === "all" ? "" : OUTCOME_TONE[status]}" data-filter-status="${status}" aria-pressed="${filter.status === status}">${status === "all" ? icons.gallery : STATUS_ICON[status]}${status === "all" ? t("Everything") : t(STATUS_LABEL[status])}<b>${counts[status]}</b></button>`).join("")}</div>
      ${usedTypes.length > 1 && !options.categoryBar ? `<div class="chip-row" role="group" aria-label="${t("Task type")}">${usedTypes.map(type => `<button type="button" class="chip type-chip ${sw(typeSwatch(taskTypes, type))}" data-filter-type="${e(type)}" aria-pressed="${filter.type === type}"><i></i>${e(typeName(type))}</button>`).join("")}</div>` : ""}
      <label class="search-pill">${icons.search}<span class="sr-only">${t("Search history")}</span><input name="gallerySearch" placeholder="${t("Search your wins…")}" value="${e(filter.query)}"></label>
      ${tree ? "" : `<label class="sort-pill"><span class="sr-only">${t("Sort")}</span><select name="gallerySort">${([["newest", t("Newest first")], ["oldest", t("Oldest first")], ["priority", t("Most important")], ["longest", t("Longest journey")]] as const).map(([value, label]) => `<option value="${value}" ${filter.sort === value ? "selected" : ""}>${label}</option>`).join("")}</select></label>`}
      ${filter.day ? `<button type="button" class="chip active-day" data-action="clear-day">${icons.schedule}${e(formatDate(filter.day, {weekday: "short", month: "short", day: "numeric"}))}${icons.close}</button>` : ""}
    </div>
    <section id="history" class="gallery-region ${tree ? "tree-region" : ""}" aria-live="${tree ? "off" : "polite"}">${options.region}</section>`;
}

export function galleryGroups(items: HistoryItem[], filter: GalleryFilter, limit: number, today: string, taskTypes: Record<string, string>): string {
  const visible = filterHistory(items, filter);
  if (!visible.length) return empty(items.length ? t("Nothing matches these filters. Try another outcome or day.") : t("Your first small win will appear here."));
  const shown = visible.slice(0, limit);
  return `${groupHistory(shown, today, filter.sort).map(group => `<section class="gallery-group"><header class="gallery-heading"><h3>${e(group.label)}</h3><span class="count">${group.items.length}</span><svg class="squiggle" viewBox="0 0 120 10" preserveAspectRatio="none" aria-hidden="true"><path d="M0 5 Q 7.5 0 15 5 T 30 5 T 45 5 T 60 5 T 75 5 T 90 5 T 105 5 T 120 5"/></svg></header><div class="gallery">${group.items.map(item => galleryCard(item, items.indexOf(item), taskTypes)).join("")}</div></section>`).join("")}
    ${visible.length > shown.length ? `<div class="gallery-more"><button type="button" class="button" data-gallery-more>${icons.plus}<span>${t("Show {count} more", {count: Math.min(60, visible.length - shown.length)})}</span></button></div>` : ""}`;
}

const moment = (iso: string) => `<strong>${e(formatDate(iso, {weekday: "short", month: "short", day: "numeric"}))}</strong><small>${e(formatDate(iso, {hour: "numeric", minute: "2-digit"}))}</small>`;

/** The hover card for one history item: what it was, when it was added and closed, how long that took,
 *  and how its pace compares with the other completed tasks. */
export function peekCard(item: HistoryItem, items: HistoryItem[], taskTypes: Record<string, string>): string {
  const status = statusOf(item);
  const tone = typeSwatch(taskTypes, item.type ?? null);
  const hours = leadHours(item);
  // How its time to finish compares with the other completed tasks, said in whichever direction is true.
  const others = status === "completed" ? items.filter(entry => entry !== item && statusOf(entry) === "completed").map(leadHours) : [];
  const slower = others.filter(value => value > hours).length / (others.length || 1);
  const faster = others.filter(value => value < hours).length / (others.length || 1);
  const pace = !others.length ? "" : slower >= faster
    ? (slower ? t("Quicker than {percent}% of your other completed tasks.", {percent: Math.round(slower * 100)}) : t("About as quick as your other completed tasks."))
    : t("Took longer than {percent}% of your other completed tasks.", {percent: Math.round(faster * 100)});
  const type = (item.type && taskTypes[item.type]) || item.typeName || t("Task");
  return `<div class="peek-card ${sw(tone)}">
    <div class="peek-head"><div class="peek-art" aria-hidden="true">${artwork(item.description + item.startedAt, tone, status !== "completed")}</div>
      <div class="peek-title"><span class="peek-status ${OUTCOME_TONE[status]}">${STATUS_ICON[status]}${t(STATUS_LABEL[status])}</span><strong>${e(item.description)}</strong><span class="peek-type"><i></i>${e(type)}</span></div></div>
    <ol class="peek-journey"><li><span>${t("Added")}</span>${moment(item.startedAt)}</li><li class="peek-span"><i></i><em>${icons.hourglass}${formatHours(hours)}</em></li><li><span>${t(STATUS_LABEL[status])}</span>${moment(item.endedAt)}</li></ol>
    <div class="peek-foot"><span>${t("Importance")}</span>${priorityDots(item.priority)}<b>${item.priority}/10</b></div>
    ${pace ? `<p class="peek-note">${icons.zap}${pace}</p>` : ""}
  </div>`;
}

/** Detail view for one history item, with its lead time placed among all completed tasks. */
export function lightbox(item: HistoryItem, items: HistoryItem[], position: {index: number; total: number}, taskTypes: Record<string, string>): string {
  const status = statusOf(item);
  const tone = typeSwatch(taskTypes, item.type ?? null);
  const hours = leadHours(item);
  const completed = items.filter(entry => statusOf(entry) === "completed").map(leadHours).sort((a, b) => a - b);
  const maxHours = Math.max(1, ...completed, hours);
  const scale = (value: number) => Math.log1p(value) / Math.log1p(maxHours);
  const quicker = completed.length > 1 ? Math.round(completed.filter(value => value > hours).length / completed.length * 100) : null;
  const sample = completed.filter((_, i) => completed.length <= 80 || i % Math.ceil(completed.length / 80) === 0);
  return `<div class="lb-art ${sw(tone)}">${artwork(item.description + item.startedAt, tone, status !== "completed")}</div>
    <div class="lb-body ${sw(tone)}"><div class="lb-top"><span class="gc-status lb-status ${OUTCOME_TONE[status]}">${STATUS_ICON[status]}${t(STATUS_LABEL[status])}</span><span class="muted">${t("{n} of {total}", {n: position.index + 1, total: position.total})}</span></div>
      <h2>${e(item.description)}</h2><p class="lb-type ${sw(tone)}"><i></i>${e(item.typeName ?? t("Task"))}</p>
      <ol class="lb-journey"><li><span>${t("Added")}</span><strong>${e(formatDate(item.startedAt, {weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit"}))}</strong></li><li class="lb-span" aria-hidden="true"><i></i><em>${icons.hourglass}${formatHours(hours)}</em></li><li><span>${t(STATUS_LABEL[status])}</span><strong>${e(formatDate(item.endedAt, {weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit"}))}</strong></li></ol>
      <div class="lb-metric"><span>${t("Importance")}</span>${priorityDots(item.priority)}<b>${item.priority}/10</b></div>
      ${completed.length > 1 ? `<div class="lb-metric lb-spread"><span>${t("Time to finish, among {count} completed tasks", {count: completed.length})}</span><div class="spread" role="img" aria-label="${quicker === null ? "" : t("Quicker than {percent}% of completed tasks", {percent: quicker})}">${sample.map(value => `<i style="--x:${scale(value)}"></i>`).join("")}<b class="${sw(tone)}" style="--x:${scale(hours)}"></b></div><small>${quicker !== null && status === "completed" ? t("Quicker than {percent}% of your completed tasks.", {percent: quicker}) : t("Faster finishes sit on the left.")}</small></div>` : ""}
      <div class="button-row lb-nav"><button type="button" class="button" data-lightbox="prev" ${position.index === 0 ? "disabled" : ""}>${icons.back}<span>${t("Previous")}</span></button><button type="button" class="button" data-lightbox="next" ${position.index >= position.total - 1 ? "disabled" : ""}><span>${t("Next")}</span>${icons.arrow}</button></div>
    </div>`;
}

// ----- Weekly overview -------------------------------------------------------

function trendBars(overview: WeeklyOverview): string {
  const base = overview.baseline.completed;
  const max = Math.max(1, ...overview.trend.map(week => week.completed), base ?? 0);
  const last = overview.trend.length - 1;
  return `<div class="trend" role="img" aria-label="${t("Completed tasks over the last {count} weeks", {count: overview.trend.length})}">${overview.trend.map((week, i) => `<div class="tb ${i === last ? "current" : ""} ${week.active ? "" : "inactive"}" data-tip="${e(t("Week of {date} · {count} completed", {date: short(week.weekStart), count: week.completed}))}"><span class="tb-bar" style="--h:${week.completed / max}; --i:${i}">${i === last ? `<b>${week.completed}</b>` : ""}</span><small>${e(formatDate(week.weekStart, {month: "numeric", day: "numeric"}))}</small></div>`).join("")}
    ${base !== null ? `<span class="tb-base" style="--h:${base / max}"><em>${t("usual {value}", {value: base})}</em></span>` : ""}</div>`;
}

function dailyRhythm(overview: WeeklyOverview): string {
  const all = [...overview.days, ...overview.previousDays];
  const max = Math.max(1, ...all.map(day => day.completed));
  const typeIds = [...new Set([...Object.keys(overview.taskTypes), ...all.flatMap(day => Object.keys(day.byType))])];
  const name = (id: string) => overview.taskTypes[id] ?? id;
  const usedTypes = typeIds.filter(id => overview.days.some(day => day.byType[id]));
  const column = (day: WeekDay, previous: WeekDay) => {
    const label = weekday(day.date);
    const segments = typeIds.filter(id => day.byType[id]).map(id => `<i class="seg ${sw(typeSwatch(overview.taskTypes, id))}" style="--h:${day.byType[id] / max}" data-tip="${e(`${label} · ${name(id)}: ${day.byType[id]}`)}"></i>`).join("");
    const habits = day.habitsTotal ? day.habitsDone / day.habitsTotal : null;
    return `<div class="day-col ${day.future ? "future" : ""} ${day.date === overview.today ? "is-today" : ""}"><div class="day-stack"><span class="ghost-bar" style="--h:${previous.completed / max}" data-tip="${e(t("Last {weekday}: {count} completed", {weekday: weekday(previous.date, "long"), count: previous.completed}))}"></span><div class="segs">${segments}</div></div><b class="day-val">${day.future ? "" : day.completed}</b><span class="day-name">${e(label)}</span>${habits === null || day.future ? `<span class="day-habits empty-habits"></span>` : `<span class="day-habits" style="--p:${habits}" data-tip="${e(t("Habits {done}/{total}", {done: day.habitsDone, total: day.habitsTotal}))}" aria-label="${e(t("Habits {done} of {total}", {done: day.habitsDone, total: day.habitsTotal}))}"></span>`}</div>`;
  };
  return `<div class="daily" role="group" aria-label="${t("Completed tasks per day")}">${overview.days.map((day, i) => column(day, overview.previousDays[i])).join("")}</div>
    <div class="legend">${usedTypes.map(id => `<span class="${sw(typeSwatch(overview.taskTypes, id))}"><i></i>${e(name(id))}</span>`).join("")}<span class="legend-ghost"><i></i>${t("Last week")}</span><span class="legend-habit"><i></i>${t("Habits done")}</span></div>`;
}

function typeMix(overview: WeeklyOverview): string {
  const ids = [...new Set([...Object.keys(overview.current.byType), ...Object.keys(overview.baseline.byType)])]
    .sort((a, b) => (overview.current.byType[b] ?? 0) - (overview.current.byType[a] ?? 0) || (overview.baseline.byType[b] ?? 0) - (overview.baseline.byType[a] ?? 0));
  if (!ids.length) return empty(t("Finish a task this week to see where your effort goes."));
  const max = Math.max(1, ...ids.map(id => Math.max(overview.current.byType[id] ?? 0, overview.baseline.byType[id] ?? 0)));
  return `<div class="mix">${ids.map(id => {
    const count = overview.current.byType[id] ?? 0;
    const usual = overview.baseline.byType[id];
    const label = overview.taskTypes[id] ?? id;
    return `<div class="mix-row ${sw(typeSwatch(overview.taskTypes, id))}"><span class="mix-name" title="${e(label)}"><i></i><span>${e(label)}</span></span><div class="mix-track"><b style="--w:${count / max}" data-tip="${e(t("{type}: {count} this week", {type: label, count}))}"></b>${usual !== undefined ? `<em style="--x:${usual / max}" data-tip="${e(t("Usual: {count} per week", {count: usual}))}"></em>` : ""}</div><span class="mix-val">${count}</span></div>`;
  }).join("")}</div><div class="legend"><span class="legend-bar"><i></i>${t("This week")}</span>${overview.baseline.weeks ? `<span class="legend-tick"><i></i>${t("Usual ({weeks}-week average)", {weeks: overview.baseline.weeks})}</span>` : ""}</div>`;
}

function outcomes(overview: WeeklyOverview): string {
  const {completed, cancelled, dropped, added} = overview.current;
  const closed = completed + cancelled + dropped;
  const parts = ([["completed", completed], ["cancelled", cancelled], ["dropped", dropped]] as const).filter(([, count]) => count);
  const max = Math.max(1, added, closed);
  const net = closed - added;
  return `<div class="segbar" role="img" aria-label="${t("{completed} completed, {cancelled} cancelled, {dropped} dropped", {completed, cancelled, dropped})}">${parts.length ? parts.map(([status, count]) => `<i class="${OUTCOME_TONE[status]}" style="flex:${count}" data-tip="${t(STATUS_LABEL[status])}: ${count}"></i>`).join("") : `<i class="none"></i>`}</div>
    <div class="legend">${(["completed", "cancelled", "dropped"] as const).map(status => `<span class="${OUTCOME_TONE[status]}">${STATUS_ICON[status]}${t(STATUS_LABEL[status])} <b>${overview.current[status]}</b></span>`).join("")}</div>
    <div class="flow"><div class="flow-row"><span>${t("Added")}</span><div class="flow-track"><b class="sw-blue" style="--w:${added / max}"></b></div><strong>${added}</strong></div><div class="flow-row"><span>${t("Closed")}</span><div class="flow-track"><b class="sw-aqua" style="--w:${closed / max}"></b></div><strong>${closed}</strong></div>
    <p class="flow-net">${net > 0 ? `${icons.down} ${t("Your list shrank by {count}.", {count: net})}` : net < 0 ? `${icons.up} ${t("Your list grew by {count}.", {count: -net})}` : `${icons.target} ${t("Your list held steady.")}`}</p></div>`;
}

function habitGrid(overview: WeeklyOverview): string {
  if (!overview.habits.length) return empty(t("Add habits in Settings to see your weekly rhythm."));
  const days = overview.days.map(day => weekday(day.date).slice(0, 2));
  return `<div class="habit-grid" role="table" aria-label="${t("Habits this week")}"><div class="hg-row hg-head" role="row"><span role="columnheader">${t("Habit")}</span>${days.map(day => `<span role="columnheader">${e(day)}</span>`).join("")}<span role="columnheader">${t("Rate")}</span></div>
    ${overview.habits.map(habit => `<div class="hg-row" role="row"><span class="hg-name" role="rowheader" title="${e(habit.description)}">${e(habit.description)}</span>${habit.days.map((done, i) => { const state = done === null ? t("upcoming") : done ? t("done") : t("missed"); return `<span role="cell" class="hd ${done === null ? "future" : done ? "done" : "miss"}" style="--i:${i}" data-tip="${e(`${weekday(overview.days[i].date, "long")}: ${state}`)}">${done ? icons.check : ""}<span class="sr-only">${state}</span></span>`; }).join("")}
      <span role="cell" class="hg-rate"><b>${pctText(habit.rate)}</b>${habit.rate !== null && habit.previousRate !== null && Math.round((habit.rate - habit.previousRate) * 100) !== 0 ? `<small class="${habit.rate > habit.previousRate ? "up" : "down"}">${habit.rate > habit.previousRate ? icons.up : icons.down}<span class="sr-only">${habit.rate > habit.previousRate ? t("up from {rate} last week", {rate: pctText(habit.previousRate)}) : t("down from {rate} last week", {rate: pctText(habit.previousRate)})}</span></small>` : ""}</span></div>`).join("")}</div>`;
}

function dumbbell(options: {label: string; current: number | null; usual: number | null; min: number; max: number; format: (value: number) => string; deltaFormat?: (value: number) => string; invert?: boolean; hint: string}): string {
  const {current, usual, min, max, format} = options;
  const at = (value: number) => Math.max(0, Math.min(1, (value - min) / (max - min || 1)));
  const hasBoth = current !== null && usual !== null;
  return `<div class="dumbbell"><div class="db-head"><span>${e(options.label)}</span>${delta(current, usual, {label: t("usual"), digits: 1, invert: options.invert, format: options.deltaFormat})}</div>
    <div class="db-track" role="img" aria-label="${e(t("{label}: this week {current}, usual {usual}", {label: options.label, current: current === null ? t("none") : format(current), usual: usual === null ? t("none") : format(usual)}))}">${hasBoth ? `<span class="db-link" style="--a:${Math.min(at(current!), at(usual!))}; --b:${Math.max(at(current!), at(usual!))}"></span>` : ""}${usual !== null ? `<i class="db-usual" style="--x:${at(usual)}" data-tip="${t("Usual: {value}", {value: e(format(usual))})}"><em>${t("usual {value}", {value: e(format(usual))})}</em></i>` : ""}${current !== null ? `<i class="db-now" style="--x:${at(current)}" data-tip="${t("This week: {value}", {value: e(format(current))})}"><em>${e(format(current))}</em></i>` : ""}</div><small class="muted">${e(options.hint)}</small></div>`;
}

export function weeklyView(overview: WeeklyOverview): string {
  const {current, previous, baseline} = overview;
  const usualLabel = baseline.weeks ? t("your {weeks}-week average", {weeks: baseline.weeks}) : t("your usual");
  const leadMax = Math.max(1, current.medianLeadHours ?? 0, baseline.medianLeadHours ?? 0) * 1.25;
  const range = `${formatDate(overview.weekStart, {month: "short", day: "numeric"})} – ${formatDate(overview.weekEnd, {month: "short", day: "numeric", year: "numeric"})}`;
  const atStart = !!overview.firstActivity && overview.weekStart <= overview.firstActivity;
  const flames = overview.days.map(day => `<i class="${day.completed ? "lit" : ""} ${day.future ? "future" : ""}" data-tip="${e(t("{day}: {count} completed", {day: weekday(day.date, "long"), count: day.completed}))}">${icons.flame}</i>`).join("");
  return `<div class="week-bar"><button type="button" class="button ghost" data-action="close-week">${icons.left}<span>${t("History")}</span></button><div class="week-nav" role="group" aria-label="${t("Choose week")}"><button type="button" class="icon-button" data-week="prev" aria-label="${t("Previous week")}" ${atStart ? "disabled" : ""}>${icons.back}</button><h2 class="week-range">${e(range)}</h2><button type="button" class="icon-button" data-week="next" aria-label="${t("Next week")}" ${overview.isCurrentWeek ? "disabled" : ""}>${icons.arrow}</button></div>${overview.isCurrentWeek ? `<span class="pill">${t("This week")}</span>` : `<button type="button" class="button small" data-week="today">${t("This week")}</button>`}</div>
    <section class="bento week-bento">
      <article class="card hero-card wk-hero"><p class="eyebrow">${t("TASKS COMPLETED")}</p><div class="hero-number"><strong>${current.completed}</strong><div class="hero-deltas">${delta(current.completed, previous.completed, {label: t("last week")})}${delta(current.completed, baseline.completed, {label: usualLabel, digits: 1})}</div></div>${trendBars(overview)}</article>
      <article class="card wk-habits"><p class="eyebrow">${t("HABIT CONSISTENCY")}</p>${ring({value: current.habitRate, compare: previous.habitRate, size: 132, label: t("Habits: {current} this week, {previous} last week", {current: pctText(current.habitRate), previous: pctText(previous.habitRate)})})}<div class="legend vertical"><span class="sw-green"><i></i>${t("This week")} <b>${pctText(current.habitRate)}</b></span><span class="legend-ghost"><i></i>${t("Last week")} <b>${pctText(previous.habitRate)}</b></span></div>${current.habitRate !== null && previous.habitRate !== null ? delta(Math.round(current.habitRate * 100), Math.round(previous.habitRate * 100), {label: t("last week"), format: points => t("{n} pts", {n: points})}) : ""}</article>
      <article class="card wk-streak ${overview.streak ? "lit" : ""}"><p class="eyebrow">${t("STREAK")}</p><div class="streak-flame">${icons.flame}<strong>${overview.streak}</strong></div><p class="muted">${tn("day in a row with a finished task", "days in a row with a finished task", overview.streak)}</p><div class="flames">${flames}</div></article>
      <article class="card wk-daily"><div class="section-heading compact"><div><h2>${t("Daily rhythm")}</h2><p class="muted">${t("Finished tasks by type, with last week as a ghost.")}</p></div></div>${dailyRhythm(overview)}</article>
      <article class="card wk-mix"><div class="section-heading compact"><div><h2>${t("Where your effort went")}</h2><p class="muted">${t("This week against {usual}.", {usual: e(usualLabel)})}</p></div></div>${typeMix(overview)}</article>
      <article class="card wk-outcomes"><div class="section-heading compact"><div><h2>${t("Outcomes & flow")}</h2><p class="muted">${t("How tasks left your list, and how many arrived.")}</p></div></div>${outcomes(overview)}</article>
      <article class="card wk-habit-grid"><div class="section-heading compact"><div><h2>${t("Habit check-ins")}</h2><p class="muted">${t("Arrows compare with last week.")}</p></div></div>${habitGrid(overview)}</article>
      <article class="card wk-pace"><div class="section-heading compact"><div><h2>${t("Pace & focus")}</h2><p class="muted">${t("This week (bright) against {usual} (hollow).", {usual: e(usualLabel)})}</p></div></div>
        ${dumbbell({label: t("Time to finish (median)"), current: current.medianLeadHours, usual: baseline.medianLeadHours, min: 0, max: leadMax, format: formatHours, deltaFormat: formatHours, invert: true, hint: t("From adding a task to completing it. Further left is faster.")})}
        ${dumbbell({label: t("Importance of finished tasks"), current: current.avgPriority, usual: baseline.avgPriority, min: 1, max: 10, format: value => `${Math.round(value * 10) / 10}/10`, hint: t("Average importance, from 1 to 10.")})}</article>
      <article class="card wk-highlights"><div class="section-heading compact"><div><h2>${icons.trophy} ${t("Highlights")}</h2><p class="muted">${t("The most important things you finished.")}</p></div></div>${overview.highlights.length ? `<div class="highlights">${overview.highlights.map((item, i) => `<div class="highlight ${sw(typeSwatch(overview.taskTypes, item.type))}" style="--i:${i}"><div class="hl-art">${artwork(item.description, typeSwatch(overview.taskTypes, item.type))}<span class="hl-rank">${i + 1}</span></div><div><strong>${e(item.description)}</strong><small>${e(formatDate(item.endedAt, {weekday: "short", month: "short", day: "numeric"}))} · ${t("importance {priority}", {priority: item.priority})}</small></div></div>`).join("")}</div>` : empty(t("Your highlights will appear as you finish tasks this week."))}</article>
    </section>`;
}
