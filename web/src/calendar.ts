// Interactive year calendar for deadlines: colored day markers, a day popover, and drag-to-reschedule chips.
// Rendering is pure; render.ts binds clicks, drags, and saves through the deadline API.
import type { DeadlineItem } from "./types";
import { dayDiff, e, formatDate, icons, isSwatch, PALETTE, sw, swatchLabel, swatchPicker, typeSwatch, type Swatch } from "./ui";

export type CalendarState = {year: number; day: string | null; filter: Swatch | null; flash: string | null};
type Indexed = {deadline: DeadlineItem; index: number; swatch: Swatch};

/** The color a deadline shows: its own marker, otherwise its type's color, otherwise violet. */
export function deadlineSwatch(deadline: DeadlineItem, taskTypes: Record<string, string>): Swatch {
  return isSwatch(deadline.color) ? deadline.color : typeSwatch(taskTypes, deadline.type) ?? "violet";
}

export function deadlinesByDay(deadlines: DeadlineItem[], taskTypes: Record<string, string>): Map<string, Indexed[]> {
  const days = new Map<string, Indexed[]>();
  deadlines.forEach((deadline, index) => {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(deadline.deadline)) return;
    if (!days.has(deadline.deadline)) days.set(deadline.deadline, []);
    days.get(deadline.deadline)!.push({deadline, index, swatch: deadlineSwatch(deadline, taskTypes)});
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
  return diff === 0 ? "Today" : diff === 1 ? "Tomorrow" : diff === -1 ? "Yesterday" : diff > 0 ? `In ${diff} days` : `${-diff} days ago`;
};

export function yearCalendar(options: {deadlines: DeadlineItem[]; taskTypes: Record<string, string>; today: string; state: CalendarState}): string {
  const {deadlines, taskTypes, today, state} = options;
  const byDay = deadlinesByDay(deadlines, taskTypes);
  const counts = new Map<Swatch, number>();
  for (const items of byDay.values()) for (const item of items) counts.set(item.swatch, (counts.get(item.swatch) ?? 0) + 1);
  const months = Array.from({length: 12}, (_, month) => {
    const cells = monthCells(state.year, month);
    const monthDays = cells.filter((day): day is string => !!day);
    const total = monthDays.reduce((sum, day) => sum + (byDay.get(day)?.length ?? 0), 0);
    const isCurrent = today.startsWith(`${state.year}-${String(month + 1).padStart(2, "0")}`);
    return `<section class="month ${isCurrent ? "is-current" : ""}" style="--m:${month}"><header><h3>${e(formatDate(`${state.year}-${String(month + 1).padStart(2, "0")}-01`, {month: "long"}))}</h3>${total ? `<span class="month-count">${total}</span>` : ""}</header>
      <div class="dow" aria-hidden="true"><span>M</span><span>T</span><span>W</span><span>T</span><span>F</span><span>S</span><span>S</span></div>
      <div class="days">${cells.map((day, i) => day ? dayCell(day, i % 7 >= 5, byDay.get(day) ?? [], today, state) : `<span class="day pad"></span>`).join("")}</div></section>`;
  }).join("");
  const upcoming = [...byDay.entries()].filter(([day]) => day >= today).sort(([a], [b]) => a.localeCompare(b)).flatMap(([day, items]) => items.map(item => ({...item, day})));
  return `<article class="card calendar-card" id="calendar">
    <div class="cal-head"><div><p class="eyebrow">${icons.schedule} YEAR AT A GLANCE</p><h2 class="cal-year">${state.year}</h2></div>
      <div class="cal-nav" role="group" aria-label="Choose year"><button type="button" class="icon-button" data-cal="prev" aria-label="Previous year">${icons.back}</button><button type="button" class="button small" data-cal="today">Today</button><button type="button" class="icon-button" data-cal="next" aria-label="Next year">${icons.arrow}</button></div></div>
    <div class="cal-legend" role="group" aria-label="Highlight a color">${PALETTE.map(color => `<button type="button" class="legend-swatch sw-${color}" data-cal-filter="${color}" aria-pressed="${state.filter === color}" title="Highlight ${swatchLabel(color)}"><i></i><span class="sr-only">${swatchLabel(color)}</span>${counts.get(color) ? `<b>${counts.get(color)}</b>` : ""}</button>`).join("")}<span class="cal-hint">${icons.palette} Click a day to add a deadline · drag a chip onto a day to move it</span></div>
    <div class="cal-body"><div class="year-grid ${state.filter ? `filtering filter-${state.filter}` : ""}">${months}</div>
      <aside class="cal-side" aria-label="Upcoming deadlines"><h3>Upcoming <span class="count">${upcoming.length}</span></h3>${upcoming.length ? `<ul class="ddl-chips">${upcoming.map(item => `<li><button type="button" class="ddl-chip ${sw(item.swatch)}" draggable="true" data-drag-index="${item.index}" data-chip-day="${item.day}" title="Drag onto a day to move"><span class="chip-grip">${icons.grip}</span><i></i><span class="chip-copy"><strong>${e(item.deadline.event)}</strong><small>${e(formatDate(item.day, {month: "short", day: "numeric"}))} · ${relative(today, item.day)}</small></span></button></li>`).join("")}</ul>` : `<p class="muted">Nothing ahead. Click any day to plant a deadline.</p>`}</aside></div>
    ${state.day ? dayPopover(state.day, byDay.get(state.day) ?? [], taskTypes, today) : ""}
  </article>`;
}

function dayCell(day: string, weekend: boolean, items: Indexed[], today: string, state: CalendarState): string {
  const classes = ["day", weekend ? "weekend" : "", day < today ? "past" : "", day === today ? "is-today" : "", items.length ? `has-ddl ${sw(items[0].swatch)}` : "", day === state.day ? "selected" : "", day === state.flash ? "flash" : "", state.filter && items.some(item => item.swatch === state.filter) ? "match" : ""].filter(Boolean).join(" ");
  const label = `${formatDate(day, {weekday: "long", month: "long", day: "numeric"})}${day === today ? " (today)" : ""}${items.length ? `: ${items.map(item => item.deadline.event).join(", ")}` : ""}`;
  const drag = items.length === 1 && day >= today ? ` draggable="true" data-drag-index="${items[0].index}"` : "";
  return `<button type="button" class="${classes}" data-day="${day}" aria-label="${e(label)}"${items.length ? ` data-tip="${e(items.map(item => item.deadline.event).join(" · "))}"` : ""}${drag}><span>${Number(day.slice(8))}</span>${items.length ? `<i class="marks">${items.slice(0, 3).map(item => `<b class="${sw(item.swatch)}"></b>`).join("")}</i>` : ""}</button>`;
}

function dayPopover(day: string, items: Indexed[], taskTypes: Record<string, string>, today: string): string {
  const past = day < today;
  const typeOptions = Object.entries(taskTypes).map(([id, name]) => `<option value="${e(id)}">${e(name)}</option>`).join("");
  return `<div class="day-popover" role="dialog" aria-label="${e(formatDate(day, {weekday: "long", month: "long", day: "numeric", year: "numeric"}))}" data-popover-day="${day}">
    <header><div><strong>${e(formatDate(day, {weekday: "long", month: "long", day: "numeric"}))}</strong><small>${relative(today, day)}</small></div><button type="button" class="icon-button" data-pop-close aria-label="Close">${icons.close}</button></header>
    ${items.length ? `<ul class="pop-list">${items.map(item => `<li class="${sw(item.swatch)}"><i class="pop-dot"></i><div class="pop-copy"><strong>${e(item.deadline.event)}</strong><small>${e(item.deadline.typeName ?? "Uncategorized")}</small></div><div class="mini-swatches" role="group" aria-label="${e(`Color for ${item.deadline.event}`)}">${PALETTE.map(color => `<button type="button" class="mini-swatch sw-${color}" data-pop-color="${color}" data-pop-index="${item.index}" aria-pressed="${item.deadline.color === color}" aria-label="${swatchLabel(color)}"></button>`).join("")}</div><div class="pop-actions"><button type="button" class="icon-button" data-pop-edit="${item.index}" aria-label="${e(`Edit ${item.deadline.event}`)}">${icons.edit}</button><button type="button" class="icon-button danger" data-pop-remove="${item.index}" aria-label="${e(`Remove ${item.deadline.event}`)}">${icons.trash}</button></div></li>`).join("")}</ul>` : ""}
    ${past ? `<p class="muted pop-past">This day has passed. Pick today or a later day to add a deadline.</p>` : `<form class="pop-form"><label class="sr-only" for="pop-event">Deadline name</label><input id="pop-event" name="popEvent" placeholder="${items.length ? "Add another deadline…" : "Name a deadline…"}" required maxlength="200" autocomplete="off">${swatchPicker("popColor", "", "Match type color")}<div class="pop-row"><label>Type<select name="popType"><option value="">Uncategorized</option>${typeOptions}</select></label><button class="button primary" type="submit">${icons.plus}<span>Add</span></button></div></form>`}
  </div>`;
}
