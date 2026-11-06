// Shared markup helpers, icons, and color assignments for the browser workspace.
// Every page module builds HTML strings with these helpers; user text must pass through escapeHtml.
import {
  ArrowDownRight, ArrowLeft, Ban, CalendarDays, Check, ChevronDown, ChevronLeft, ChevronRight, ChevronsDownUp, ChevronsUpDown, CircleCheck, Clock3,
  Copy, CornerDownLeft, Ellipsis, Feather, Repeat, Flag, Flame, GalleryHorizontalEnd, GripVertical, Hourglass, LayoutDashboard, ListTodo, Monitor, Moon,
  Palette, Pencil, Plus, RefreshCw, Search, Settings2, Shuffle, Sparkles, Sprout, Sun, Target, Trash2, TreeDeciduous, TrendingDown, TrendingUp,
  Trophy, X, Zap
} from "lucide";

type SvgNode = readonly [tag: string, attrs: Record<string, string | number>, children?: readonly SvgNode[]];

export function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, char => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;"}[char]!));
}
export const e = escapeHtml;

function icon(nodes: SvgNode): string {
  const node = ([tag, attrs, children]: SvgNode): string => `<${tag} ${Object.entries(attrs).map(([key, value]) => `${key}="${escapeHtml(String(value))}"`).join(" ")}>${children?.map(node).join("") ?? ""}</${tag}>`;
  return `<svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${(nodes[2] ?? []).map(node).join("")}</svg>`;
}

export const icons = {
  sprout: icon(Sprout), today: icon(LayoutDashboard), schedule: icon(CalendarDays), routines: icon(Sprout), activity: icon(CircleCheck),
  settings: icon(Settings2), plus: icon(Plus), refresh: icon(RefreshCw), close: icon(X), check: icon(Check), spark: icon(Sparkles),
  tasks: icon(ListTodo), clock: icon(Clock3), flag: icon(Flag), arrow: icon(ChevronRight), back: icon(ChevronLeft), left: icon(ArrowLeft),
  light: icon(Sun), dark: icon(Moon), system: icon(Monitor), flame: icon(Flame), up: icon(TrendingUp), down: icon(TrendingDown),
  ban: icon(Ban), dropped: icon(ArrowDownRight), trophy: icon(Trophy), search: icon(Search), gallery: icon(GalleryHorizontalEnd),
  grip: icon(GripVertical), hourglass: icon(Hourglass), palette: icon(Palette), edit: icon(Pencil), trash: icon(Trash2),
  flow: icon(Shuffle), target: icon(Target), feather: icon(Feather), zap: icon(Zap), copy: icon(Copy), repeat: icon(Repeat),
  tree: icon(TreeDeciduous), chevron: icon(ChevronDown), expand: icon(ChevronsUpDown), fold: icon(ChevronsDownUp), enter: icon(CornerDownLeft),
  more: icon(Ellipsis)
};

/** Categorical hues in a fixed, colorblind-validated order (see styles.css for light and dark steps). */
export const PALETTE = ["blue", "orange", "aqua", "yellow", "magenta", "green", "violet", "red"] as const;
export type Swatch = (typeof PALETTE)[number];
export const isSwatch = (value: unknown): value is Swatch => typeof value === "string" && (PALETTE as readonly string[]).includes(value);
export const swatchLabel = (swatch: Swatch) => swatch[0].toUpperCase() + swatch.slice(1);

const typeOrder = (taskTypes: Record<string, string>) => Object.keys(taskTypes).sort((a, b) => a.localeCompare(b, undefined, {numeric: true}));

let savedColors: Record<string, Swatch> = {};
/** Use the type colors saved in the settings (the snapshot's taskTypeColors) for every later lookup. */
export function setTypeColors(colors: Record<string, string> | null | undefined): void {
  savedColors = Object.fromEntries(Object.entries(colors ?? {}).filter((entry): entry is [string, Swatch] => isSwatch(entry[1])));
}

/** A type's saved color; otherwise the color of its place among the configured types, which is how colors were
 *  given before they were saved. Unknown IDs get a stable hashed hue. */
export function typeSwatch(taskTypes: Record<string, string>, id: string | null | undefined): Swatch | null {
  if (!id) return null;
  if (savedColors[id]) return savedColors[id];
  const index = typeOrder(taskTypes).indexOf(id);
  return PALETTE[(index >= 0 ? index : [...id].reduce((sum, char) => sum + char.charCodeAt(0), 0)) % PALETTE.length];
}
export const sw = (swatch: Swatch | null) => swatch ? `sw-${swatch}` : "sw-none";

/** Deterministic 32-bit hash for per-item art, tilt, and layout variety. */
export function hash(text: string): number {
  let value = 2166136261;
  for (let i = 0; i < text.length; i++) { value ^= text.charCodeAt(i); value = Math.imul(value, 16777619); }
  return value >>> 0;
}

export const empty = (text: string) => `<div class="empty"><span class="empty-blob">${icons.sprout}</span><p>${text}</p></div>`;
export const button = (id: string, text: string, image = "", classes = "button") => `<button type="button" class="${classes}" data-action="${id}">${image}<span>${text}</span></button>`;
export const field = (name: string, label: string, type = "text", value = "", attrs = "") => `<label>${label}<input name="${name}" type="${type}" value="${e(value)}" ${attrs}></label>`;

export function swatchPicker(name: string, selected: Swatch | "" , autoLabel = "Auto"): string {
  return `<fieldset class="swatch-picker"><legend>Color</legend><label class="swatch auto" title="${e(autoLabel)}"><input type="radio" name="${name}" value="" ${selected === "" ? "checked" : ""}><span aria-hidden="true"></span><b class="sr-only">${e(autoLabel)}</b></label>${PALETTE.map(color => `<label class="swatch sw-${color}" title="${swatchLabel(color)}"><input type="radio" name="${name}" value="${color}" ${selected === color ? "checked" : ""}><span aria-hidden="true"></span><b class="sr-only">${swatchLabel(color)}</b></label>`).join("")}</fieldset>`;
}

const dateFormats = new Map<string, Intl.DateTimeFormat>();
/** Format an ISO date (YYYY-MM-DD) or datetime in local time without UTC shifts. */
export function formatDate(value: string, options: Intl.DateTimeFormatOptions): string {
  const key = JSON.stringify(options);
  if (!dateFormats.has(key)) dateFormats.set(key, new Intl.DateTimeFormat(undefined, options));
  return dateFormats.get(key)!.format(parseLocal(value));
}
export function parseLocal(value: string): Date {
  const [datePart, timePart = "12:00:00"] = value.split("T");
  const [year, month, day] = datePart.split("-").map(Number);
  const [hour = 0, minute = 0, second = 0] = timePart.replace(/[Z+].*$/, "").split(":").map(part => Number.parseFloat(part));
  return new Date(year, month - 1, day, hour, minute, second);
}
export function isoDay(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
export function addDays(iso: string, days: number): string {
  const date = parseLocal(iso);
  date.setDate(date.getDate() + days);
  return isoDay(date);
}
export const dayDiff = (from: string, to: string) => Math.round((parseLocal(to.slice(0, 10)).getTime() - parseLocal(from.slice(0, 10)).getTime()) / 86400000);

export function formatHours(hours: number): string {
  if (hours < 1) return `${Math.max(1, Math.round(hours * 60))}m`;
  if (hours < 48) return `${Math.round(hours * 10) / 10}h`;
  return `${Math.round(hours / 24 * 10) / 10}d`;
}
