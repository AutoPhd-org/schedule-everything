// Settings → Reminders: what the reminder service rings for, and when. Each kind has a switch; the timed ones have
// their times beside it, the schedule's blocks and messages can be quieted one by one, and quiet days silence a whole
// day. The server keeps it all in settings.toml (`[reminders]`, `[tasks]`, `[settings] skip_days`) and the running
// service picks changes up on its next check of the clock, so every change here is saved at once.
import { msg, t } from "./i18n";
import { addDays, e, formatDate, icons } from "./ui";

/** Each kind of reminder, by its `[reminders]` switch. */
export type ReminderKind = "block_start" | "block_end" | "time_points" | "urgent_tasks" | "deadlines" | "daily_summary" | "habit_prompt" | "weekly_report" | "monthly_report";
type TimedKind = "urgent_tasks" | "deadlines" | "daily_summary" | "habit_prompt" | "weekly_report" | "monthly_report";
type DayTime<D> = {day: D; time: string} | null;
/** What `reminders_read` returns. A kind is `on` only when it is switched on and has a time to ring at. */
export type RemindersData = {
  on: Record<ReminderKind, boolean>;
  times: {urgent_tasks: string[]; deadlines: string[]; daily_summary: string | null; habit_prompt: string | null; weekly_report: DayTime<string>; monthly_report: DayTime<number>};
  skipDays: string[];
  urgentImportance: number;
  deadlineDays: number;
  blocks: {name: string; minutes: number | null; on: boolean}[];
  points: {name: string; message: string; on: boolean}[];
  limits: {maxDeadlineDays: number};
};
/** Whether a server answer is one the card can show. */
export const isReminders = (value: unknown): value is RemindersData => {
  const data = value as RemindersData | null;
  return !!data && typeof data.on === "object" && !!data.on && typeof data.times === "object" && !!data.times && Array.isArray(data.skipDays) && Array.isArray(data.blocks) && Array.isArray(data.points);
};
/** What one change sends to `reminders_save`. */
export type RemindersPatch = {
  on?: Partial<Record<ReminderKind, boolean>>;
  times?: Partial<Record<TimedKind, unknown>>;
  skipDays?: string[];
  urgentImportance?: number;
  deadlineDays?: number;
  blocks?: Record<string, boolean>;
  points?: Record<string, boolean>;
};

export const WEEKDAYS = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"] as const;
/** A weekday's name in the interface language (2024-01-01 was a Monday). */
const dayName = (day: string, width: "long" | "short" = "long") => formatDate(addDays("2024-01-01", Math.max(0, WEEKDAYS.indexOf(day as typeof WEEKDAYS[number]))), {weekday: width});

const KINDS: Record<ReminderKind, [title: string, detail: string]> = {
  block_start: [msg("When a block starts"), msg("An alarm as each focus block, meeting, or break in your week schedule begins.")],
  block_end: [msg("When a block ends"), msg("An alarm when it is over, so you stop and take the break.")],
  time_points: [msg("Schedule messages"), msg("The fixed reminders in your week schedule, such as lunch or bedtime.")],
  urgent_tasks: [msg("Important tasks check-in"), msg("Asks about each important or put-off task: done, or still open? The day’s last check marks open ones as put off.")],
  deadlines: [msg("Deadline alarm"), msg("Lists the deadlines that are close or overdue.")],
  daily_summary: [msg("Daily summary"), msg("A look back at what you finished today.")],
  habit_prompt: [msg("Habit check-in"), msg("Asks which of today’s habits you kept.")],
  weekly_report: [msg("Weekly report"), msg("Makes a PDF report of your week.")],
  monthly_report: [msg("Monthly report"), msg("Makes a PDF report of your month.")]
};
/** A kind's name in the interface language, for notices. */
export const reminderName = (kind: ReminderKind) => t(KINDS[kind][0]);

function toggle(kind: ReminderKind, on: boolean) {
  const [title, detail] = KINDS[kind];
  return `<label class="setting-toggle reminder-toggle"><input type="checkbox" data-reminder-on="${kind}" ${on ? "checked" : ""}><span class="timed-switch" aria-hidden="true"></span><span class="setting-copy"><strong>${t(title)}</strong><small>${t(detail)}</small></span></label>`;
}
const row = (kind: ReminderKind, on: boolean, when = "") =>
  `<div class="reminder-row ${on ? "" : "is-off"}" data-reminder-row="${kind}">${toggle(kind, on)}${when ? `<div class="reminder-when">${when}</div>` : ""}</div>`;

const timeInput = (kind: TimedKind, value: string | null, label: string) =>
  `<input type="time" class="reminder-time" data-reminder-time="${kind}" value="${e(value ?? "")}" aria-label="${e(label)}" required>`;

/** Times that repeat each day: one chip per time, and a field to add another. */
function timeList(kind: "urgent_tasks" | "deadlines", times: string[]) {
  const name = t(KINDS[kind][0]);
  const chips = times.map(time => `<span class="time-chip"><span>${e(time)}</span><button type="button" data-reminder-remove="${kind}" data-time="${e(time)}" aria-label="${e(t("Remove {time} from {name}", {time, name}))}">${icons.close}</button></span>`).join("");
  return `<div class="time-chips" role="group" aria-label="${e(t("Times for {name}", {name}))}">${chips || `<span class="muted time-none">${t("No times yet")}</span>`}<span class="time-add"><input type="time" data-reminder-new="${kind}" aria-label="${e(t("New time for {name}", {name}))}"><button type="button" class="icon-button" data-reminder-add="${kind}" aria-label="${e(t("Add the time to {name}", {name}))}">${icons.plus}</button></span></div>`;
}

const options = (values: (string | number)[], selected: string | number, label: (value: string | number) => string) =>
  values.map(value => `<option value="${e(String(value))}" ${String(value) === String(selected) ? "selected" : ""}>${e(label(value))}</option>`).join("");

function weekly(when: DayTime<string>) {
  const day = when?.day ?? "sunday";
  return `<span class="reminder-pair"><select data-reminder-day="weekly_report" aria-label="${e(t("Day of the weekly report"))}">${options([...WEEKDAYS], day, value => dayName(String(value)))}</select>${timeInput("weekly_report", when?.time ?? "20:00", t("Time of the weekly report"))}</span>`;
}
function monthly(when: DayTime<number>) {
  const day = when?.day ?? 1;
  const days = Array.from({length: 31}, (_, i) => i + 1);
  const note = day > 28 ? `<small class="muted reminder-note">${t("Months without that day are skipped.")}</small>` : "";
  return `<span class="reminder-pair"><select data-reminder-day="monthly_report" aria-label="${e(t("Day of the monthly report"))}">${options(days, day, value => t("Day {day}", {day: value}))}</select>${timeInput("monthly_report", when?.time ?? "20:00", t("Time of the monthly report"))}</span>${note}`;
}

/** A row of switch-like chips: pressed rings, unpressed is quiet. */
function picks(label: string, items: {key: string; text: string; title: string; on: boolean}[], attr: string, dimmed: boolean) {
  if (!items.length) return "";
  const chips = items.map(item => `<button type="button" class="chip reminder-pick" ${attr}="${e(item.key)}" aria-pressed="${item.on}" title="${e(item.title)}">${item.on ? icons.ringing : icons.quiet}<span>${e(item.text)}</span></button>`).join("");
  return `<div class="reminder-picks ${dimmed ? "is-off" : ""}" role="group" aria-label="${e(label)}"><span class="picks-label">${e(label)}</span><div class="picks">${chips}</div></div>`;
}

/** The Reminders card. `paused` is P mode, which pauses the schedule's alarms. */
export function remindersCard(data: RemindersData | null, view: {paused: boolean; error?: string | null}) {
  const heading = `<div class="reminders-head"><h2>${icons.ringing} ${t("Reminders")}</h2><p class="muted">${t("Choose what the reminder service rings for, and when. Saved in settings.toml; the running service follows each change within a minute.")}</p></div>`;
  if (!data) return `<article class="card reminders-card" id="reminders-card">${heading}<p class="muted">${view.error ? e(view.error) : t("Loading…")}</p></article>`;
  const {on, times} = data;
  const blocksOff = !on.block_start && !on.block_end;
  const blocks = picks(t("Blocks that ring"), data.blocks.map(block => ({
    key: block.name, on: block.on,
    text: block.minutes === null ? block.name : t("{name} · {minutes} min", {name: block.name, minutes: block.minutes}),
    title: block.on ? t("Rings. Press to keep {name} quiet.", {name: block.name}) : t("Quiet. Press to let {name} ring.", {name: block.name})
  })), "data-reminder-block", blocksOff);
  const points = picks(t("Messages that ring"), data.points.map(point => ({
    key: point.name, on: point.on, text: point.message,
    title: point.on ? t("Rings. Press to keep it quiet.") : t("Quiet. Press to let it ring.")
  })), "data-reminder-point", !on.time_points);
  const importance = `<label class="reminder-extra">${t("Ask about tasks of importance {level} or more", {level: `<select data-reminder-importance aria-label="${e(t("Least importance"))}">${options(Array.from({length: 10}, (_, i) => i + 1), data.urgentImportance, String)}</select>`})}</label>`;
  const lead = `<label class="reminder-extra">${t("Ring for deadlines due within {days} days", {days: `<input type="number" data-reminder-lead min="0" max="${data.limits.maxDeadlineDays}" step="1" inputmode="numeric" value="${data.deadlineDays}" aria-label="${e(t("Days before a deadline"))}">`})}</label>`;
  const quiet = WEEKDAYS.map(day => `<button type="button" class="chip quiet-day" data-quiet-day="${day}" aria-pressed="${data.skipDays.includes(day)}" title="${e(dayName(day))}">${e(dayName(day, "short"))}</button>`).join("");
  const group = (icon: string, title: string, note: string, body: string) =>
    `<section class="reminder-group"><header><h3>${icon}<span>${title}</span></h3>${note ? `<p class="muted">${note}</p>` : ""}</header>${body}</section>`;
  const scheduleNote = `${t("At the times in your week schedule.")} ${view.paused ? `<b>${t("P mode pauses these until you switch back to J mode.")}</b> ` : ""}<button type="button" class="text-button" data-page="schedule">${t("Open the schedule")}</button>`;
  return `<article class="card reminders-card" id="reminders-card">${heading}<div class="reminder-groups">
    ${group(icons.schedule, t("During your schedule"), scheduleNote, `${row("block_start", on.block_start)}${row("block_end", on.block_end)}${blocks}${row("time_points", on.time_points)}${points}`)}
    ${group(icons.flag, t("Tasks & deadlines"), "", `${row("urgent_tasks", on.urgent_tasks, `${timeList("urgent_tasks", times.urgent_tasks)}${importance}`)}${row("deadlines", on.deadlines, `${timeList("deadlines", times.deadlines)}${lead}`)}`)}
    ${group(icons.dark, t("End of the day"), "", `${row("daily_summary", on.daily_summary, timeInput("daily_summary", times.daily_summary, t("Time of the daily summary")))}${row("habit_prompt", on.habit_prompt, timeInput("habit_prompt", times.habit_prompt, t("Time of the habit check-in")))}`)}
    ${group(icons.week, t("Reports"), "", `${row("weekly_report", on.weekly_report, weekly(times.weekly_report))}${row("monthly_report", on.monthly_report, monthly(times.monthly_report))}`)}
    ${group(icons.quiet, t("Quiet days"), t("No reminders ring on these days, and their schedule is set aside."), `<div class="quiet-days" role="group" aria-label="${e(t("Quiet days"))}">${quiet}</div>`)}
  </div></article>`;
}

/** Wire the card's controls: each change is sent at once as a patch, with the notice to show once it is saved. */
export function bindReminders(scope: ParentNode, data: RemindersData | null, save: (patch: RemindersPatch, notice: string) => void) {
  const card = scope.querySelector<HTMLElement>("#reminders-card");
  if (!card || !data) return;
  const saved = t("Saved. The reminder service follows it within a minute.");
  card.querySelectorAll<HTMLInputElement>("[data-reminder-on]").forEach(box => box.onchange = () => {
    const kind = box.dataset.reminderOn as ReminderKind;
    save({on: {[kind]: box.checked}}, box.checked ? t("“{name}” is on.", {name: reminderName(kind)}) : t("“{name}” is off.", {name: reminderName(kind)}));
  });
  const listOf = (kind: string) => data.times[kind as "urgent_tasks" | "deadlines"] ?? [];
  card.querySelectorAll<HTMLButtonElement>("[data-reminder-remove]").forEach(btn => btn.onclick = () => {
    const kind = btn.dataset.reminderRemove!;
    save({times: {[kind]: listOf(kind).filter(time => time !== btn.dataset.time)}}, saved);
  });
  const add = (kind: string) => {
    const input = card.querySelector<HTMLInputElement>(`[data-reminder-new="${kind}"]`);
    const time = input?.value ?? "";
    if (!time) { input?.focus(); return; }
    if (listOf(kind).includes(time)) { input!.value = ""; return; }
    save({times: {[kind]: [...listOf(kind), time]}}, saved);
  };
  card.querySelectorAll<HTMLButtonElement>("[data-reminder-add]").forEach(btn => btn.onclick = () => add(btn.dataset.reminderAdd!));
  card.querySelectorAll<HTMLInputElement>("[data-reminder-new]").forEach(input => input.onkeydown = event => { if (event.key === "Enter") { event.preventDefault(); add(input.dataset.reminderNew!); } });
  // A day and time are saved together; a time field left empty keeps what was saved.
  const timeOf = (kind: string) => card.querySelector<HTMLInputElement>(`[data-reminder-time="${kind}"]`)?.value ?? "";
  const dayOf = (kind: string) => card.querySelector<HTMLSelectElement>(`[data-reminder-day="${kind}"]`)?.value ?? "";
  const saveWhen = (kind: TimedKind) => {
    const time = timeOf(kind);
    if (!time) return;
    const value = kind === "weekly_report" ? {day: dayOf(kind), time} : kind === "monthly_report" ? {day: Number(dayOf(kind)), time} : time;
    save({times: {[kind]: value}}, saved);
  };
  card.querySelectorAll<HTMLInputElement>("[data-reminder-time]").forEach(input => input.onchange = () => saveWhen(input.dataset.reminderTime as TimedKind));
  card.querySelectorAll<HTMLSelectElement>("[data-reminder-day]").forEach(select => select.onchange = () => saveWhen(select.dataset.reminderDay as TimedKind));
  const importance = card.querySelector<HTMLSelectElement>("[data-reminder-importance]");
  if (importance) importance.onchange = () => save({urgentImportance: Number(importance.value)}, saved);
  const lead = card.querySelector<HTMLInputElement>("[data-reminder-lead]");
  if (lead) lead.onchange = () => {
    const days = Number(lead.value);
    if (lead.value === "" || !Number.isInteger(days) || days < 0 || days > data.limits.maxDeadlineDays) { lead.value = String(data.deadlineDays); return; }
    save({deadlineDays: days}, saved);
  };
  // Blocks and messages: a pressed chip rings; pressing it again keeps that one quiet.
  for (const [selector, key, field] of [["[data-reminder-block]", "reminderBlock", "blocks"], ["[data-reminder-point]", "reminderPoint", "points"]] as const) {
    card.querySelectorAll<HTMLButtonElement>(selector).forEach(btn => btn.onclick = () => {
      const on = btn.getAttribute("aria-pressed") !== "true";
      const name = btn.dataset[key]!;
      // A block by its name; a message by what it says.
      const text = field === "blocks" ? name : btn.textContent?.trim() || name;
      save({[field]: {[name]: on}}, on ? t("“{name}” will ring again.", {name: text}) : t("“{name}” stays quiet now.", {name: text}));
    });
  }
  card.querySelectorAll<HTMLButtonElement>("[data-quiet-day]").forEach(btn => btn.onclick = () => {
    const day = btn.dataset.quietDay!;
    const quiet = btn.getAttribute("aria-pressed") !== "true";
    const days = quiet ? [...data.skipDays, day] : data.skipDays.filter(item => item !== day);
    save({skipDays: WEEKDAYS.filter(item => days.includes(item))}, quiet ? t("{day} is a quiet day now.", {day: dayName(day)}) : t("Reminders ring on {day} again.", {day: dayName(day)}));
  });
}
