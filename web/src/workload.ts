// Workload: how much open work there is, weighted by importance. Every open task counts its importance out of
// 10, times the share of it still left: an importance-8 task counts 0.8 untouched, 0.4 half done, and nothing
// at 100%. Tasks postponed to a later day do not count yet. The total picks the workspace's theme (the
// [data-load] rules in styles.css), and each group's own workload places it in the spread board layout.
import { progressOf } from "./timers";
import type { TaskItem } from "./types";
import { icons } from "./ui";

export type LoadLevel = "clear" | "light" | "steady" | "busy" | "heavy";
type LevelInfo = {id: LoadLevel; label: string; below: number; icon: string; note: string};

/** From the lightest up; each level holds workloads below `below` (after rounding to one decimal). */
export const LOAD_LEVELS: readonly LevelInfo[] = [
  {id: "clear", label: "All clear", below: 0.05, icon: icons.light, note: "Nothing left to do. Enjoy the quiet."},
  {id: "light", label: "Light", below: 2.5, icon: icons.leaf, note: "Plenty of room in your day."},
  {id: "steady", label: "Steady", below: 5.5, icon: icons.waves, note: "A full but manageable day."},
  {id: "busy", label: "Busy", below: 9, icon: icons.zap, note: "A lot on your plate. Pick what matters most."},
  {id: "heavy", label: "Heavy", below: Infinity, icon: icons.flame, note: "More than a day’s work. Consider deferring or dropping something."}
];
/** Where gauges top out: a little past the start of Heavy. */
export const LOAD_SCALE = 12;
export const WORKLOAD_DEFINITION = "Workload counts every open task by its importance out of 10, times the share of it still left. Tasks postponed to a later day wait until then.";

export const roundLoad = (points: number) => Math.round(points * 10) / 10;
export const levelInfo = (level: LoadLevel): LevelInfo => LOAD_LEVELS.find(item => item.id === level)!;
export const levelOf = (points: number): LoadLevel => LOAD_LEVELS.find(item => roundLoad(points) < item.below)!.id;
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
