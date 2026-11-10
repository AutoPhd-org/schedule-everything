// Interactive year calendar for deadlines: colored day markers, a day popover, and drag-to-reschedule chips.
// Rendering is pure; render.ts binds clicks, drags, and saves through the deadline API.
import { locale, msg, t } from "./i18n";
import type { ConferenceEvent, ConferenceGroup, ConferencePayload, DeadlineItem, RepeatRule } from "./types";
import { addDays, dayDiff, e, formatDate, icons, isoDay, isSwatch, PALETTE, parseLocal, sw, swatchLabel, swatchPicker, typeSwatch, type Swatch } from "./ui";

const SOURCE = "https://github.com/huggingface/ai-deadlines";
/** Only plain web links from remote data become clickable. */
const webLink = (url: string | null | undefined) => url && /^https?:\/\//i.test(url) ? url : null;

export type CalendarState = {year: number; day: string | null; filter: Swatch | null; flash: string | null};
/** The AI-conference layer: off by default; `selected` null means every conference. */
export type ConferenceView = {enabled: boolean; menuOpen: boolean; selected: string[] | null; data: ConferencePayload | null; loading: boolean};

export const CONFERENCE_GROUPS: [ConferenceGroup, string, Swatch][] = [["ml", msg("Machine learning"), "violet"], ["vision", msg("Computer vision"), "blue"], ["language", msg("Language"), "aqua"], ["ai", msg("AI"), "orange"], ["robotics", msg("Robotics"), "magenta"]];
const groupSwatch = (group: ConferenceGroup): Swatch => CONFERENCE_GROUPS.find(([id]) => id === group)?.[2] ?? "violet";
export const conferenceSwatch = (event: ConferenceEvent) => groupSwatch(event.group);

/** Conference events by day for the selected conferences; conferences and rebuttal periods span every day they run. */
export function conferencesByDay(view: ConferenceView | undefined): Map<string, ConferenceEvent[]> {
  const days = new Map<string, ConferenceEvent[]>();
  if (!view?.enabled || !view.data) return days;
  const add = (day: string, event: ConferenceEvent) => { if (!days.has(day)) days.set(day, []); days.get(day)!.push(event); };
  for (const event of view.data.events) {
    if (view.selected && !view.selected.includes(event.conference)) continue;
    if (!event.endDate) { add(event.date, event); continue; }
    for (let day = event.date, guard = 0; day <= event.endDate && guard < 40; day = addDays(day, 1), guard++) add(day, event);
  }
  return days;
}
/** Abstract and paper deadlines get the flag and the "My deadlines" button; rebuttals and conference days do not. */
const isDeadline = (event: ConferenceEvent) => event.kind === "abstract" || event.kind === "submission";
const shortDate = (day: string) => formatDate(day, {month: "short", day: "numeric"});
const dateSpan = (event: ConferenceEvent) => `${shortDate(event.date)} – ${shortDate(event.endDate ?? event.date)}`;
/** Lowercase a label for a sentence but keep acronyms: "Author + Reviewer + AC Discussion Period" → "author + reviewer + AC discussion period". */
const phrase = (label: string) => label.replace(/\b[A-Z][a-z]+\b/g, word => word.toLowerCase());
const conferenceTitle = (event: ConferenceEvent) => event.kind === "conference" ? `${event.name} ${event.edition}${event.place ? ` · ${event.place}` : ""}`
  : `${event.name} ${event.edition} ${phrase(event.label)}${event.endDate ? `, ${dateSpan(event)}` : ""}${event.time ? ` ${event.endDate ? t("(ends {time})", {time: event.time}) : `(${event.time})`}` : ""}`;

function conferenceControl(view: ConferenceView | undefined): string {
  if (!view) return "";
  const conferences = view.data?.conferences ?? [];
  const count = view.selected ? view.selected.length : conferences.length || 15;
  const status = view.loading ? t("Fetching conference dates…") : view.data?.error ? view.data.error : view.data?.fetchedAt ? t("Updated {date} · refreshed at most weekly", {date: new Date(view.data.fetchedAt).toLocaleDateString(locale(), {month: "short", day: "numeric"})}) : "";
  const menu = view.enabled && view.menuOpen ? `<div class="conf-menu" role="dialog" aria-label="${t("Choose AI conferences")}">
      <div class="conf-menu-head"><strong>${t("Show conferences")}</strong><span><button type="button" class="text-button" data-conf-all>${t("All")}</button><button type="button" class="text-button" data-conf-none>${t("None")}</button></span></div>
      ${conferences.length ? CONFERENCE_GROUPS.map(([group, label, swatch]) => {
        const members = conferences.filter(conference => conference.group === group);
        return members.length ? `<fieldset class="conf-group sw-${swatch}"><legend><i></i>${t(label)}</legend>${members.map(conference => `<label class="conf-pick"><input type="checkbox" data-conf-pick="${e(conference.id)}" ${!view.selected || view.selected.includes(conference.id) ? "checked" : ""}><span>${e(conference.name)}</span></label>`).join("")}</fieldset>` : "";
      }).join("") : `<p class="muted">${view.loading ? t("Loading the conference list…") : t("No conference data yet.")}</p>`}
      <div class="conf-menu-foot"><small class="${view.data?.error ? "conf-error" : "muted"}" role="status">${e(status)}</small><button type="button" class="button small" data-conf-refresh ${view.loading ? "disabled" : ""}>${icons.refresh}<span>${t("Refresh")}</span></button></div>
      <small class="muted">${t("Dates from {source}. Deadlines and rebuttal dates are shown in your time zone.", {source: `<a href="${e(webLink(view.data?.source) ?? SOURCE)}" target="_blank" rel="noopener noreferrer">Hugging Face AI Deadlines</a>`})}</small>
    </div>` : "";
  return `<div class="conf-control"><button type="button" class="conf-toggle ${view.loading ? "is-loading" : ""}" data-conf-toggle aria-pressed="${view.enabled}" title="${t("Show AI conference submission deadlines, rebuttal periods, and conference dates")}">${icons.spark}<span>${t("AI conferences")}</span>${view.enabled ? `<b>${count}</b>` : ""}</button>${view.enabled ? `<button type="button" class="icon-button" data-conf-menu aria-expanded="${view.menuOpen}" aria-label="${t("Choose AI conferences")}">${icons.settings}</button>` : ""}${menu}</div>`;
}

function conferenceSide(byConference: Map<string, ConferenceEvent[]>, view: ConferenceView | undefined, today: string, year: number): string {
  if (!view?.enabled) return "";
  const seen = new Set<string>();
  const events = [...byConference.values()].flat().filter(event => !seen.has(event.id) && seen.add(event.id)).filter(event => (event.endDate ?? event.date) >= today || year > Number(today.slice(0, 4))).sort((a, b) => a.date.localeCompare(b.date));
  const body = view.loading && !view.data ? `<p class="muted">${t("Fetching conference dates…")}</p>` : view.data?.error && !view.data.events.length ? `<p class="conf-error">${e(view.data.error)}</p>` : events.length ? `<ul class="conf-list">${events.slice(0, 8).map(event => `<li><button type="button" class="conf-item sw-${conferenceSwatch(event)}" data-conf-day="${event.date}"><i class="conf-kind-${event.kind}"></i><span><strong>${e(event.name)} ${event.edition}</strong><small>${e(event.kind === "conference" ? [dateSpan(event), event.place].filter(Boolean).join(" · ") : event.endDate ? `${dateSpan(event)} · ${event.label}` : `${event.label} · ${shortDate(event.date)}`)}</small></span></button></li>`).join("")}</ul>${events.length > 8 ? `<p class="muted">${t("+{count} more on the calendar", {count: events.length - 8})}</p>` : ""}` : `<p class="muted">${t("No dates for the selected conferences in {year}.", {year})}</p>`;
  return `<h3 class="conf-side-title">${icons.spark} ${t("AI conferences")}</h3>${body}`;
}

/** A deadline on one day; `occurrence` marks a later repeat rather than the stored next date. */
type Indexed = {deadline: DeadlineItem; index: number; swatch: Swatch; occurrence: boolean};

/** Repeat rules with their labels; the labels are message keys, translated where drawn (see repeatLabel). */
export const REPEAT_OPTIONS: [RepeatRule | "", string][] = [["", msg("Does not repeat")], ["weekly", msg("Weekly")], ["biweekly", msg("Every 2 weeks")], ["monthly", msg("Monthly")]];
const REPEAT_LABELS: Record<RepeatRule, string> = {weekly: msg("Weekly"), biweekly: msg("Every 2 weeks"), monthly: msg("Monthly")};
/** The same rules as a sentence, for tooltips; lowercased (first letter only) inside other text. */
const REPEATS: Record<RepeatRule, string> = {weekly: msg("Repeats weekly"), biweekly: msg("Repeats every 2 weeks"), monthly: msg("Repeats monthly")};
const repeatsPhrase = (rule: RepeatRule) => { const text = t(REPEATS[rule]); return text.charAt(0).toLowerCase() + text.slice(1); };
const isRepeat = (value: unknown): value is RepeatRule => value === "weekly" || value === "biweekly" || value === "monthly";
export const repeatLabel = (rule: RepeatRule | null | undefined) => isRepeat(rule) ? t(REPEAT_LABELS[rule]) : "";
export const repeatOptions = (selected: RepeatRule | "" | null | undefined) => REPEAT_OPTIONS.map(([value, label]) => `<option value="${value}" ${(selected ?? "") === value ? "selected" : ""}>${t(label)}</option>`).join("");
export const repeatBadge = (rule: RepeatRule | null | undefined) => isRepeat(rule) ? `<span class="repeat-badge" title="${t(REPEATS[rule])}">${icons.repeat}${t(REPEAT_LABELS[rule])}</span>` : "";

/** The occurrence after `iso` (mirrors `crates/rmd/src/recurrence.rs`; monthly keeps the chosen day, clamped to short months). */
export function nextOccurrence(iso: string, rule: RepeatRule, day?: number | null): string {
  if (rule === "weekly") return addDays(iso, 7);
  if (rule === "biweekly") return addDays(iso, 14);
  const current = parseLocal(iso);
  const year = current.getFullYear() + Math.floor((current.getMonth() + 1) / 12);
  const month = (current.getMonth() + 1) % 12;
  const last = new Date(year, month + 1, 0).getDate();
  return isoDay(new Date(year, month, Math.min(day ?? current.getDate(), last), 12));
}

/** The color a deadline shows: its own marker, otherwise its type's color, otherwise violet. */
export function deadlineSwatch(deadline: DeadlineItem, taskTypes: Record<string, string>): Swatch {
  return isSwatch(deadline.color) ? deadline.color : typeSwatch(taskTypes, deadline.type) ?? "violet";
}

/** Deadlines by day; recurring ones also appear on every later occurrence through `until`. */
export function deadlinesByDay(deadlines: DeadlineItem[], taskTypes: Record<string, string>, until?: string): Map<string, Indexed[]> {
  const days = new Map<string, Indexed[]>();
  const add = (day: string, item: Indexed) => { if (!days.has(day)) days.set(day, []); days.get(day)!.push(item); };
  deadlines.forEach((deadline, index) => {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(deadline.deadline)) return;
    const swatch = deadlineSwatch(deadline, taskTypes);
    add(deadline.deadline, {deadline, index, swatch, occurrence: false});
    if (!until || !isRepeat(deadline.repeat)) return;
    let day = nextOccurrence(deadline.deadline, deadline.repeat, deadline.repeatDay);
    for (let guard = 0; day <= until && guard < 600; guard++) {
      add(day, {deadline, index, swatch, occurrence: true});
      day = nextOccurrence(day, deadline.repeat, deadline.repeatDay);
    }
  });
  return days;
}

/** Monday-first month grid; null pads the leading and trailing cells. */
export function monthCells(year: number, month: number): (string | null)[] {
  const first = new Date(year, month, 1);
  const lead = (first.getDay() + 6) % 7;
  const length = new Date(year, month + 1, 0).getDate();
  const cells: (string | null)[] = Array.from({length: lead}, () => null);
  for (let day = 1; day <= length; day++) cells.push(`${year}-${String(month + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`);
  while (cells.length % 7) cells.push(null);
  return cells;
}

const relative = (today: string, day: string) => {
  const diff = dayDiff(today, day);
  return diff === 0 ? t("Today") : diff === 1 ? t("Tomorrow") : diff === -1 ? t("Yesterday") : diff > 0 ? t("In {count} days", {count: diff}) : t("{count} days ago", {count: -diff});
};

export function yearCalendar(options: {deadlines: DeadlineItem[]; taskTypes: Record<string, string>; today: string; state: CalendarState; conferences?: ConferenceView}): string {
  const {deadlines, taskTypes, today, state, conferences} = options;
  const byDay = deadlinesByDay(deadlines, taskTypes, `${state.year}-12-31`);
  const byConference = conferencesByDay(conferences);
  const counts = new Map<Swatch, number>();
  // Weekday initials, Monday first (1 January 2024 was a Monday), in the interface language.
  const initials = Array.from({length: 7}, (_, i) => `<span>${e(formatDate(addDays("2024-01-01", i), {weekday: "narrow"}))}</span>`).join("");
  for (const items of byDay.values()) for (const item of items) if (!item.occurrence) counts.set(item.swatch, (counts.get(item.swatch) ?? 0) + 1);
  const months = Array.from({length: 12}, (_, month) => {
    const cells = monthCells(state.year, month);
    const monthDays = cells.filter((day): day is string => !!day);
    const total = monthDays.reduce((sum, day) => sum + (byDay.get(day)?.length ?? 0), 0);
    const isCurrent = today.startsWith(`${state.year}-${String(month + 1).padStart(2, "0")}`);
    return `<section class="month ${isCurrent ? "is-current" : ""}" style="--m:${month}"><header><h3>${e(formatDate(`${state.year}-${String(month + 1).padStart(2, "0")}-01`, {month: "long"}))}</h3>${total ? `<span class="month-count">${total}</span>` : ""}</header>
      <div class="dow" aria-hidden="true">${initials}</div>
      <div class="days">${cells.map((day, i) => day ? dayCell(day, i % 7 >= 5, byDay.get(day) ?? [], today, state, byConference) : `<span class="day pad"></span>`).join("")}</div></section>`;
  }).join("");
  const upcoming = [...byDay.entries()].filter(([day]) => day >= today).sort(([a], [b]) => a.localeCompare(b)).flatMap(([day, items]) => items.filter(item => !item.occurrence).map(item => ({...item, day})));
  return `<article class="card calendar-card" id="calendar">
    <div class="cal-head"><div><p class="eyebrow">${icons.schedule} ${t("YEAR AT A GLANCE")}</p><h2 class="cal-year">${state.year}</h2></div>
      ${conferenceControl(conferences)}<div class="cal-nav" role="group" aria-label="${t("Choose year")}"><button type="button" class="icon-button" data-cal="prev" aria-label="${t("Previous year")}">${icons.back}</button><button type="button" class="button small" data-cal="today">${t("Today")}</button><button type="button" class="icon-button" data-cal="next" aria-label="${t("Next year")}">${icons.arrow}</button></div></div>
    <div class="cal-legend" role="group" aria-label="${t("Highlight a color")}">${PALETTE.map(color => `<button type="button" class="legend-swatch sw-${color}" data-cal-filter="${color}" aria-pressed="${state.filter === color}" title="${t("Highlight {color}", {color: swatchLabel(color)})}"><i></i><span class="sr-only">${swatchLabel(color)}</span>${counts.get(color) ? `<b>${counts.get(color)}</b>` : ""}</button>`).join("")}<span class="cal-hint">${icons.palette} ${t("Click a day to add a deadline or a repeating event · drag a chip onto a day to move it")}</span></div>
    <div class="cal-body"><div class="year-grid ${state.filter ? `filtering filter-${state.filter}` : ""}">${months}</div>
      <aside class="cal-side" aria-label="${t("Upcoming deadlines")}"><h3>${t("Upcoming")} <span class="count">${upcoming.length}</span></h3>${upcoming.length ? `<ul class="ddl-chips">${upcoming.map(item => `<li><button type="button" class="ddl-chip ${sw(item.swatch)}" draggable="true" data-drag-index="${item.index}" data-chip-day="${item.day}" title="${t("Drag onto a day to move")}"><span class="chip-grip">${icons.grip}</span><i></i><span class="chip-copy"><strong>${e(item.deadline.event)}</strong><small>${e(formatDate(item.day, {month: "short", day: "numeric"}))} · ${relative(today, item.day)}${item.deadline.repeat ? ` · ↻ ${repeatLabel(item.deadline.repeat)}` : ""}</small></span></button></li>`).join("")}</ul>` : `<p class="muted">${t("Nothing ahead. Click any day to plant a deadline.")}</p>`}${conferenceSide(byConference, conferences, today, state.year)}</aside></div>
    ${state.day ? dayPopover(state.day, byDay.get(state.day) ?? [], taskTypes, today, byConference.get(state.day) ?? []) : ""}
  </article>`;
}

function dayCell(day: string, weekend: boolean, items: Indexed[], today: string, state: CalendarState, byConference: Map<string, ConferenceEvent[]>): string {
  const onlyRepeats = items.length > 0 && items.every(item => item.occurrence);
  const conferenceEvents = byConference.get(day) ?? [];
  // A flag marks deadlines, a bar under the day marks conference days, and a dashed bar over it marks rebuttal dates.
  const deadlinesHere = conferenceEvents.filter(isDeadline);
  const running = conferenceEvents.find(event => event.kind === "conference");
  const rebuttal = conferenceEvents.find(event => event.kind === "rebuttal");
  const confClasses = [deadlinesHere.length ? "conf-deadline" : "", running ? `conf-range${running.date === day ? " conf-start" : ""}${running.endDate === day ? " conf-end" : ""}` : "",
    rebuttal ? `conf-rebuttal${rebuttal.date === day ? " rebuttal-start" : ""}${(rebuttal.endDate ?? rebuttal.date) === day ? " rebuttal-end" : ""}` : ""].filter(Boolean).join(" ");
  const confColors = [deadlinesHere.length ? `--conf-c: var(--c-${conferenceSwatch(deadlinesHere[0])})` : "", running ? `--conf-bar: var(--c-${conferenceSwatch(running)})` : "", rebuttal ? `--rebuttal-c: var(--c-${conferenceSwatch(rebuttal)})` : ""].filter(Boolean);
  const confStyle = confColors.length ? ` style="${confColors.join("; ")}"` : "";
  const tips = [...items.map(item => item.deadline.event + (isRepeat(item.deadline.repeat) ? " ↻" : "")), ...conferenceEvents.map(conferenceTitle)];
  const classes = ["day", weekend ? "weekend" : "", day < today ? "past" : "", day === today ? "is-today" : "", items.length ? `has-ddl ${sw(items[0].swatch)}` : "", onlyRepeats ? "occurrence" : "", items.some(item => isRepeat(item.deadline.repeat)) ? "repeats" : "", day === state.day ? "selected" : "", day === state.flash ? "flash" : "", state.filter && items.some(item => item.swatch === state.filter) ? "match" : "", confClasses].filter(Boolean).join(" ");
  const date = formatDate(day, {weekday: "long", month: "long", day: "numeric"});
  const label = `${day === today ? t("{date} (today)", {date}) : date}${items.length ? `: ${items.map(item => item.deadline.event + (isRepeat(item.deadline.repeat) ? ` (${repeatsPhrase(item.deadline.repeat)})` : "")).join(", ")}` : ""}${conferenceEvents.length ? `${items.length ? "; " : ": "}${conferenceEvents.map(conferenceTitle).join(", ")}` : ""}`;
  const drag = items.length === 1 && day >= today ? ` draggable="true" data-drag-index="${items[0].index}"` : "";
  return `<button type="button" class="${classes}" data-day="${day}" aria-label="${e(label)}"${tips.length ? ` data-tip="${e(tips.join(" · "))}"` : ""}${confStyle}${drag}><span>${Number(day.slice(8))}</span>${items.length ? `<i class="marks">${items.slice(0, 3).map(item => `<b class="${sw(item.swatch)}"></b>`).join("")}</i>` : ""}${deadlinesHere.length ? `<i class="conf-flag" aria-hidden="true"></i>` : ""}</button>`;
}

function conferenceList(events: ConferenceEvent[], day: string, today: string): string {
  if (!events.length) return "";
  // Conference days show their dates and place; deadlines and rebuttals show your local time and the original one.
  const when = (event: ConferenceEvent) => event.kind === "conference" ? [dateSpan(event), event.place]
    : [event.endDate ? dateSpan(event) : "", event.time ? (event.endDate ? t("ends {time} your time", {time: event.time}) : t("{time} your time", {time: event.time})) : "", event.sourceTime];
  return `<ul class="pop-conf">${events.map(event => `<li class="sw-${conferenceSwatch(event)}"><i class="conf-kind-${event.kind}" aria-hidden="true"></i><div class="pop-copy"><strong>${e(event.kind === "conference" ? `${event.name} ${event.edition}` : `${event.name} ${event.edition} · ${event.label}`)}</strong><small>${e(when(event).filter(Boolean).join(" · "))}</small>${webLink(event.link) ? `<a class="conf-link" href="${e(webLink(event.link)!)}" target="_blank" rel="noopener noreferrer">${e(event.fullName || t("Conference website"))} ↗</a>` : ""}</div>${isDeadline(event) && day >= today ? `<button type="button" class="button small" data-conf-add="${e(event.id)}" title="${t("Copy this date into your deadlines")}">${icons.plus}<span>${t("My deadlines")}</span></button>` : ""}</li>`).join("")}</ul>`;
}

function dayPopover(day: string, items: Indexed[], taskTypes: Record<string, string>, today: string, conferenceEvents: ConferenceEvent[] = []): string {
  const past = day < today;
  const typeOptions = Object.entries(taskTypes).map(([id, name]) => `<option value="${e(id)}">${e(name)}</option>`).join("");
  return `<div class="day-popover" role="dialog" aria-label="${e(formatDate(day, {weekday: "long", month: "long", day: "numeric", year: "numeric"}))}" data-popover-day="${day}">
    <header><div><strong>${e(formatDate(day, {weekday: "long", month: "long", day: "numeric"}))}</strong><small>${relative(today, day)}</small></div><button type="button" class="icon-button" data-pop-close aria-label="${t("Close")}">${icons.close}</button></header>
    ${conferenceList(conferenceEvents, day, today)}
    ${items.length ? `<ul class="pop-list">${items.map(item => `<li class="${sw(item.swatch)}"><i class="pop-dot"></i><div class="pop-copy"><strong>${e(item.deadline.event)}</strong><small>${e(item.deadline.typeName ?? t("Uncategorized"))}${item.deadline.repeat ? ` · ↻ ${item.occurrence ? t("{repeat}, next on {date}", {repeat: repeatLabel(item.deadline.repeat), date: e(formatDate(item.deadline.deadline, {month: "short", day: "numeric"}))}) : repeatLabel(item.deadline.repeat)}` : ""}</small></div><div class="mini-swatches" role="group" aria-label="${e(t("Color for {event}", {event: item.deadline.event}))}">${PALETTE.map(color => `<button type="button" class="mini-swatch sw-${color}" data-pop-color="${color}" data-pop-index="${item.index}" aria-pressed="${item.deadline.color === color}" aria-label="${swatchLabel(color)}"></button>`).join("")}</div><div class="pop-actions"><button type="button" class="icon-button" data-pop-edit="${item.index}" aria-label="${e(t("Edit {event}", {event: item.deadline.event}))}">${icons.edit}</button><button type="button" class="icon-button danger" data-pop-remove="${item.index}" aria-label="${e(t("Remove {event}", {event: item.deadline.event}))}">${icons.trash}</button></div></li>`).join("")}</ul>` : ""}
    ${past ? `<p class="muted pop-past">${t("This day has passed. Pick today or a later day to add a deadline.")}</p>` : `<form class="pop-form"><label class="sr-only" for="pop-event">${t("Deadline name")}</label><input id="pop-event" name="popEvent" placeholder="${items.length ? t("Add another deadline…") : t("Name a deadline…")}" required maxlength="200" autocomplete="off">${swatchPicker("popColor", "", t("Match type color"))}<div class="pop-row"><label>${t("Repeat")}<select name="popRepeat">${repeatOptions("")}</select></label><label>${t("Type")}<select name="popType"><option value="">${t("Uncategorized")}</option>${typeOptions}</select></label></div><div class="pop-submit"><button class="button primary" type="submit">${icons.plus}<span>${t("Add")}</span></button></div></form>`}
  </div>`;
}
