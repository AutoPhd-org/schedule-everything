// Workload: how much open work there is, weighted by importance. Every open task counts its importance out of
// 10, times the share of it still left: an importance-8 task counts 0.8 untouched, 0.4 half done, and nothing
// at 100%. Tasks postponed to a later day do not count yet. The total picks the workspace's theme (the
// [data-load] rules in styles.css), and each group's own workload places it in the spread board layout.
// Where Steady, Busy, and Heavy start is a display preference set in Settings (see setLoadThresholds).
import { progressOf } from "./timers";
import type { TaskItem } from "./types";
import { icons } from "./ui";

export type LoadLevel = "clear" | "light" | "steady" | "busy" | "heavy";
type LevelInfo = {id: LoadLevel; label: string; below: number; icon: string; note: string};

/** Where Steady, Busy, and Heavy start, in that order. All clear is always a workload of 0, and Light is
 *  anything above that, below Steady. */
export type LoadThresholds = readonly [steady: number, busy: number, heavy: number];
export const DEFAULT_THRESHOLDS: LoadThresholds = [2.5, 5.5, 9];
/** The highest number a level may start at. */
export const MAX_THRESHOLD = 100;

export const roundLoad = (points: number) => Math.round(points * 10) / 10;
/** A workload number as the Settings page writes it: one decimal at most, so 9 rather than 9.0. */
export const loadNumber = (points: number) => String(roundLoad(points));

/** Why these numbers cannot start the levels, or null when they can: each must be a number above 0 (so Light
 *  keeps some room), at most MAX_THRESHOLD, and higher than the one before. */
export function thresholdProblem(values: readonly number[]): string | null {
  if (values.length !== 3 || values.some(value => !Number.isFinite(value))) return "Enter a number for each level.";
  if (values.some(value => roundLoad(value) <= 0)) return "Each level needs to start above 0.";
  if (values.some(value => roundLoad(value) > MAX_THRESHOLD)) return `Keep each number at ${MAX_THRESHOLD} or below.`;
  if (!(roundLoad(values[0]) < roundLoad(values[1]) && roundLoad(values[1]) < roundLoad(values[2]))) return "Each level needs to start higher than the one before it.";
  return null;
}
/** Saved numbers, if they can start the levels; otherwise null. */
export function asThresholds(value: unknown): LoadThresholds | null {
  if (!Array.isArray(value) || value.some(item => typeof item !== "number") || thresholdProblem(value)) return null;
  return [roundLoad(value[0]), roundLoad(value[1]), roundLoad(value[2])];
}

const LEVELS: readonly Omit<LevelInfo, "below">[] = [
  {id: "clear", label: "All clear", icon: icons.light, note: "Nothing left to do. Enjoy the quiet."},
  {id: "light", label: "Light", icon: icons.leaf, note: "Plenty of room in your day."},
  {id: "steady", label: "Steady", icon: icons.waves, note: "A full but manageable day."},
  {id: "busy", label: "Busy", icon: icons.zap, note: "A lot on your plate. Pick what matters most."},
  {id: "heavy", label: "Heavy", icon: icons.flame, note: "More than a day’s work. Consider deferring or dropping something."}
];
const levelsFor = ([steady, busy, heavy]: LoadThresholds): LevelInfo[] =>
  LEVELS.map((level, i) => ({...level, below: [0.05, steady, busy, heavy, Infinity][i]}));
let thresholds: LoadThresholds = DEFAULT_THRESHOLDS;
let levels = levelsFor(thresholds);

/** Set where the levels start, for every workload judged from now on. */
export function setLoadThresholds(values: LoadThresholds): void {
  thresholds = asThresholds(values) ?? DEFAULT_THRESHOLDS;
  levels = levelsFor(thresholds);
}
export const loadThresholds = (): LoadThresholds => thresholds;
/** From the lightest up; each level holds workloads below `below` (after rounding to one decimal). */
export const loadLevels = (): readonly LevelInfo[] => levels;
/** Each level's range in words, as Settings lists them. */
export function levelRanges(): string[] {
  const [steady, busy, heavy] = thresholds.map(loadNumber);
  return ["0", `under ${steady}`, `${steady} – ${busy}`, `${busy} – ${heavy}`, `${heavy} and up`];
}
/** Where gauges top out: a third past the start of Heavy. */
export const loadScale = () => roundLoad(thresholds[2] * 4 / 3);
export const WORKLOAD_DEFINITION = "Workload counts every open task by its importance out of 10, times the share of it still left. Tasks postponed to a later day wait until then.";

export const levelInfo = (level: LoadLevel): LevelInfo => levels.find(item => item.id === level)!;
export const levelOf = (points: number): LoadLevel => levels.find(item => roundLoad(points) < item.below)!.id;
export const isPostponed = (task: Pick<TaskItem, "alarmFrom">, today: string) => !!task.alarmFrom && task.alarmFrom > today;
/** One task's share of the workload. */
export const taskLoad = (task: TaskItem, today: string) =>
  isPostponed(task, today) ? 0 : Math.max(0, Math.min(10, task.priority)) / 10 * (1 - progressOf(task) / 100);

export type Workload = {
  /** Rounded to one decimal, as shown. */
  points: number;
  level: LoadLevel;
  /** Open tasks that count today. */
  counted: number;
  /** Their average importance, to one decimal. */
  importance: number | null;
  /** Their average progress, 0–100. */
  done: number;
};

export function workload(tasks: readonly TaskItem[], today: string): Workload {
  const counted = tasks.filter(task => !isPostponed(task, today));
  const points = roundLoad(counted.reduce((sum, task) => sum + taskLoad(task, today), 0));
  const average = (value: (task: TaskItem) => number) => counted.reduce((sum, task) => sum + value(task), 0) / counted.length;
  return {
    points,
    level: levelOf(points),
    counted: counted.length,
    importance: counted.length ? Math.round(average(task => task.priority) * 10) / 10 : null,
    done: counted.length ? Math.round(average(progressOf)) : 0
  };
}

/** The workload in a sentence, for tooltips and screen readers. */
export function describeLoad(load: Workload): string {
  const tasks = `${load.counted} open ${load.counted === 1 ? "task" : "tasks"}`;
  return `Workload ${load.points.toFixed(1)} · ${levelInfo(load.level).label}. ${tasks}${load.importance === null ? "" : `, importance ${load.importance} on average, ${load.done}% done`}.`;
}
