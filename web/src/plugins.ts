// Plugins: task categories that need more than a list, such as fitness training, guitar practice, or learning
// Spanish. A plugin owns one task type, so its tasks are ordinary tasks of that type: they sit in the type's group
// on the board, run in the timers, and land in the activity history like any other. On top, each plugin keeps its
// own data and log, drawn by its kind (fitness.ts, tracker.ts), and the user decides in Settings where it shows
// up: as its own page, with its own metrics when Activity is viewed by its category, as a card in the timer row,
// and whether AI agents may see its description and data to suggest tasks for it.
//
// This module registers the kinds and draws what every plugin shares: the Settings card and the plugin editor,
// the page header, the sidebar entries, the Activity category bar, and the hooks into the timers and the board.
import { fitness, starterState } from "./fitness";
import { msg, t, tn } from "./i18n";
import { bars, PLUGIN_ICONS, pluginIcon, recentWeeks, tile, weekStart, type PluginHost, type PluginModule } from "./plugin-kit";
import { metricsOf, tracker, type Metric, type MetricKind, type MetricTotal } from "./tracker";
import type { HistoryItem, PluginItem, PluginKind, PluginSettings, Snapshot, TaskItem } from "./types";
import { e, formatDate, formatHours, icons, parseLocal, sw, typeSwatch } from "./ui";

export type { PluginHost } from "./plugin-kit";
export { pluginIcon, tickPluginClocks } from "./plugin-kit";

export const MODULES: Record<PluginKind, PluginModule> = {fitness, tracker};
const KIND_NAMES: Record<PluginKind, string> = {fitness: msg("Fitness"), tracker: msg("Tracker")};
export const pluginsOf = (s: Snapshot | null | undefined): PluginItem[] => (s?.plugins ?? []).filter(plugin => plugin.kind in MODULES);
export const pluginById = (s: Snapshot | null | undefined, id: string) => pluginsOf(s).find(plugin => plugin.id === id) ?? null;
export const pluginOfType = (s: Snapshot | null | undefined, type: string | null | undefined) => type ? pluginsOf(s).find(plugin => plugin.type === type) ?? null : null;
export const PLUGIN_PAGE = "plugin:";
export const pluginPageId = (page: string) => page.startsWith(PLUGIN_PAGE) ? page.slice(PLUGIN_PAGE.length) : null;

// ----- Templates -------------------------------------------------------------------------------------------

type Template = {id: string; kind: PluginKind; name: string; description: string; icon: string; settings: PluginSettings; state: () => Record<string, unknown>};
const metric = (id: string, name: string, kind: MetricKind, unit = "", goal: number | null = null, total: MetricTotal = "sum"): Metric => ({id, name, kind, unit, goal, total});
const ON_PAGE: PluginSettings = {page: true, activity: true, timer: true, agent: false};
/** Where a new plugin starts from. Names and metric names are drawn in the interface language when it is made. */
export const TEMPLATES: Template[] = [
  {id: "fitness", kind: "fitness", name: msg("Fitness"), description: msg("Strength and cardio: plan workouts, run them set by set with a rest timer, and watch muscles, volume, records, and body weight."), icon: "dumbbell", settings: ON_PAGE, state: starterState},
  {id: "guitar", kind: "tracker", name: msg("Guitar practice"), description: msg("Daily practice: scales, technique, and songs, with time and tempo."), icon: "guitar", settings: ON_PAGE, state: () => ({metrics: [metric("minutes", t("Practice"), "minutes", "", 150), metric("tempo", t("Tempo"), "number", "BPM", null, "max"), metric("piece", t("Piece"), "text")]})},
  {id: "language", kind: "tracker", name: msg("Language learning"), description: msg("Lessons, new words, and time spent with the language."), icon: "languages", settings: ON_PAGE, state: () => ({metrics: [metric("minutes", t("Study"), "minutes", "", 105), metric("words", t("New words"), "number", t("words"), 70), metric("lesson", t("Lesson"), "text")]})},
  {id: "reading", kind: "tracker", name: msg("Reading"), description: msg("Pages read, time spent, and the book you are on."), icon: "book", settings: {...ON_PAGE, timer: false}, state: () => ({metrics: [metric("pages", t("Pages"), "number", t("pages"), 150), metric("minutes", t("Reading time"), "minutes"), metric("book", t("Book"), "text")]})},
  {id: "custom", kind: "tracker", name: msg("Your own tracker"), description: msg("Define what to record: minutes, numbers, or notes, each with a weekly goal if you like."), icon: "puzzle", settings: {...ON_PAGE, timer: false}, state: () => ({metrics: [metric("minutes", t("Time"), "minutes")]})}
];

// ----- Sidebar and page header -------------------------------------------------------------------------------

/** Sidebar entries for the plugins that have their own page. */
export function pluginNav(s: Snapshot, current: string): string {
  const pages = pluginsOf(s).filter(plugin => plugin.settings.page);
  if (!pages.length) return "";
  return `<div class="nav-section">${t("PLUGINS")}</div>${pages.map(plugin => {
    const page = `${PLUGIN_PAGE}${plugin.id}`;
    const open = s.tasks.filter(task => task.type === plugin.type).length;
    const live = plugin.kind === "fitness" ? !!plugin.state.active : !!plugin.state.timer;
    return `<button type="button" data-page="${e(page)}" title="${e(plugin.name)}" aria-current="${current === page ? "page" : "false"}" class="nav-item nav-plugin ${current === page ? "active" : ""} ${sw(typeSwatch(s.taskTypes, plugin.type))}"><span class="nav-icon">${pluginIcon(plugin)}</span><span class="nav-label">${e(plugin.name)}</span>${live ? `<span class="nav-live" aria-label="${t("In progress")}"></span>` : open ? `<span class="nav-count">${open}</span>` : ""}</button>`;
  }).join("")}`;
}
const flag = (on: boolean, onText: string, offText: string, image: string) => `<span class="plugin-flag ${on ? "is-on" : ""}">${image}<span>${on ? onText : offText}</span></span>`;
export function pluginPage(plugin: PluginItem, host: PluginHost): string {
  const s = host.snapshot();
  const module = MODULES[plugin.kind];
  const flags = [
    flag(true, t("Tasks in “{type}” on the board", {type: e(plugin.name)}), "", icons.tasks),
    flag(plugin.settings.activity, t("Tracked in Activity"), t("Not in Activity"), icons.activity),
    flag(plugin.settings.timer, t("In the timer row"), t("Not in timers"), icons.clock),
    flag(plugin.settings.agent, t("AI agents can see it"), t("Private from AI agents"), icons.spark)
  ].join("");
  return `<div class="page-heading plugin-heading ${sw(typeSwatch(s.taskTypes, plugin.type))}"><div><p class="eyebrow"><span class="plugin-badge">${pluginIcon(plugin)}</span> ${t("PLUGIN · {kind}", {kind: t(KIND_NAMES[plugin.kind]).toUpperCase()})}</p><h1>${e(plugin.name)}<span class="accent-dot">.</span></h1>${plugin.description ? `<p class="muted plugin-description">${e(plugin.description)}</p>` : ""}<div class="plugin-flags">${flags}</div></div><div class="heading-actions"><button type="button" class="button ghost" data-plugin-edit="${e(plugin.id)}">${icons.settings}<span>${t("Plugin settings")}</span></button></div></div>
    <div id="plugin-page" class="plugin-page plugin-${plugin.kind}" data-plugin="${e(plugin.id)}">${module.page(plugin, host)}</div>`;
}
export function bindPluginPage(root: HTMLElement, plugin: PluginItem, host: PluginHost): void {
  const scope = root.querySelector<HTMLElement>("#plugin-page");
  if (scope) {
    MODULES[plugin.kind].bindPage(scope, plugin, host);
    host.bindTasks(scope);
  }
}

// ----- Timer row and board -----------------------------------------------------------------------------------

/** Cards in the timer row for the plugins that asked for one. */
export function pluginTimers(s: Snapshot, now: number): string[] {
  return pluginsOf(s).filter(plugin => plugin.settings.timer).map(plugin => MODULES[plugin.kind].timer(plugin, s, now)).filter(Boolean);
}
export function bindPluginTimers(scope: HTMLElement, host: PluginHost): void {
  for (const plugin of pluginsOf(host.snapshot()).filter(item => item.settings.timer)) MODULES[plugin.kind].bindTimer?.(scope, plugin, host);
}
/** What a plugin adds to its tasks' cards on the board. */
export function taskExtra(s: Snapshot, task: TaskItem, index: number): string {
  const plugin = pluginOfType(s, task.type);
  return plugin ? MODULES[plugin.kind].taskExtra?.(task, index, plugin) ?? "" : "";
}
export function bindTaskExtras(scope: HTMLElement, host: PluginHost): void {
  for (const plugin of pluginsOf(host.snapshot())) MODULES[plugin.kind].bindTaskExtras?.(scope, plugin, host);
  scope.querySelectorAll<HTMLElement>("[data-plugin-open]").forEach(el => el.onclick = event => { event.stopPropagation(); host.navigate(`${PLUGIN_PAGE}${el.dataset.pluginOpen}`); });
}
/** The icon of a plugin's group on the board: the plugin's own, a button that opens its page. */
export function groupIcon(s: Snapshot, type: string): string | null {
  const plugin = pluginOfType(s, type);
  if (!plugin) return null;
  return `<button type="button" class="group-icon is-plugin" data-plugin-open="${e(plugin.id)}" title="${e(t("Open {name}", {name: plugin.name}))}" aria-label="${e(t("Open {name}", {name: plugin.name}))}">${pluginIcon(plugin)}</button>`;
}
/** A line under a suggestion about the plugin data it carries, such as a suggested workout. */
export function describeSuggestion(s: Snapshot, type: string, data: Record<string, unknown> | null | undefined): string {
  const plugin = pluginOfType(s, type);
  if (!plugin || !data) return "";
  return MODULES[plugin.kind].describeData?.(data, plugin) ?? "";
}
/** Steps for a task made from a suggestion's plugin data. */
export function subtasksFor(s: Snapshot, type: string, data: Record<string, unknown> | null | undefined) {
  const plugin = pluginOfType(s, type);
  return plugin && data ? MODULES[plugin.kind].subtasksFor?.(data, plugin) ?? [] : [];
}

// ----- Activity by category ----------------------------------------------------------------------------------

/** Chips to view Activity by one category: every type in the history, plugins first with their icons. */
export function categoryBar(s: Snapshot, items: HistoryItem[], selected: string | null): string {
  const used = new Set(items.map(item => item.type).filter((type): type is string => !!type));
  const plugins = pluginsOf(s).filter(plugin => plugin.type && (plugin.settings.activity || used.has(plugin.type)));
  const pluginTypes = new Set(plugins.map(plugin => plugin.type!));
  const others = [...used].filter(type => !pluginTypes.has(type)).sort((a, b) => a.localeCompare(b, undefined, {numeric: true}));
  if (!plugins.length && others.length < 2) return "";
  const count = (type: string) => items.filter(item => item.type === type).length;
  const name = (type: string) => s.taskTypes[type] ?? items.find(item => item.type === type)?.typeName ?? type;
  const chip = (type: string, label: string, image: string) => `<button type="button" class="chip category-chip ${sw(typeSwatch(s.taskTypes, type))}" data-category="${e(type)}" aria-pressed="${selected === type}">${image}${e(label)}<b>${count(type)}</b></button>`;
  return `<nav class="category-bar" aria-label="${t("View by category")}"><span class="category-label">${icons.grid}${t("View by category")}</span><button type="button" class="chip category-chip" data-category="" aria-pressed="${!selected}">${icons.gallery}${t("Everything")}<b>${items.length}</b></button>${plugins.map(plugin => chip(plugin.type!, plugin.name, `<span class="chip-plugin">${pluginIcon(plugin)}</span>`)).join("")}${others.map(type => chip(type, name(type), "<i></i>")).join("")}</nav>`;
}
/** Activity for one category: a plugin's own metrics when it is tracked there, and for every category its
 *  finished tasks per week, how many this week and last, and how long they took. */
export function categoryPanel(s: Snapshot, items: HistoryItem[], type: string, now: number): string {
  const plugin = pluginOfType(s, type);
  const own = plugin && plugin.settings.activity ? MODULES[plugin.kind].activity(plugin, now) : "";
  const done = items.filter(item => item.type === type && item.status !== "cancelled" && item.status !== "dropped");
  const weeks = recentWeeks(s.today.date, 12);
  const perWeek = weeks.map(week => done.filter(item => weekStart(item.endedAt) === week).length);
  const labels = weeks.map(week => formatDate(week, {month: "numeric", day: "numeric"}));
  const hours = done.map(item => Math.max(0, (parseLocal(item.endedAt).getTime() - parseLocal(item.startedAt).getTime()) / 3600000)).sort((a, b) => a - b);
  const median = hours.length ? hours[Math.floor(hours.length / 2)] : null;
  const name = plugin?.name ?? s.taskTypes[type] ?? items.find(item => item.type === type)?.typeName ?? type;
  const share = items.filter(item => item.status !== "cancelled" && item.status !== "dropped").length;
  const summary = `<section class="category-summary ${sw(typeSwatch(s.taskTypes, type))}"><article class="card"><div class="section-heading compact"><div><h2>${plugin ? `<span class="plugin-badge">${pluginIcon(plugin)}</span>` : ""}${e(name)}</h2><p class="muted">${t("Finished tasks in this category.")}${plugin && !plugin.settings.activity ? ` ${t("Turn on “Tracked in Activity” for this plugin to see its own metrics here.")}` : ""}</p></div>${plugin ? `<button type="button" class="button small" data-plugin-open="${e(plugin.id)}">${t("Open {name}", {name: e(plugin.name)})} ${icons.arrow}</button>` : ""}</div>
    <div class="pk-tiles">${tile(t("This week"), String(perWeek[perWeek.length - 1]), t("completed"))}${tile(t("Last week"), String(perWeek[perWeek.length - 2]), t("completed"))}${tile(t("All time"), String(done.length), share ? t("{percent}% of everything", {percent: Math.round(done.length / share * 100)}) : "")}${tile(t("Time to finish"), median === null ? "–" : formatHours(median), t("median"))}</div>
    ${bars({values: perWeek, labels, tips: perWeek.map((count, i) => t("Week of {date} · {count} completed", {date: labels[i], count})), label: t("Finished tasks per week"), tone: sw(typeSwatch(s.taskTypes, type))})}</article></section>`;
  return `${own}${summary}`;
}
export function bindCategoryPanel(root: HTMLElement, host: PluginHost, type: string | null): void {
  const plugin = pluginOfType(host.snapshot(), type);
  const scope = root.querySelector<HTMLElement>("#category-panel");
  if (!scope) return;
  if (plugin && plugin.settings.activity) MODULES[plugin.kind].bindActivity?.(scope, plugin, host);
  scope.querySelectorAll<HTMLElement>("[data-plugin-open]").forEach(el => el.onclick = () => host.navigate(`${PLUGIN_PAGE}${el.dataset.pluginOpen}`));
}

// ----- Settings ----------------------------------------------------------------------------------------------

const SETTING_LABELS: [keyof PluginSettings, string, string][] = [
  ["page", msg("Own page"), msg("A page in the sidebar.")],
  ["activity", msg("Tracked in Activity"), msg("Its own metrics when Activity is viewed by its category.")],
  ["timer", msg("In the timer row"), msg("A card above the board, such as a workout in progress or a practice timer.")],
  ["agent", msg("Visible to AI agents"), msg("Your agent reads its description and data (rmd suggest ls --json) to suggest tasks for it.")]
];
const toggle = (id: string, key: keyof PluginSettings, on: boolean, title: string, detail: string, name = "") => `<label class="setting-toggle plugin-toggle" title="${e(t(detail))}"><input type="checkbox" ${name ? `name="${name}"` : `data-plugin-setting="${e(id)}" data-key="${key}"`} ${on ? "checked" : ""}><span class="timed-switch" aria-hidden="true"></span><span class="setting-copy"><strong>${t(title)}</strong><small>${t(detail)}</small></span></label>`;

export function pluginsCard(s: Snapshot): string {
  const plugins = pluginsOf(s);
  const rows = plugins.map((plugin, i) => {
    const sessions = plugin.log.length;
    return `<li class="plugin-row ${sw(typeSwatch(s.taskTypes, plugin.type))}" style="--i:${i}"><div class="plugin-row-head"><span class="plugin-badge big">${pluginIcon(plugin)}</span><div class="plugin-copy"><strong>${e(plugin.name)}</strong><small>${t(KIND_NAMES[plugin.kind])} · ${tn("{count} entry logged", "{count} entries logged", sessions)}</small></div><div class="plugin-row-actions"><button type="button" class="button small" data-plugin-open="${e(plugin.id)}">${t("Open")}</button><button type="button" class="icon-button" data-plugin-edit="${e(plugin.id)}" aria-label="${e(t("Edit {name}", {name: plugin.name}))}">${icons.edit}</button><button type="button" class="icon-button danger" data-plugin-remove="${e(plugin.id)}" aria-label="${e(t("Remove {name}", {name: plugin.name}))}">${icons.trash}</button></div></div>${plugin.description ? `<p class="plugin-row-description">${e(plugin.description)}</p>` : ""}<div class="plugin-switches">${SETTING_LABELS.map(([key, title, detail]) => toggle(plugin.id, key, plugin.settings[key], title, detail)).join("")}</div></li>`;
  }).join("");
  const starters = TEMPLATES.map(template => `<button type="button" class="plugin-starter" data-plugin-new="${template.id}"><span class="plugin-badge">${PLUGIN_ICONS[template.icon]}</span><span>${t(template.name)}</span></button>`).join("");
  return `<article class="card plugins-card"><div class="section-heading compact"><div><h2>${PLUGIN_ICONS.puzzle} ${t("Plugins")}</h2><p class="muted">${t("A plugin is a task category with its own page and tracking, such as fitness or guitar practice. Its tasks are ordinary tasks of its type, on the board and in Activity.")}</p></div><button type="button" class="button small primary" data-plugin-new="">${icons.plus}<span>${t("New plugin")}</span></button></div>
    ${plugins.length ? `<ul class="plugin-list">${rows}</ul>` : `<p class="muted">${t("Start from one of these:")}</p>`}<div class="plugin-starters">${starters}</div></article>`;
}

/** The plugin editor: a new plugin from a template, or an existing one's name, description, icon, settings,
 *  and (for a tracker) the metrics it records. */
function editor(root: HTMLElement, host: PluginHost, plugin: PluginItem | null, templateId: string) {
  const s = host.snapshot();
  root.querySelector("dialog.plugin-dialog")?.remove();
  const el = document.createElement("dialog");
  el.className = "editor-dialog plugin-dialog";
  let template = TEMPLATES.find(item => item.id === templateId) ?? TEMPLATES[0];
  let kind: PluginKind = plugin?.kind ?? template.kind;
  let metrics: Metric[] = plugin ? metricsOf(plugin) : kind === "tracker" ? template.state().metrics as Metric[] : [];
  const owned = new Set(pluginsOf(s).map(item => item.type));
  const freeTypes = Object.entries(s.taskTypes).filter(([id]) => !owned.has(id));
  const metricRows = () => metrics.map((item, i) => `<li class="metric-row" data-metric="${i}"><input name="metricName" value="${e(item.name)}" maxlength="40" aria-label="${t("Metric name")}" placeholder="${t("Name")}" required><select name="metricKind" aria-label="${t("Measured as")}">${(["minutes", "number", "text"] as MetricKind[]).map(value => `<option value="${value}" ${item.kind === value ? "selected" : ""}>${value === "minutes" ? t("Minutes") : value === "number" ? t("Number") : t("Note")}</option>`).join("")}</select><input name="metricUnit" value="${e(item.kind === "minutes" ? "" : item.unit)}" maxlength="16" aria-label="${t("Unit")}" placeholder="${t("Unit")}" ${item.kind === "number" ? "" : "disabled"}><select name="metricTotal" aria-label="${t("A week counts")}" ${item.kind === "number" ? "" : "disabled"}>${([["sum", t("Total")], ["max", t("Best")], ["avg", t("Average")]] as const).map(([value, label]) => `<option value="${value}" ${(item.total ?? "sum") === value ? "selected" : ""}>${label}</option>`).join("")}</select><input name="metricGoal" type="number" min="0" step="any" value="${item.goal ?? ""}" aria-label="${t("Weekly goal")}" placeholder="${t("Weekly goal")}" ${item.kind === "text" ? "disabled" : ""}><button type="button" class="icon-button danger" data-metric-remove="${i}" aria-label="${t("Remove this metric")}">${icons.close}</button></li>`).join("");
  const render = () => {
    const name = plugin?.name ?? t(template.name);
    const description = plugin?.description ?? t(template.description);
    const icon = plugin?.icon ?? template.icon;
    const settings = plugin?.settings ?? template.settings;
    el.innerHTML = `<form autocomplete="off"><div class="section-heading"><h2>${plugin ? t("Edit “{name}”", {name: e(plugin.name)}) : t("A new plugin")}</h2><button class="icon-button" type="button" data-close aria-label="${t("Close")}">${icons.close}</button></div>
      ${plugin ? "" : `<fieldset class="plugin-templates"><legend>${t("Start from")}</legend>${TEMPLATES.map(item => `<label class="plugin-template ${item.id === template.id ? "is-on" : ""}"><input type="radio" name="template" value="${item.id}" ${item.id === template.id ? "checked" : ""}><span class="plugin-badge">${PLUGIN_ICONS[item.icon]}</span><strong>${t(item.name)}</strong><small>${t(item.description)}</small></label>`).join("")}</fieldset>`}
      <label>${t("Name")}<input name="name" required maxlength="60" value="${e(name)}"></label>
      <label>${t("Description")}<textarea name="description" rows="3" maxlength="600" placeholder="${t("What it is for and what you want from it.")}">${e(description)}</textarea><small class="muted">${t("AI agents read this when you let them see the plugin.")}</small></label>
      <fieldset class="plugin-icons"><legend>${t("Icon")}</legend>${Object.entries(PLUGIN_ICONS).map(([id, image]) => `<label class="plugin-icon-pick" title="${e(id)}"><input type="radio" name="icon" value="${id}" ${id === icon ? "checked" : ""}><span>${image}</span></label>`).join("")}</fieldset>
      <fieldset class="plugin-where"><legend>${t("Where it shows up")}</legend>${SETTING_LABELS.map(([key, title, detail]) => toggle("", key, settings[key], title, detail, `setting-${key}`)).join("")}</fieldset>
      ${kind === "tracker" ? `<fieldset class="plugin-metrics"><legend>${t("What it records")}</legend><ul class="metric-list">${metricRows()}</ul><button type="button" class="text-button" data-metric-add>${icons.plus}<span>${t("Add a metric")}</span></button></fieldset>` : ""}
      ${plugin ? "" : `<label>${t("Task type")}<select name="type"><option value="">${t("A new type named after it")}</option>${freeTypes.map(([id, label]) => `<option value="${e(id)}">${t("Use “{type}”", {type: e(label)})}</option>`).join("")}</select><small class="muted">${t("The plugin’s tasks are the tasks of this type.")}</small></label>`}
      <div class="button-row"><button class="button primary" type="submit">${plugin ? t("Save") : t("Add plugin")}</button><button class="button ghost" type="button" data-close>${t("Cancel")}</button></div><p class="dialog-error" role="alert"></p></form>`;
    bind();
  };
  const readMetrics = () => {
    el.querySelectorAll<HTMLElement>(".metric-row").forEach(row => {
      const item = metrics[Number(row.dataset.metric)];
      if (!item) return;
      item.name = row.querySelector<HTMLInputElement>('[name="metricName"]')!.value;
      item.kind = row.querySelector<HTMLSelectElement>('[name="metricKind"]')!.value as MetricKind;
      item.unit = row.querySelector<HTMLInputElement>('[name="metricUnit"]')!.value;
      item.total = item.kind === "number" ? row.querySelector<HTMLSelectElement>('[name="metricTotal"]')!.value as MetricTotal : "sum";
      const goal = Number(row.querySelector<HTMLInputElement>('[name="metricGoal"]')!.value);
      item.goal = goal > 0 && item.kind !== "text" ? goal : null;
    });
  };
  /** Keep what was typed when the form is drawn again (a template, a metric added or removed). */
  const keepTyped = () => {
    const form = el.querySelector("form");
    if (!form) return null;
    const data = new FormData(form);
    return {name: String(data.get("name") ?? ""), description: String(data.get("description") ?? ""), icon: String(data.get("icon") ?? "")};
  };
  const restore = (typed: ReturnType<typeof keepTyped>) => {
    if (!typed) return;
    el.querySelector<HTMLInputElement>('[name="name"]')!.value = typed.name;
    el.querySelector<HTMLTextAreaElement>('[name="description"]')!.value = typed.description;
    const icon = el.querySelector<HTMLInputElement>(`[name="icon"][value="${typed.icon}"]`);
    if (icon) icon.checked = true;
  };
  const bind = () => {
    el.querySelectorAll<HTMLButtonElement>("[data-close]").forEach(btn => btn.onclick = () => { el.close(); el.remove(); });
    el.querySelectorAll<HTMLInputElement>('[name="template"]').forEach(radio => radio.onchange = () => {
      template = TEMPLATES.find(item => item.id === radio.value) ?? template;
      kind = template.kind;
      metrics = kind === "tracker" ? template.state().metrics as Metric[] : [];
      render();
      el.querySelector<HTMLInputElement>(`[name="template"][value="${template.id}"]`)?.focus();
    });
    const typeSelect = el.querySelector<HTMLSelectElement>('[name="type"]');
    if (typeSelect) typeSelect.onchange = () => {
      const nameField = el.querySelector<HTMLInputElement>('[name="name"]')!;
      const picked = s.taskTypes[typeSelect.value];
      // Taking over a type offers its name, unless a name of one's own was typed already.
      if (picked && (nameField.value === t(template.name) || Object.values(s.taskTypes).includes(nameField.value))) nameField.value = picked;
    };
    el.querySelector<HTMLButtonElement>("[data-metric-add]")?.addEventListener("click", () => {
      readMetrics();
      const typed = keepTyped();
      metrics.push(metric(`m${Date.now().toString(36)}`, "", "number"));
      render();
      restore(typed);
      el.querySelector<HTMLInputElement>(`.metric-row[data-metric="${metrics.length - 1}"] [name="metricName"]`)?.focus();
    });
    el.querySelectorAll<HTMLButtonElement>("[data-metric-remove]").forEach(btn => btn.onclick = () => {
      readMetrics();
      const typed = keepTyped();
      metrics.splice(Number(btn.dataset.metricRemove), 1);
      render();
      restore(typed);
    });
    el.querySelectorAll<HTMLSelectElement>('[name="metricKind"]').forEach(select => select.onchange = () => {
      const row = select.closest(".metric-row")!;
      row.querySelector<HTMLInputElement>('[name="metricUnit"]')!.disabled = select.value !== "number";
      row.querySelector<HTMLSelectElement>('[name="metricTotal"]')!.disabled = select.value !== "number";
      row.querySelector<HTMLInputElement>('[name="metricGoal"]')!.disabled = select.value === "text";
    });
    el.querySelector("form")!.onsubmit = event => {
      event.preventDefault();
      readMetrics();
      const form = event.target as HTMLFormElement;
      const data = new FormData(form);
      const submit = form.querySelector<HTMLButtonElement>('[type="submit"]')!;
      const settings = Object.fromEntries(SETTING_LABELS.map(([key]) => [key, data.get(`setting-${key}`) === "on"])) as PluginSettings;
      const cleaned = metrics.filter(item => item.name.trim()).map((item, i) => ({...item, name: item.name.trim(), id: item.id || `m${i}`, unit: item.kind === "number" ? item.unit.trim() : ""}));
      submit.disabled = true;
      const payload: Record<string, unknown> = {name: String(data.get("name") ?? "").trim(), description: String(data.get("description") ?? "").trim(), icon: String(data.get("icon") ?? ""), settings};
      const run = async () => {
        if (plugin) {
          await host.request("plugin_save", {...payload, id: plugin.id});
          if (plugin.kind === "tracker") await host.request("plugin_state", {id: plugin.id, set: {metrics: cleaned}});
        } else {
          const state = kind === "tracker" ? {metrics: cleaned} : template.state();
          const result = await host.request<{plugin: PluginItem}>("plugin_save", {...payload, kind, state, type: String(data.get("type") ?? "") || null});
          await host.reload();
          if (settings.page) host.navigate(`${PLUGIN_PAGE}${result.plugin.id}`);
          host.notify(t("Added the plugin “{name}”. Its tasks live in the “{type}” group on the board.", {name: result.plugin.name, type: result.plugin.name}));
          return;
        }
        await host.reload();
        host.notify(t("Saved “{name}”.", {name: String(payload.name)}));
      };
      void run().then(() => { el.close(); el.remove(); }).catch(error => {
        el.querySelector(".dialog-error")!.textContent = error instanceof Error ? error.message : String(error);
        submit.disabled = false;
      });
    };
  };
  root.append(el);
  el.showModal();
  render();
  el.querySelector<HTMLInputElement>(plugin ? '[name="name"]' : '[name="template"]:checked')?.focus();
}

/** Plugin controls wherever they are drawn: the Settings card's switches, and Open, Edit, Remove, New. */
export function bindPluginControls(root: HTMLElement, host: PluginHost): void {
  root.querySelectorAll<HTMLInputElement>("[data-plugin-setting]").forEach(box => box.onchange = () => {
    const plugin = pluginById(host.snapshot(), box.dataset.pluginSetting!);
    const key = box.dataset.key as keyof PluginSettings;
    if (!plugin) return;
    void host.action(async () => {
      try { await host.request("plugin_save", {id: plugin.id, name: plugin.name, settings: {[key]: box.checked}}); }
      catch (error) { box.checked = !box.checked; throw error; }
      await host.reload();
      const what = t(SETTING_LABELS.find(([id]) => id === key)![1]);
      host.notify(box.checked ? t("“{name}”: {setting} is on.", {name: plugin.name, setting: what}) : t("“{name}”: {setting} is off.", {name: plugin.name, setting: what}));
    });
  });
  root.querySelectorAll<HTMLButtonElement>("[data-plugin-new]").forEach(btn => btn.onclick = () => editor(root, host, null, btn.dataset.pluginNew || "fitness"));
  root.querySelectorAll<HTMLButtonElement>("[data-plugin-edit]").forEach(btn => btn.onclick = () => {
    const plugin = pluginById(host.snapshot(), btn.dataset.pluginEdit!);
    if (plugin) editor(root, host, plugin, plugin.kind);
  });
  root.querySelectorAll<HTMLButtonElement>("[data-plugin-open]").forEach(btn => btn.onclick = () => host.navigate(`${PLUGIN_PAGE}${btn.dataset.pluginOpen}`));
  root.querySelectorAll<HTMLButtonElement>("[data-plugin-remove]").forEach(btn => btn.onclick = () => {
    const plugin = pluginById(host.snapshot(), btn.dataset.pluginRemove!);
    if (!plugin || !window.confirm(t("Remove the plugin “{name}” and its log? Its tasks stay on the board as a plain type.", {name: plugin.name}))) return;
    void host.action(async () => {
      const result = await host.request<{undo: Record<string, unknown>}>("plugin_delete", {id: plugin.id});
      await host.reload();
      host.offerUndo(t("Removed the plugin “{name}”. Its tasks stay as a plain type.", {name: plugin.name}), async () => {
        await host.request("plugin_restore", result.undo);
        await host.reload();
        host.notify(t("“{name}” is back.", {name: plugin.name}));
      });
    });
  });
}
