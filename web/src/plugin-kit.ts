// What every plugin module shares: the host the workspace shell hands it (requests, notices, redraws), the
// module contract (a page, an Activity panel, a timer card, extras on its task cards), icons, small charts,
// number steppers, live clocks, and week arithmetic. Plugin modules (fitness.ts, tracker.ts) build on this;
// plugins.ts registers them and draws what all plugins have in common.
import {
  Activity, ArrowDown, ArrowUp, Bike, BookOpen, Brain, Camera, ChefHat, Code, Dumbbell, Footprints, Guitar, HeartPulse,
  Languages, Music, Palette, PenTool, Puzzle, Sprout
} from "lucide";
import { t } from "./i18n";
import type { HistoryItem, PluginItem, PluginKind, Snapshot, Subtask, TaskItem } from "./types";
import { addDays, e, isoDay, parseLocal } from "./ui";

type SvgNode = readonly [tag: string, attrs: Record<string, string | number>, children?: readonly SvgNode[]];
function svg(nodes: SvgNode): string {
  const node = ([tag, attrs, children]: SvgNode): string => `<${tag} ${Object.entries(attrs).map(([key, value]) => `${key}="${e(String(value))}"`).join(" ")}>${children?.map(node).join("") ?? ""}</${tag}>`;
  return `<svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${(nodes[2] ?? []).map(node).join("")}</svg>`;
}

/** Icons a plugin can wear, by the name saved with it. */
export const PLUGIN_ICONS: Record<string, string> = {
  dumbbell: svg(Dumbbell), guitar: svg(Guitar), languages: svg(Languages), book: svg(BookOpen), music: svg(Music),
  brain: svg(Brain), heart: svg(HeartPulse), bike: svg(Bike), footprints: svg(Footprints), palette: svg(Palette),
  pen: svg(PenTool), code: svg(Code), camera: svg(Camera), chef: svg(ChefHat), sprout: svg(Sprout), activity: svg(Activity), puzzle: svg(Puzzle)
};
/** Small icons plugin pages use besides the workspace's own. */
export const kitIcons = {up: svg(ArrowUp), down: svg(ArrowDown)};
export const pluginIcon = (plugin: Pick<PluginItem, "icon" | "kind"> | null | undefined) =>
  PLUGIN_ICONS[plugin?.icon ?? ""] ?? (plugin?.kind === "fitness" ? PLUGIN_ICONS.dumbbell : PLUGIN_ICONS.puzzle);

/** What the workspace shell lends a plugin module. */
export type PluginHost = {
  root: HTMLElement;
  snapshot(): Snapshot;
  /** The finished-task history, once the Activity page has loaded it. */
  history(): HistoryItem[] | null;
  request<T = unknown>(command: string, payload?: Record<string, unknown>): Promise<T>;
  /** Run work that changes data: buttons wait, errors become the notice. */
  action(work: () => Promise<void>): Promise<void>;
  /** Fetch everything again and redraw. */
  reload(): Promise<void>;
  /** Redraw the page from what is loaded, without fetching. */
  redraw(): void;
  notify(text: string): void;
  /** A notice with Undo beside it. */
  offerUndo(text: string, run: () => Promise<void>): void;
  navigate(page: string): void;
  /** Bind task cards drawn in `scope` (complete, edit, progress, subtasks) like the board's. */
  bindTasks(scope: ParentNode): void;
  /** A task card like the board's, for the task at `index` in the snapshot. */
  taskCard(task: TaskItem, index: number, position: number): string;
};

/** What a kind of plugin draws. Rendering is pure (data in, HTML out); `bind*` wire up what was drawn. */
export type PluginModule = {
  kind: PluginKind;
  /** The plugin's page, below the header every plugin page shares. */
  page(plugin: PluginItem, host: PluginHost): string;
  bindPage(scope: HTMLElement, plugin: PluginItem, host: PluginHost): void;
  /** The plugin's own metrics, shown when Activity is viewed by its category. */
  activity(plugin: PluginItem, now: number): string;
  bindActivity?(scope: HTMLElement, plugin: PluginItem, host: PluginHost): void;
  /** A card in the timer row above the board. */
  timer(plugin: PluginItem, snapshot: Snapshot, now: number): string;
  bindTimer?(scope: HTMLElement, plugin: PluginItem, host: PluginHost): void;
  /** Extra controls on the board's card of one of the plugin's tasks. */
  taskExtra?(task: TaskItem, index: number, plugin: PluginItem): string;
  bindTaskExtras?(scope: HTMLElement, plugin: PluginItem, host: PluginHost): void;
  /** Steps for a task made from plugin data, such as a suggested workout's exercises. */
  subtasksFor?(data: Record<string, unknown>, plugin: PluginItem): Subtask[];
  /** A short line about plugin data on a suggestion, such as "5 exercises · ~40 min". */
  describeData?(data: Record<string, unknown>, plugin: PluginItem): string;
};

// ----- Memory for what a plugin page shows (open tab, drafts), kept while the workspace stays open ----------

const memory = new Map<string, Record<string, unknown>>();
/** Forget what every plugin page showed (for tests, and a fresh start). */
export function forgetViews(): void { memory.clear(); }
export function view<T extends Record<string, unknown>>(pluginId: string, init: () => T): T {
  if (!memory.has(pluginId)) memory.set(pluginId, init());
  return memory.get(pluginId) as T;
}
/** A per-browser convenience (a remembered tab or draft); without storage it is simply not remembered. */
export function remember(key: string, value: unknown): void {
  try { window.localStorage.setItem(`schedule-everything-plugin-${key}`, JSON.stringify(value)); } catch { /* Keep working without storage. */ }
}
export function recall<T>(key: string, fallback: T): T {
  try {
    const raw = window.localStorage.getItem(`schedule-everything-plugin-${key}`);
    return raw === null ? fallback : JSON.parse(raw) as T;
  } catch { return fallback; }
}

// ----- Saving plugin data ---------------------------------------------------------------------------------

/** State changes go out one after another, and wait a moment for clicks to settle. */
const waiting = new Map<string, {timer: number; patch: Record<string, unknown>; send: () => Promise<void>}>();
let saving: Promise<unknown> = Promise.resolve();
/** Change parts of a plugin's data at once on the page, and save them a moment later (or now, with `now`). */
export function saveState(host: PluginHost, plugin: PluginItem, patch: Record<string, unknown>, options: {now?: boolean} = {}): Promise<void> {
  for (const [key, value] of Object.entries(patch)) {
    if (value === null) delete plugin.state[key]; else plugin.state[key] = value;
  }
  const pending = waiting.get(plugin.id);
  if (pending) window.clearTimeout(pending.timer);
  const merged = {...pending?.patch, ...patch};
  const send = () => {
    waiting.delete(plugin.id);
    const next = saving.then(() => host.request("plugin_state", {id: plugin.id, set: merged}));
    saving = next.catch(() => {});
    return next.then(() => undefined, (error: unknown) => {
      host.notify(error instanceof Error ? error.message : String(error));
      void host.reload().catch(() => {});
    });
  };
  if (options.now) return send();
  return new Promise(resolve => {
    const go = () => send().then(resolve);
    waiting.set(plugin.id, {timer: window.setTimeout(go, 350), patch: merged, send: go});
  });
}
/** Send what is still waiting for a plugin now, and wait until everything is saved. */
export async function flushState(pluginId: string): Promise<void> {
  const pending = waiting.get(pluginId);
  if (pending) {
    window.clearTimeout(pending.timer);
    await pending.send();
  }
  await saving;
}

// ----- Time ----------------------------------------------------------------------------------------------

/** Local time as `YYYY-MM-DDTHH:MM:SS`, how plugin logs keep it. */
export function localStamp(date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${isoDay(date)}T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}
export const weekStart = (iso: string) => addDays(iso.slice(0, 10), -((parseLocal(iso.slice(0, 10)).getDay() + 6) % 7));
/** The Mondays of the last `count` weeks, oldest first, ending with this week's. */
export function recentWeeks(today: string, count: number): string[] {
  const monday = weekStart(today);
  return Array.from({length: count}, (_, i) => addDays(monday, (i - count + 1) * 7));
}
/** A running time: `4:05`, or `1:02:09` past an hour. */
export function stopwatch(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor(total % 3600 / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return hours ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}` : `${minutes}:${seconds}`;
}
/** Live clocks: `data-since` counts up from a moment, `data-until` counts down to one. When a countdown
 *  reaches zero its element gets `is-done` and fires `plugin-clock-done`, once. */
export function tickPluginClocks(root: ParentNode, now: number): void {
  root.querySelectorAll<HTMLElement>("[data-since]").forEach(el => {
    const text = stopwatch(now - Number(el.dataset.since));
    if (el.textContent !== text) el.textContent = text;
  });
  root.querySelectorAll<HTMLElement>("[data-until]").forEach(el => {
    const left = Number(el.dataset.until) - now;
    const text = stopwatch(left + 999);
    if (el.textContent !== text) el.textContent = text;
    const ring = el.closest("[data-rest]")?.querySelector<SVGCircleElement>("[data-rest-ring]");
    const span = Number(el.dataset.span) || 1;
    if (ring) ring.setAttribute("stroke-dasharray", `${(Math.max(0, Math.min(1, left / span)) * 100).toFixed(2)} 100`);
    if (left <= 0 && !el.classList.contains("is-done")) {
      el.classList.add("is-done");
      el.dispatchEvent(new CustomEvent("plugin-clock-done", {bubbles: true}));
    }
  });
}
/** A short chime and a buzz, for a rest that is over; quietly nothing where the browser allows neither. */
export function chime(): void {
  try { navigator.vibrate?.([120, 60, 120]); } catch { /* No vibration. */ }
  try {
    const Context = window.AudioContext ?? (window as unknown as {webkitAudioContext?: typeof AudioContext}).webkitAudioContext;
    if (!Context) return;
    const audio = new Context();
    [0, 0.18].forEach((delay, i) => {
      const tone = audio.createOscillator();
      const gain = audio.createGain();
      tone.frequency.value = i ? 1046 : 784;
      gain.gain.setValueAtTime(0.0001, audio.currentTime + delay);
      gain.gain.exponentialRampToValueAtTime(0.18, audio.currentTime + delay + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, audio.currentTime + delay + 0.28);
      tone.connect(gain).connect(audio.destination);
      tone.start(audio.currentTime + delay);
      tone.stop(audio.currentTime + delay + 0.3);
    });
    window.setTimeout(() => void audio.close().catch(() => {}), 900);
  } catch { /* No sound. */ }
}

// ----- Numbers -------------------------------------------------------------------------------------------

export const round = (value: number, digits = 1) => Math.round(value * 10 ** digits) / 10 ** digits;
/** A number in the interface language's style, without trailing zeros. */
export const num = (value: number, digits = 1) => round(value, digits).toLocaleString(document.documentElement.lang || undefined, {maximumFractionDigits: digits});
/** Thousands as `12.4k`. */
export const compact = (value: number) => value >= 10000 ? `${num(value / 1000, 1)}k` : num(value, 0);

/** A number field with − and + beside it. `data-field` names what it sets; bindSteppers reports changes. */
export function stepper(options: {field: string; value: number; step: number; min: number; max: number; label: string; unit?: string; digits?: number; small?: boolean; caption?: boolean}): string {
  const {field, value, step, min, max, label, unit, small} = options;
  const box = `<div class="stepper ${small ? "is-small" : ""}" role="group" aria-label="${e(label)}"><button type="button" class="stepper-button" data-step="-1" aria-label="${e(t("Less {what}", {what: label}))}" tabindex="-1">−</button><label class="stepper-field"><span class="sr-only">${e(label)}</span><input type="number" inputmode="decimal" data-field="${e(field)}" value="${round(value, options.digits ?? 2)}" min="${min}" max="${max}" step="${step}">${unit ? `<small>${e(unit)}</small>` : ""}</label><button type="button" class="stepper-button" data-step="1" aria-label="${e(t("More {what}", {what: label}))}" tabindex="-1">+</button></div>`;
  return options.caption ? `<div class="stepper-with-caption"><span class="stepper-caption" aria-hidden="true">${e(label)}</span>${box}</div>` : box;
}
/** Wire up every stepper in `scope`: the buttons move the number by its step, within its bounds, and every
 *  settled change reports the field and its new value along with the stepper's element. */
export function bindSteppers(scope: ParentNode, changed: (field: string, value: number, input: HTMLInputElement) => void): void {
  scope.querySelectorAll<HTMLElement>(".stepper").forEach(box => {
    const input = box.querySelector<HTMLInputElement>("input[data-field]");
    if (!input) return;
    const clamp = (value: number) => Math.max(Number(input.min), Math.min(Number(input.max), value));
    const report = () => {
      const value = Number(input.value);
      if (!Number.isFinite(value)) return;
      const clamped = clamp(value);
      if (clamped !== value) input.value = String(clamped);
      changed(input.dataset.field!, clamped, input);
    };
    box.querySelectorAll<HTMLButtonElement>("[data-step]").forEach(btn => btn.onclick = () => {
      const step = Number(input.step) || 1;
      const next = clamp(round((Number(input.value) || 0) + step * Number(btn.dataset.step), 3));
      input.value = String(next);
      box.classList.remove("bump");
      void box.offsetWidth;
      box.classList.add("bump");
      report();
    });
    input.onchange = report;
  });
}

// ----- Charts --------------------------------------------------------------------------------------------

/** Bars over time: one per label, the last one marked as now, an optional dashed goal line. */
export function bars(options: {values: number[]; labels: string[]; tips: string[]; goal?: number | null; label: string; tone?: string}): string {
  const {values, labels, tips, goal, label} = options;
  const max = Math.max(1, ...values, goal ?? 0);
  const last = values.length - 1;
  return `<div class="pk-bars ${options.tone ?? ""}" role="img" aria-label="${e(label)}">${values.map((value, i) => `<div class="pk-bar ${i === last ? "is-now" : ""}" data-tip="${e(tips[i])}"><span style="--h:${(value / max).toFixed(3)}; --i:${i}"></span><small>${e(labels[i])}</small></div>`).join("")}${goal ? `<i class="pk-goal" style="--h:${(goal / max).toFixed(3)}"><em>${e(t("goal {n}", {n: num(goal)}))}</em></i>` : ""}</div>`;
}
/** A line through dated points, with an optional dashed target; empty when there are fewer than two. */
export function line(options: {points: {at: string; value: number}[]; target?: number | null; label: string; format: (value: number) => string}): string {
  const points = [...options.points].sort((a, b) => a.at.localeCompare(b.at));
  if (points.length < 2) return "";
  const values = points.map(point => point.value).concat(options.target ?? []);
  const low = Math.min(...values);
  const high = Math.max(...values);
  const pad = (high - low) * 0.15 || 1;
  const [min, max] = [low - pad, high + pad];
  const first = parseLocal(points[0].at).getTime();
  const span = Math.max(1, parseLocal(points[points.length - 1].at).getTime() - first);
  const x = (at: string) => 8 + (parseLocal(at).getTime() - first) / span * 284;
  const y = (value: number) => 92 - (value - min) / (max - min) * 84;
  const path = points.map((point, i) => `${i ? "L" : "M"}${x(point.at).toFixed(1)},${y(point.value).toFixed(1)}`).join(" ");
  const area = `${path} L${x(points[points.length - 1].at).toFixed(1)},100 L${x(points[0].at).toFixed(1)},100 Z`;
  const target = options.target ? `<line class="pk-line-target" x1="0" x2="300" y1="${y(options.target).toFixed(1)}" y2="${y(options.target).toFixed(1)}"/>` : "";
  const dots = points.map(point => `<circle cx="${x(point.at).toFixed(1)}" cy="${y(point.value).toFixed(1)}" r="2.6" data-tip="${e(`${point.at.slice(0, 10)} · ${options.format(point.value)}`)}"/>`).join("");
  return `<svg class="pk-line" viewBox="0 0 300 100" preserveAspectRatio="none" role="img" aria-label="${e(options.label)}">${target}<path class="pk-line-area" d="${area}"/><path class="pk-line-path" d="${path}" pathLength="100"/>${dots}</svg>`;
}
/** A number with its label, for a row of stat tiles. */
export const tile = (label: string, value: string, detail = "", tone = "") => `<div class="pk-tile ${tone}"><span>${e(label)}</span><strong>${value}</strong>${detail ? `<small>${detail}</small>` : ""}</div>`;
