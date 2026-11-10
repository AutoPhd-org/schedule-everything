// The fitness plugin: an exercise library, a body figure whose muscles light up with the week's training (click
// one to find exercises for it), a workout builder whose units (an exercise with its sets, reps, weight, time,
// distance, and rest) are adjusted with steppers, saved routines, workouts planned as tasks, a live session with
// set check-offs and a rest timer, and progress: weekly sessions and volume, personal records, body weight.
//
// Its data (`plugin.state`): `units` ("kg" or "lb"; weights are kept in kg), `goal` (workouts a week),
// `bodyGoal` (kg), `exercises` (the user's own), `routines` (saved workouts), and `active` (the session in
// progress). Its log: `session` entries (a workout done, with every set), `task` entries (a planned workout
// completed from the board), and `body` entries (weight in kg, body fat in %).
import { ring } from "./charts";
import { msg, t, tn } from "./i18n";
import {
  bars, bindSteppers, chime, compact, flushState, kitIcons, line, localStamp, num, pluginIcon, recall, recentWeeks, remember,
  round, saveState, stepper, tile, view, weekStart, type PluginHost, type PluginModule
} from "./plugin-kit";
import type { PluginItem, PluginLogEntry, Snapshot, Subtask, TaskItem } from "./types";
import { addDays, e, empty, formatDate, icons, isoDay } from "./ui";

// ----- Muscles and exercises ------------------------------------------------------------------------------

export type Muscle = "chest" | "shoulders" | "biceps" | "triceps" | "forearms" | "abs" | "obliques" | "traps" | "lats" | "lowerBack" | "glutes" | "quads" | "hamstrings" | "calves";
export const MUSCLES: Record<Muscle, string> = {
  chest: msg("Chest"), shoulders: msg("Shoulders"), biceps: msg("Biceps"), triceps: msg("Triceps"), forearms: msg("Forearms"),
  abs: msg("Abs"), obliques: msg("Obliques"), traps: msg("Traps"), lats: msg("Lats"), lowerBack: msg("Lower back"),
  glutes: msg("Glutes"), quads: msg("Quads"), hamstrings: msg("Hamstrings"), calves: msg("Calves")
};
const MUSCLE_IDS = Object.keys(MUSCLES) as Muscle[];
const isMuscle = (value: unknown): value is Muscle => typeof value === "string" && value in MUSCLES;

/** How an exercise is measured: sets of reps with a weight, sets of bodyweight reps, timed holds, or cardio. */
export type Mode = "weight" | "reps" | "time" | "cardio";
const MODES: Record<Mode, string> = {weight: msg("Weights"), reps: msg("Bodyweight"), time: msg("Holds"), cardio: msg("Cardio")};
type Defaults = {sets?: number; reps?: number; weight?: number; seconds?: number; minutes?: number; distance?: number; rest?: number};
export type Exercise = {id: string; name: string; mode: Mode; muscles: Muscle[]; secondary: Muscle[]; met: number; defaults: Defaults; custom?: boolean};

const ex = (id: string, name: string, mode: Mode, muscles: Muscle[], secondary: Muscle[], met: number, defaults: Defaults): Exercise => ({id, name, mode, muscles, secondary, met, defaults});
/** The built-in library. Names are message keys; MET values estimate calories. */
export const LIBRARY: Exercise[] = [
  ex("squat", msg("Barbell squat"), "weight", ["quads", "glutes"], ["hamstrings", "lowerBack", "abs"], 5, {sets: 5, reps: 5, weight: 60, rest: 120}),
  ex("deadlift", msg("Deadlift"), "weight", ["hamstrings", "glutes", "lowerBack"], ["traps", "forearms", "quads"], 6, {sets: 3, reps: 5, weight: 80, rest: 150}),
  ex("rdl", msg("Romanian deadlift"), "weight", ["hamstrings", "glutes"], ["lowerBack", "forearms"], 5, {sets: 3, reps: 10, weight: 50, rest: 90}),
  ex("bench", msg("Bench press"), "weight", ["chest"], ["triceps", "shoulders"], 5, {sets: 3, reps: 8, weight: 40, rest: 120}),
  ex("incline", msg("Incline dumbbell press"), "weight", ["chest", "shoulders"], ["triceps"], 5, {sets: 3, reps: 10, weight: 16, rest: 90}),
  ex("ohp", msg("Overhead press"), "weight", ["shoulders"], ["triceps", "traps"], 5, {sets: 3, reps: 8, weight: 30, rest: 90}),
  ex("row", msg("Barbell row"), "weight", ["lats", "traps"], ["biceps", "lowerBack", "forearms"], 5, {sets: 3, reps: 10, weight: 40, rest: 90}),
  ex("pullup", msg("Pull-up"), "reps", ["lats"], ["biceps", "forearms"], 8, {sets: 3, reps: 8, rest: 90}),
  ex("pushup", msg("Push-up"), "reps", ["chest"], ["triceps", "shoulders", "abs"], 3.8, {sets: 3, reps: 15, rest: 60}),
  ex("dip", msg("Dip"), "reps", ["triceps"], ["chest", "shoulders"], 5, {sets: 3, reps: 10, rest: 75}),
  ex("curl", msg("Biceps curl"), "weight", ["biceps"], ["forearms"], 3.5, {sets: 3, reps: 12, weight: 10, rest: 60}),
  ex("triext", msg("Triceps extension"), "weight", ["triceps"], [], 3.5, {sets: 3, reps: 12, weight: 12, rest: 60}),
  ex("lateral", msg("Lateral raise"), "weight", ["shoulders"], ["traps"], 3.5, {sets: 3, reps: 15, weight: 6, rest: 45}),
  ex("lunge", msg("Walking lunge"), "weight", ["quads", "glutes"], ["hamstrings", "calves"], 5, {sets: 3, reps: 12, weight: 10, rest: 60}),
  ex("legpress", msg("Leg press"), "weight", ["quads", "glutes"], ["hamstrings"], 5, {sets: 4, reps: 10, weight: 100, rest: 90}),
  ex("calf", msg("Calf raise"), "weight", ["calves"], [], 3, {sets: 4, reps: 15, weight: 40, rest: 45}),
  ex("hipthrust", msg("Hip thrust"), "weight", ["glutes"], ["hamstrings"], 4.5, {sets: 3, reps: 10, weight: 60, rest: 90}),
  ex("plank", msg("Plank"), "time", ["abs"], ["obliques", "shoulders"], 3, {sets: 3, seconds: 45, rest: 45}),
  ex("sideplank", msg("Side plank"), "time", ["obliques"], ["abs"], 3, {sets: 3, seconds: 30, rest: 30}),
  ex("crunch", msg("Crunch"), "reps", ["abs"], [], 3, {sets: 3, reps: 20, rest: 45}),
  ex("twist", msg("Russian twist"), "reps", ["obliques"], ["abs"], 3.5, {sets: 3, reps: 20, rest: 45}),
  ex("burpee", msg("Burpee"), "reps", ["quads", "chest"], ["shoulders", "abs", "calves"], 8, {sets: 3, reps: 12, rest: 60}),
  ex("run", msg("Run"), "cardio", ["quads", "hamstrings", "calves"], ["glutes"], 9.8, {minutes: 25, distance: 4}),
  ex("cycle", msg("Cycling"), "cardio", ["quads", "calves"], ["glutes", "hamstrings"], 7.5, {minutes: 30, distance: 10}),
  ex("rope", msg("Jump rope"), "cardio", ["calves"], ["shoulders", "forearms"], 11, {minutes: 10, distance: 0}),
  ex("rower", msg("Rowing machine"), "cardio", ["lats", "quads"], ["biceps", "hamstrings", "lowerBack"], 7, {minutes: 15, distance: 3})
];

/** A unit of training: one exercise and its parameters. Weights are in kg. */
export type Unit = {exercise: string} & Defaults;
type Routine = {id: string; name: string; units: Unit[]};
type SetLog = {reps?: number; weight?: number; seconds?: number; minutes?: number; distance?: number; done: boolean};
/** The session in progress, saved with the plugin so it survives a reload or a switch to the phone. */
type Active = {name: string; task: string | null; startedAt: number; units: Unit[]; sets: SetLog[][]; restUntil: number | null; restSpan: number};

const custom = (p: PluginItem): Exercise[] => (Array.isArray(p.state.exercises) ? p.state.exercises : [])
  .filter((item: Exercise) => item && typeof item.id === "string" && typeof item.name === "string")
  .map((item: Exercise) => ({...item, mode: item.mode in MODES ? item.mode : "reps", muscles: (item.muscles ?? []).filter(isMuscle), secondary: (item.secondary ?? []).filter(isMuscle), met: Number(item.met) || 4, defaults: item.defaults ?? {}, custom: true}));
const allExercises = (p: PluginItem) => [...LIBRARY, ...custom(p)];
function exerciseOf(p: PluginItem, id: string): Exercise {
  return allExercises(p).find(item => item.id === id) ?? {id, name: id, mode: "reps", muscles: [], secondary: [], met: 4, defaults: {}, custom: true};
}
const exName = (item: Exercise) => item.custom ? item.name : t(item.name);

const MODE_DEFAULTS: Record<Mode, Required<Defaults>> = {
  weight: {sets: 3, reps: 8, weight: 20, seconds: 0, minutes: 0, distance: 0, rest: 90},
  reps: {sets: 3, reps: 12, weight: 0, seconds: 0, minutes: 0, distance: 0, rest: 60},
  time: {sets: 3, reps: 0, weight: 0, seconds: 45, minutes: 0, distance: 0, rest: 45},
  cardio: {sets: 1, reps: 0, weight: 0, seconds: 0, minutes: 20, distance: 3, rest: 0}
};
/** Every parameter of a unit, defaults filled in from its exercise and its mode. */
function full(unit: Unit, item: Exercise): Required<Defaults> {
  const base = {...MODE_DEFAULTS[item.mode], ...item.defaults};
  const pick = (key: keyof Defaults) => { const value = Number(unit[key]); return Number.isFinite(value) && unit[key] !== undefined ? value : base[key]; };
  return {sets: Math.max(1, Math.round(pick("sets"))), reps: pick("reps"), weight: pick("weight"), seconds: pick("seconds"), minutes: pick("minutes"), distance: pick("distance"), rest: pick("rest")};
}
const unitFor = (item: Exercise): Unit => ({exercise: item.id, ...full({exercise: item.id}, item)});

// ----- Units of weight -------------------------------------------------------------------------------------

const LB = 2.20462;
const unitsOf = (p: PluginItem): "kg" | "lb" => p.state.units === "lb" ? "lb" : "kg";
/** A weight in kg as the user reads it, to the half. */
const shown = (p: PluginItem, kg: number) => round(unitsOf(p) === "lb" ? Math.round(kg * LB * 2) / 2 : Math.round(kg * 2) / 2, 1);
const fromShown = (p: PluginItem, value: number) => unitsOf(p) === "lb" ? value / LB : value;
const weightText = (p: PluginItem, kg: number) => `${num(shown(p, kg))} ${unitsOf(p)}`;
/** Body weight as the user reads it, to a tenth. */
const bodyShown = (p: PluginItem, kg: number) => round(unitsOf(p) === "lb" ? kg * LB : kg, 1);
const bodyText = (p: PluginItem, kg: number) => `${num(bodyShown(p, kg))} ${unitsOf(p)}`;
const goalOf = (p: PluginItem) => Math.max(1, Math.min(14, Math.round(Number(p.state.goal) || 3)));

/** A unit as one line, such as "Barbell squat · 5×5 @ 60 kg"; it names the steps of a planned workout. */
export function unitLabel(p: PluginItem, unit: Unit): string {
  const item = exerciseOf(p, unit.exercise);
  return `${exName(item)} · ${unitTarget(p, unit)}`;
}
function unitTarget(p: PluginItem, unit: Unit): string {
  const item = exerciseOf(p, unit.exercise);
  const v = full(unit, item);
  if (item.mode === "weight") return t("{sets}×{reps} @ {weight}", {sets: v.sets, reps: v.reps, weight: weightText(p, v.weight)});
  if (item.mode === "reps") return t("{sets}×{reps}", {sets: v.sets, reps: v.reps});
  if (item.mode === "time") return t("{sets}×{seconds} s", {sets: v.sets, seconds: v.seconds});
  return v.distance ? t("{minutes} min · {distance} km", {minutes: num(v.minutes), distance: num(v.distance)}) : t("{n} min", {n: num(v.minutes)});
}
/** How long a unit takes, in seconds: its work, its rests between sets, and a little to set up. */
function unitSeconds(unit: Unit, item: Exercise): number {
  const v = full(unit, item);
  if (item.mode === "cardio") return v.minutes * 60;
  const work = item.mode === "time" ? v.seconds : v.reps * 3.5;
  return v.sets * work + (v.sets - 1) * v.rest + 30;
}
type Stats = {minutes: number; volume: number; calories: number; sets: number; muscles: Muscle[]};
/** What a workout adds up to: minutes, volume (kg lifted), calories, sets, and the muscles it works most. */
function workoutStats(p: PluginItem, units: Unit[]): Stats {
  const body = bodyWeight(p);
  let seconds = 0, volume = 0, calories = 0, sets = 0;
  const load = new Map<Muscle, number>();
  for (const unit of units) {
    const item = exerciseOf(p, unit.exercise);
    const v = full(unit, item);
    const time = unitSeconds(unit, item);
    seconds += time;
    calories += item.met * body * time / 3600;
    if (item.mode === "weight") volume += v.sets * v.reps * v.weight;
    if (item.mode !== "cardio") sets += v.sets;
    for (const muscle of item.muscles) load.set(muscle, (load.get(muscle) ?? 0) + v.sets);
    for (const muscle of item.secondary) load.set(muscle, (load.get(muscle) ?? 0) + v.sets / 2);
  }
  const muscles = [...load].sort((a, b) => b[1] - a[1]).map(([muscle]) => muscle);
  return {minutes: Math.round(seconds / 60), volume, calories: Math.round(calories), sets, muscles};
}

// ----- What was done ---------------------------------------------------------------------------------------

type Session = {entry: PluginLogEntry; at: string; name: string; minutes: number; sets: number; volume: number; calories: number; units: {exercise: string; sets: SetLog[]}[]};

/** Body weight in kg: the latest measurement, or 70 until there is one. */
function bodyWeight(p: PluginItem): number {
  const latest = [...p.log].reverse().find(entry => entry.kind === "body" && Number(entry.data?.weight) > 0);
  return latest ? Number(latest.data.weight) : 70;
}
/** Workouts done, newest first: sessions with every set, and planned workouts completed from the board (their
 *  units counted as planned, those whose step was left unchecked left out). */
function sessions(p: PluginItem): Session[] {
  const list: Session[] = [];
  for (const entry of p.log) {
    const data = entry.data ?? {};
    if (entry.kind === "session") {
      const units = Array.isArray(data.units) ? data.units : [];
      list.push({entry, at: entry.at, name: String(data.name ?? entry.task ?? t("Workout")), minutes: Number(data.minutes) || 0, sets: Number(data.sets) || 0, volume: Number(data.volume) || 0, calories: Number(data.calories) || 0, units});
    } else if (entry.kind === "task") {
      const planned: Unit[] = Array.isArray(data.plugin?.workout?.units) ? data.plugin.workout.units : [];
      const steps: Subtask[] = Array.isArray(data.subtasks) ? data.subtasks : [];
      const done = planned.filter((_, i) => !steps.length || steps[i]?.done !== false);
      const stats = workoutStats(p, done);
      const units = done.map(unit => {
        const v = full(unit, exerciseOf(p, unit.exercise));
        return {exercise: unit.exercise, sets: Array.from({length: v.sets}, () => ({reps: v.reps, weight: v.weight, seconds: v.seconds, minutes: v.minutes, distance: v.distance, done: true}))};
      });
      list.push({entry, at: entry.at, name: String(data.plugin?.workout?.name ?? entry.task ?? t("Workout")), minutes: stats.minutes, sets: stats.sets, volume: stats.volume, calories: stats.calories, units});
    }
  }
  return list.sort((a, b) => b.at.localeCompare(a.at));
}
/** Sets per muscle over the last `days` days (a secondary muscle counts half). */
function muscleLoad(p: PluginItem, today: string, days = 7): Partial<Record<Muscle, number>> {
  const from = addDays(today, -(days - 1));
  const load: Partial<Record<Muscle, number>> = {};
  for (const session of sessions(p)) {
    if (session.at.slice(0, 10) < from) continue;
    for (const unit of session.units) {
      const item = exerciseOf(p, unit.exercise);
      const count = item.mode === "cardio" ? 2 : unit.sets.filter(set => set.done !== false).length;
      for (const muscle of item.muscles) load[muscle] = (load[muscle] ?? 0) + count;
      for (const muscle of item.secondary) load[muscle] = (load[muscle] ?? 0) + count / 2;
    }
  }
  return load;
}
const lastTrained = (p: PluginItem, muscle: Muscle) => sessions(p).find(session => session.units.some(unit => {
  const item = exerciseOf(p, unit.exercise);
  return item.muscles.includes(muscle) || item.secondary.includes(muscle);
}))?.at ?? null;
/** Each of the last `count` weeks: workouts, minutes, and volume. */
function weekly(p: PluginItem, today: string, count = 12) {
  const weeks = recentWeeks(today, count).map(week => ({week, sessions: 0, minutes: 0, volume: 0}));
  const index = new Map(weeks.map((week, i) => [week.week, i]));
  for (const session of sessions(p)) {
    const at = index.get(weekStart(session.at));
    if (at === undefined) continue;
    weeks[at].sessions += 1;
    weeks[at].minutes += session.minutes;
    weeks[at].volume += session.volume;
  }
  return weeks;
}
/** Weeks in a row that reached the goal, counting this week once it has. */
function streak(p: PluginItem, today: string): number {
  const weeks = weekly(p, today, 52);
  const goal = goalOf(p);
  let count = weeks[weeks.length - 1].sessions >= goal ? 1 : 0;
  for (let i = weeks.length - 2; i >= 0 && weeks[i].sessions >= goal; i--) count++;
  return count;
}
type Record_ = {exercise: Exercise; best: string; score: number; at: string; e1rm: number | null};
/** The best set of each exercise: the highest estimated one-rep max (Epley) with weights, the most reps,
 *  the longest hold, the farthest or longest cardio. Most recent records first. */
function records(p: PluginItem): Record_[] {
  const best = new Map<string, Record_>();
  for (const session of [...sessions(p)].reverse()) {
    for (const unit of session.units) {
      const item = exerciseOf(p, unit.exercise);
      for (const set of unit.sets.filter(entry => entry.done !== false)) {
        const reps = Number(set.reps) || 0, weight = Number(set.weight) || 0;
        const [score, text, e1rm] = item.mode === "weight" ? [weight * (1 + (reps > 1 ? reps / 30 : 0)), t("{reps} × {weight}", {reps, weight: weightText(p, weight)}), weight * (1 + (reps > 1 ? reps / 30 : 0))]
          : item.mode === "reps" ? [reps, tn("{count} rep", "{count} reps", reps), null]
          : item.mode === "time" ? [Number(set.seconds) || 0, t("{n} s", {n: Number(set.seconds) || 0}), null]
          : [(Number(set.distance) || 0) * 100 + (Number(set.minutes) || 0), Number(set.distance) ? t("{n} km", {n: num(Number(set.distance))}) : t("{n} min", {n: num(Number(set.minutes) || 0)}), null];
        if (score > 0 && score > (best.get(item.id)?.score ?? 0)) best.set(item.id, {exercise: item, best: text, score, at: session.at, e1rm});
      }
    }
  }
  return [...best.values()].sort((a, b) => b.at.localeCompare(a.at));
}

// ----- The body figure -------------------------------------------------------------------------------------

type Shape = string;
const mirror = (shape: Shape) => `<g transform="scale(-1 1)">${shape}</g>`;
const both = (shape: Shape) => shape + mirror(shape);
const FRONT: [Muscle, Shape][] = [
  ["shoulders", both(`<ellipse cx="44" cy="77" rx="11" ry="13" transform="rotate(-14 44 77)"/>`)],
  ["chest", both(`<path d="M2 70 L27 65 Q40 69 38 86 Q34 102 16 104 Q4 104 2 94 Z"/>`)],
  ["biceps", both(`<ellipse cx="50" cy="113" rx="7.5" ry="19" transform="rotate(-8 50 113)"/>`)],
  ["forearms", both(`<ellipse cx="58" cy="177" rx="6.5" ry="24" transform="rotate(-6 58 177)"/>`)],
  ["abs", both(`<rect x="2.5" y="109" width="12" height="15" rx="4"/><rect x="2.5" y="127" width="12" height="15" rx="4"/><rect x="2.5" y="145" width="12" height="15" rx="4"/><rect x="2.5" y="163" width="11" height="17" rx="4"/>`)],
  ["obliques", both(`<path d="M18 110 Q30 115 33 132 Q32 156 30 178 Q22 172 18 158 Z"/>`)],
  ["quads", both(`<path d="M8 202 Q22 192 33 204 Q37 236 30 270 Q22 278 15 270 Q7 238 8 202 Z"/>`)],
  ["calves", both(`<ellipse cx="22" cy="310" rx="7.5" ry="23"/>`)]
];
const BACK: [Muscle, Shape][] = [
  ["traps", `<path d="M0 50 L-25 67 Q-13 82 0 114 Q13 82 25 67 Z"/>`],
  ["shoulders", both(`<ellipse cx="44" cy="78" rx="11" ry="13" transform="rotate(-14 44 78)"/>`)],
  ["lats", both(`<path d="M9 94 Q30 84 40 97 Q38 125 26 152 Q16 142 8 122 Z"/>`)],
  ["triceps", both(`<ellipse cx="51" cy="113" rx="7.5" ry="19" transform="rotate(-8 51 113)"/>`)],
  ["forearms", both(`<ellipse cx="58" cy="177" rx="6.5" ry="24" transform="rotate(-6 58 177)"/>`)],
  ["lowerBack", `<rect x="-13" y="136" width="26" height="42" rx="9"/>`],
  ["glutes", both(`<ellipse cx="15" cy="203" rx="16" ry="17"/>`)],
  ["hamstrings", both(`<path d="M9 222 Q22 214 33 224 Q35 250 29 274 Q21 280 15 272 Q8 248 9 222 Z"/>`)],
  ["calves", both(`<ellipse cx="22" cy="306" rx="10" ry="26"/>`)]
];
const SILHOUETTE = `<ellipse cx="0" cy="28" rx="17" ry="21"/><rect x="-8" y="44" width="16" height="18" rx="5"/>
  <path d="M-46 72 Q-45 60 -30 57 L30 57 Q45 60 46 72 L42 128 Q36 160 37 186 L-37 186 Q-36 160 -42 128 Z"/>
  <path d="M-38 182 L38 182 L36 216 Q0 228 -36 216 Z"/>
  ${both(`<line class="fb-limb" x1="45" y1="72" x2="55" y2="146" stroke-width="19"/><line class="fb-limb" x1="55" y1="146" x2="62" y2="210" stroke-width="14"/><circle cx="63" cy="222" r="8"/><line class="fb-limb" x1="20" y1="200" x2="23" y2="282" stroke-width="30"/><line class="fb-limb" x1="23" y1="282" x2="21" y2="346" stroke-width="18"/><ellipse cx="25" cy="358" rx="12" ry="6"/>`)}`;
const heat = (sets: number) => sets <= 0 ? 0 : sets < 3 ? 1 : sets < 6 ? 2 : sets < 10 ? 3 : 4;

/** The front and back of a body, each muscle shaded by how many sets worked it this week. Muscles are buttons
 *  (`data-muscle`) when `interactive`. */
export function bodyFigure(options: {load: Partial<Record<Muscle, number>>; selected?: Muscle | null; highlight?: Muscle[]; interactive?: boolean; label: string; tips?: (muscle: Muscle) => string}): string {
  const {load, selected, interactive = true} = options;
  const lit = new Set(options.highlight ?? []);
  const group = (muscle: Muscle, shape: Shape) => {
    const name = t(MUSCLES[muscle]);
    const tip = options.tips?.(muscle) ?? name;
    const classes = `fb-muscle h${heat(load[muscle] ?? 0)} ${selected === muscle ? "is-selected" : ""} ${lit.has(muscle) ? "is-lit" : ""}`;
    return interactive
      ? `<g class="${classes}" data-muscle="${muscle}" data-tip="${e(tip)}" role="button" tabindex="0" aria-pressed="${selected === muscle}" aria-label="${e(tip)}">${shape}</g>`
      : `<g class="${classes}">${shape}</g>`;
  };
  const figure = (parts: [Muscle, Shape][], x: number, caption: string) => `<g transform="translate(${x} 8)"><g class="fb-base">${SILHOUETTE}</g>${parts.map(([muscle, shape]) => group(muscle, shape)).join("")}<text class="fb-caption" x="0" y="384" text-anchor="middle">${e(caption)}</text></g>`;
  return `<svg class="body-figure" viewBox="0 0 360 396" role="${interactive ? "group" : "img"}" aria-label="${e(options.label)}">${figure(FRONT, 90, t("Front"))}${figure(BACK, 270, t("Back"))}</svg>`;
}
const heatLegend = () => `<div class="fb-legend" aria-hidden="true"><span>${t("Rested")}</span>${[0, 1, 2, 3, 4].map(level => `<i class="h${level}"></i>`).join("")}<span>${t("Worked hard")}</span></div>`;

// ----- The page ----------------------------------------------------------------------------------------------

type Tab = "train" | "exercises" | "progress";
type View = {tab: Tab; muscle: Muscle | null; mode: Mode | "all"; query: string; draft: {name: string; units: Unit[]}; timed: boolean; due: string; adding: boolean};
const viewOf = (p: PluginItem) => view<View>(p.id, () => ({
  tab: recall<Tab>(`${p.id}-tab`, "train"), muscle: null, mode: "all", query: "", timed: false, due: "", adding: false,
  draft: recall(`${p.id}-draft`, {name: "", units: [] as Unit[]})
}));
const saveDraft = (p: PluginItem, v: View) => remember(`${p.id}-draft`, v.draft);
const activeOf = (p: PluginItem): Active | null => {
  const a = p.state.active;
  return a && typeof a === "object" && Array.isArray(a.units) && Array.isArray(a.sets) ? a as Active : null;
};
const routinesOf = (p: PluginItem): Routine[] => (Array.isArray(p.state.routines) ? p.state.routines : []).filter((item: Routine) => item && Array.isArray(item.units));
const muscleChips = (muscles: Muscle[], limit = 4) => muscles.slice(0, limit).map(muscle => `<span class="fit-chip">${t(MUSCLES[muscle])}</span>`).join("");

function hero(p: PluginItem, today: string): string {
  const weeks = weekly(p, today, 2);
  const now = weeks[1];
  const goal = goalOf(p);
  const left = Math.max(0, goal - now.sessions);
  const load = muscleLoad(p, today);
  const v = viewOf(p);
  const calories = sessions(p).filter(session => weekStart(session.at) === now.week).reduce((sum, session) => sum + session.calories, 0);
  const tips = (muscle: Muscle) => {
    const sets = Math.round(load[muscle] ?? 0);
    const last = lastTrained(p, muscle);
    const when = last ? t("last {date}", {date: formatDate(last, {weekday: "short", month: "short", day: "numeric"})}) : t("not trained yet");
    return `${t(MUSCLES[muscle])} · ${tn("{count} set this week", "{count} sets this week", sets)} · ${when}`;
  };
  return `<section class="fit-hero">
    <article class="card fit-week"><p class="eyebrow">${icons.flame} ${t("THIS WEEK")}</p>
      <div class="fit-week-main">${ring({value: now.sessions / goal, size: 128, label: t("{done} of {goal} workouts this week", {done: now.sessions, goal}), center: `${now.sessions}<small>/${goal}</small>`, tone: "sw-orange"})}
        <div class="pk-tiles">${tile(t("Minutes"), num(now.minutes, 0))}${tile(t("Volume"), `${compact(shown(p, now.volume))}<small>${unitsOf(p)}</small>`)}${tile(t("Calories"), num(calories, 0), t("estimated"))}${tile(t("Streak"), `${streak(p, today)}`, t("weeks at goal"))}</div></div>
      <p class="fit-week-note">${left ? tn("{count} more workout to reach your weekly goal.", "{count} more workouts to reach your weekly goal.", left) : `${icons.trophy} ${t("Weekly goal reached. Strong work.")}`}</p>
    </article>
    <article class="card fit-figure-card"><div class="section-heading compact"><div><h3>${t("Muscles this week")}</h3><p class="muted">${t("Pick a muscle to find exercises for it.")}</p></div>${v.muscle ? `<button type="button" class="chip active-day" data-muscle="${v.muscle}">${e(t(MUSCLES[v.muscle]))}${icons.close}</button>` : ""}</div>
      ${bodyFigure({load, selected: v.muscle, label: t("Muscles worked this week"), tips})}${heatLegend()}</article>
  </section>`;
}
const tabs = (v: View) => `<div class="layout-switch fit-tabs" role="tablist" aria-label="${t("Fitness sections")}">${([["train", t("Train"), icons.zap], ["exercises", t("Exercises"), icons.grid], ["progress", t("Progress"), icons.up]] as const).map(([id, label, image]) => `<button type="button" role="tab" data-fit-tab="${id}" aria-selected="${v.tab === id}" aria-pressed="${v.tab === id}">${image}<span>${label}</span></button>`).join("")}</div>`;

/** The adjustable parameters of a unit, as steppers. `prefix` keeps fields apart (a builder unit, a set). */
function unitSteppers(p: PluginItem, unit: Unit, prefix: string, small = false): string {
  const item = exerciseOf(p, unit.exercise);
  const v = full(unit, item);
  const lb = unitsOf(p) === "lb";
  const s = (field: string, value: number, step: number, min: number, max: number, label: string, unitName?: string) => stepper({field: `${prefix}${field}`, value, step, min, max, label, unit: unitName, small, caption: true});
  const parts = item.mode === "cardio"
    ? [s("minutes", v.minutes, 1, 1, 600, t("Minutes"), t("min")), s("distance", v.distance, 0.5, 0, 300, t("Distance"), "km")]
    : [s("sets", v.sets, 1, 1, 20, t("Sets")),
      item.mode === "time" ? s("seconds", v.seconds, 5, 5, 900, t("Seconds"), t("s")) : s("reps", v.reps, 1, 1, 200, t("Reps")),
      ...(item.mode === "weight" ? [s("weight", shown(p, v.weight), lb ? 5 : 2.5, 0, lb ? 1500 : 700, t("Weight"), unitsOf(p))] : []),
      s("rest", v.rest, 15, 0, 600, t("Rest"), t("s"))];
  return `<div class="fit-params">${parts.join("")}</div>`;
}
function builderSummary(p: PluginItem, units: Unit[]): string {
  if (!units.length) return `<p class="muted">${t("Add exercises to see how long it takes and what it works.")}</p>`;
  const stats = workoutStats(p, units);
  return `<div class="fit-summary-stats"><span>${icons.clock}<b>~${stats.minutes}</b> ${t("min")}</span><span>${icons.zap}<b>${stats.sets}</b> ${t("sets")}</span>${stats.volume ? `<span>${icons.scale}<b>${compact(shown(p, stats.volume))}</b> ${unitsOf(p)}</span>` : ""}<span>${icons.flame}<b>~${stats.calories}</b> ${t("kcal")}</span></div><div class="fit-summary-muscles">${muscleChips(stats.muscles, 6)}</div>`;
}
function builder(p: PluginItem, v: View, s: Snapshot): string {
  const units = v.draft.units;
  const cards = units.map((unit, i) => {
    const item = exerciseOf(p, unit.exercise);
    return `<li class="fit-unit mode-${item.mode}" data-unit="${i}" style="--i:${i}"><div class="fit-unit-head"><span class="fit-unit-num">${i + 1}</span><div class="fit-unit-title"><strong>${e(exName(item))}</strong><span>${muscleChips(item.muscles, 3)}</span></div><div class="fit-unit-tools"><button type="button" class="icon-button" data-unit-move="${i}" data-by="-1" aria-label="${t("Move up")}" ${i === 0 ? "disabled" : ""}>${kitIcons.up}</button><button type="button" class="icon-button" data-unit-move="${i}" data-by="1" aria-label="${t("Move down")}" ${i === units.length - 1 ? "disabled" : ""}>${kitIcons.down}</button><button type="button" class="icon-button danger" data-unit-remove="${i}" aria-label="${e(t("Remove {exercise}", {exercise: exName(item)}))}">${icons.close}</button></div></div>${unitSteppers(p, unit, `u:${i}:`)}</li>`;
  }).join("");
  const grouped = MUSCLE_IDS.map(muscle => {
    const list = allExercises(p).filter(item => item.muscles[0] === muscle);
    return list.length ? `<optgroup label="${e(t(MUSCLES[muscle]))}">${list.map(item => `<option value="${e(item.id)}">${e(exName(item))}</option>`).join("")}</optgroup>` : "";
  }).join("");
  const free = s.tasks.some(task => task.description === (v.draft.name.trim() || t("Workout")));
  return `<article class="card fit-builder"><div class="section-heading compact"><div><h2>${icons.edit} ${t("Workout builder")}</h2><p class="muted">${t("Adjust each unit with − and +. Weights are in {units}.", {units: unitsOf(p)})}</p></div>${units.length ? `<button type="button" class="text-button" data-fit-clear>${t("Clear")}</button>` : ""}</div>
    <label class="fit-name">${t("Name")}<input name="fitDraftName" maxlength="80" placeholder="${t("Push day, 5 km run…")}" value="${e(v.draft.name)}"></label>
    ${units.length ? `<ol class="fit-units">${cards}</ol>` : `<div class="fit-empty">${icons.zap}<p>${t("Start from a routine, pick exercises in the library, or add one here.")}</p></div>`}
    <label class="fit-add"><span class="sr-only">${t("Add an exercise")}</span><select name="fitAdd"><option value="">${t("+ Add an exercise…")}</option>${grouped}</select></label>
    <div class="fit-summary">${builderSummary(p, units)}</div>
    <fieldset class="timed-field ${v.timed ? "is-on" : ""}"><legend class="sr-only">${t("Timer")}</legend><label class="timed-toggle"><input type="checkbox" name="fitTimed" ${v.timed ? "checked" : ""}><span class="timed-switch" aria-hidden="true"></span><span class="timed-copy"><strong>${icons.clock}${t("Plan it for a time")}</strong><small>${t("Due by tonight, it gets a timer above the board.")}</small></span></label><label class="timed-due">${t("Finish by")}<input type="datetime-local" name="fitDue" value="${e(v.due || defaultDue())}" ${v.timed ? "" : "disabled"}></label></fieldset>
    <div class="button-row fit-actions"><button type="button" class="button primary" data-fit-start-draft ${units.length ? "" : "disabled"}>${icons.zap}<span>${t("Start now")}</span></button><button type="button" class="button" data-fit-plan ${units.length ? "" : "disabled"}>${icons.tasks}<span>${t("Plan as a task")}</span></button><button type="button" class="button ghost" data-fit-log-draft ${units.length ? "" : "disabled"} title="${t("Record it as done now, without the live session")}">${icons.check}<span>${t("Log as done")}</span></button><button type="button" class="button ghost" data-fit-save-routine ${units.length ? "" : "disabled"}>${icons.copy}<span>${t("Save as routine")}</span></button></div>
    ${free ? `<p class="muted fit-note">${t("A task with this name is already planned; a new one gets a number.")}</p>` : ""}
  </article>`;
}
function routinesCard(p: PluginItem): string {
  const list = routinesOf(p);
  return `<article class="card fit-routines"><div class="section-heading compact"><h3>${icons.repeat} ${t("Routines")} <span class="count">${list.length}</span></h3></div>${list.length ? `<div class="fit-routine-list">${list.map((routine, i) => {
    const stats = workoutStats(p, routine.units);
    return `<div class="fit-routine" style="--i:${i}"><div class="fit-routine-top"><strong>${e(routine.name)}</strong><button type="button" class="icon-button danger" data-routine-delete="${e(routine.id)}" aria-label="${e(t("Delete the routine {name}", {name: routine.name}))}">${icons.close}</button></div><p class="muted">${tn("{count} exercise", "{count} exercises", routine.units.length)} · ~${stats.minutes} ${t("min")}</p><div class="fit-summary-muscles">${muscleChips(stats.muscles, 3)}</div><div class="fit-routine-actions"><button type="button" class="button small primary" data-routine-start="${e(routine.id)}">${icons.zap}<span>${t("Start")}</span></button><button type="button" class="button small" data-routine-plan="${e(routine.id)}">${icons.tasks}<span>${t("Plan")}</span></button><button type="button" class="button small ghost" data-routine-load="${e(routine.id)}">${icons.edit}<span>${t("Edit")}</span></button></div></div>`;
  }).join("")}</div>` : empty(t("Save a workout from the builder to reuse it."))}</article>`;
}
function plannedCard(p: PluginItem, host: PluginHost): string {
  const s = host.snapshot();
  const planned = s.tasks.map((task, index) => ({task, index})).filter(({task}) => task.type === p.type);
  return `<article class="card fit-planned"><div class="section-heading compact"><div><h3>${icons.tasks} ${t("Planned workouts")} <span class="count">${planned.length}</span></h3><p class="muted">${t("They are tasks in the {type} group on the board.", {type: e(p.name)})}</p></div></div>${planned.length ? `<div class="task-cards plugin-tasks">${planned.map(({task, index}, i) => host.taskCard(task, index, i)).join("")}</div>` : empty(t("Plan a workout from the builder or a routine; it shows up on the board."))}</article>`;
}
function libraryTab(p: PluginItem, v: View): string {
  const query = v.query.trim().toLowerCase();
  const list = allExercises(p).filter(item => (!v.muscle || item.muscles.includes(v.muscle) || item.secondary.includes(v.muscle)) && (v.mode === "all" || item.mode === v.mode) && (!query || exName(item).toLowerCase().includes(query)))
    .sort((a, b) => Number(v.muscle ? !a.muscles.includes(v.muscle) : 0) - Number(v.muscle ? !b.muscles.includes(v.muscle) : 0));
  const inDraft = (id: string) => v.draft.units.filter(unit => unit.exercise === id).length;
  const cards = list.map((item, i) => {
    const count = inDraft(item.id);
    return `<article class="fit-exercise mode-${item.mode} ${count ? "is-added" : ""}" style="--i:${Math.min(i, 20)}"><div class="fit-exercise-top"><span class="fit-mode-badge">${t(MODES[item.mode])}</span>${item.custom ? `<button type="button" class="icon-button danger" data-ex-delete="${e(item.id)}" aria-label="${e(t("Delete {exercise}", {exercise: exName(item)}))}">${icons.close}</button>` : ""}</div><h4>${e(exName(item))}</h4><div class="fit-exercise-muscles">${item.muscles.map(muscle => `<span class="fit-chip strong">${t(MUSCLES[muscle])}</span>`).join("")}${item.secondary.slice(0, 3).map(muscle => `<span class="fit-chip">${t(MUSCLES[muscle])}</span>`).join("")}</div><p class="muted">${e(unitTarget(p, unitFor(item)))}</p><button type="button" class="button small ${count ? "" : "primary"}" data-ex-add="${e(item.id)}">${icons.plus}<span>${count ? t("Add again ({count} in workout)", {count}) : t("Add to workout")}</span></button></article>`;
  }).join("");
  const form = v.adding ? `<form class="card fit-new-exercise" autocomplete="off"><h3>${t("Your own exercise")}</h3><label>${t("Name")}<input name="exName" required maxlength="60"></label><label>${t("Measured by")}<select name="exMode">${(Object.keys(MODES) as Mode[]).map(mode => `<option value="${mode}">${t(MODES[mode])}</option>`).join("")}</select></label><fieldset class="fit-muscle-picks"><legend>${t("Muscles it works")}</legend>${MUSCLE_IDS.map(muscle => `<label class="chip"><input type="checkbox" name="exMuscle" value="${muscle}" ${v.muscle === muscle ? "checked" : ""}>${t(MUSCLES[muscle])}</label>`).join("")}</fieldset><div class="button-row"><button type="submit" class="button primary">${icons.check}<span>${t("Add exercise")}</span></button><button type="button" class="button ghost" data-ex-cancel>${t("Cancel")}</button></div></form>` : "";
  return `<div class="fit-library"><div class="gallery-filters" role="toolbar" aria-label="${t("Filter exercises")}"><div class="chip-row" role="group" aria-label="${t("Muscle")}"><button type="button" class="chip" data-muscle-filter="" aria-pressed="${!v.muscle}">${t("All muscles")}</button>${MUSCLE_IDS.map(muscle => `<button type="button" class="chip" data-muscle-filter="${muscle}" aria-pressed="${v.muscle === muscle}">${t(MUSCLES[muscle])}</button>`).join("")}</div><div class="chip-row" role="group" aria-label="${t("Kind")}"><button type="button" class="chip" data-fit-mode="all" aria-pressed="${v.mode === "all"}">${t("Everything")}</button>${(Object.keys(MODES) as Mode[]).map(mode => `<button type="button" class="chip" data-fit-mode="${mode}" aria-pressed="${v.mode === mode}">${t(MODES[mode])}</button>`).join("")}</div><label class="search-pill">${icons.search}<span class="sr-only">${t("Search exercises")}</span><input name="fitSearch" placeholder="${t("Find an exercise…")}" value="${e(v.query)}"></label>${v.adding ? "" : `<button type="button" class="button small" data-ex-new>${icons.plus}<span>${t("Your own exercise")}</span></button>`}</div>${form}<div class="fit-exercise-grid" id="fit-exercises">${cards || empty(t("No exercise matches. Try another muscle or kind, or add your own."))}</div></div>`;
}
function progressTab(p: PluginItem, today: string): string {
  const weeks = weekly(p, today, 12);
  const labels = weeks.map(week => formatDate(week.week, {month: "numeric", day: "numeric"}));
  const goal = goalOf(p);
  const body = p.log.filter(entry => entry.kind === "body" && Number(entry.data?.weight) > 0);
  const bodyGoal = Number(p.state.bodyGoal) || 0;
  const recordsList = records(p).slice(0, 8);
  const latest = body.length ? Number(body[body.length - 1].data.weight) : null;
  const chart = line({points: body.slice(-60).map(entry => ({at: entry.at, value: bodyShown(p, Number(entry.data.weight))})), target: bodyGoal ? bodyShown(p, bodyGoal) : null, label: t("Body weight over time"), format: value => `${num(value)} ${unitsOf(p)}`});
  return `<section class="fit-progress">
    <article class="card fit-prefs"><h3>${icons.settings} ${t("Preferences")}</h3><div class="setting-block"><div class="setting-copy"><strong>${t("Weight units")}</strong></div><div class="layout-switch" role="group" aria-label="${t("Weight units")}">${(["kg", "lb"] as const).map(unit => `<button type="button" data-fit-units="${unit}" aria-pressed="${unitsOf(p) === unit}">${unit}</button>`).join("")}</div></div><div class="setting-block"><div class="setting-copy"><strong>${t("Workouts per week")}</strong><small>${t("Your weekly goal.")}</small></div>${stepper({field: "goal", value: goal, step: 1, min: 1, max: 14, label: t("Workouts per week")})}</div></article>
    <article class="card fit-chart"><div class="section-heading compact"><div><h3>${t("Workouts per week")}</h3><p class="muted">${t("The last 12 weeks, with your goal.")}</p></div></div>${bars({values: weeks.map(week => week.sessions), labels, tips: weeks.map((week, i) => t("Week of {date} · {count} workouts", {date: labels[i], count: week.sessions})), goal, label: t("Workouts per week"), tone: "sw-orange"})}</article>
    <article class="card fit-chart"><div class="section-heading compact"><div><h3>${t("Volume per week")}</h3><p class="muted">${t("Weight × reps of every set, in {units}.", {units: unitsOf(p)})}</p></div></div>${bars({values: weeks.map(week => shown(p, week.volume)), labels, tips: weeks.map((week, i) => `${labels[i]} · ${compact(shown(p, week.volume))} ${unitsOf(p)}`), label: t("Volume per week"), tone: "sw-violet"})}</article>
    <article class="card fit-records"><div class="section-heading compact"><h3>${icons.trophy} ${t("Personal records")}</h3></div>${recordsList.length ? `<ol class="fit-record-list">${recordsList.map((record, i) => `<li style="--i:${i}"><span class="fit-medal">${i < 3 ? icons.trophy : icons.flag}</span><div><strong>${e(exName(record.exercise))}</strong><small>${e(formatDate(record.at, {month: "short", day: "numeric", year: "numeric"}))}</small></div><b>${e(record.best)}</b>${record.e1rm ? `<em data-tip="${t("Estimated one-rep max (Epley)")}">${t("1RM ~{weight}", {weight: e(weightText(p, record.e1rm))})}</em>` : ""}</li>`).join("")}</ol>` : empty(t("Finish a workout to set your first records."))}</article>
    <article class="card fit-body"><div class="section-heading compact"><div><h3>${icons.scale} ${t("Body weight")}</h3><p class="muted">${latest ? t("Latest {weight}", {weight: e(bodyText(p, latest))}) : t("Log your weight to see the trend.")}${bodyGoal ? ` · ${t("goal {weight}", {weight: e(bodyText(p, bodyGoal))})}` : ""}</p></div></div>
      ${chart || `<p class="muted fit-chart-empty">${t("Two measurements draw a line.")}</p>`}
      <form class="fit-body-form" autocomplete="off">${stepper({field: "bodyWeight", value: bodyShown(p, latest ?? 70), step: unitsOf(p) === "lb" ? 0.5 : 0.1, min: 20, max: unitsOf(p) === "lb" ? 900 : 400, label: t("Weight"), unit: unitsOf(p), digits: 1})}<label class="fit-fat">${t("Body fat")}<input type="number" name="bodyFat" min="2" max="70" step="0.1" placeholder="%" inputmode="decimal"></label><button type="submit" class="button small primary">${icons.plus}<span>${t("Log weight")}</span></button></form>
      <div class="setting-block"><div class="setting-copy"><strong>${t("Goal weight")}</strong><small>${t("0 means no goal.")}</small></div>${stepper({field: "bodyGoal", value: bodyGoal ? bodyShown(p, bodyGoal) : 0, step: 0.5, min: 0, max: unitsOf(p) === "lb" ? 900 : 400, label: t("Goal weight"), unit: unitsOf(p), digits: 1})}</div></article>
    <article class="card fit-history">${recentSessions(p, 10, true)}</article>
  </section>`;
}
function recentSessions(p: PluginItem, limit: number, deletable: boolean): string {
  const list = sessions(p).slice(0, limit);
  return `<div class="section-heading compact"><h3>${icons.activity} ${t("Recent workouts")}</h3></div>${list.length ? `<ol class="fit-session-list">${list.map((session, i) => `<li style="--i:${i}"><time>${e(formatDate(session.at, {weekday: "short", month: "short", day: "numeric"}))}</time><strong>${e(session.name)}</strong><span>${t("{n} min", {n: num(session.minutes, 0)})}</span><span>${tn("{count} set", "{count} sets", session.sets)}</span><span>${session.volume ? `${compact(shown(p, session.volume))} ${unitsOf(p)}` : ""}</span>${deletable ? `<button type="button" class="icon-button danger" data-session-delete="${e(session.entry.id)}" aria-label="${e(t("Delete {name} from the log", {name: session.name}))}">${icons.close}</button>` : ""}</li>`).join("")}</ol>` : empty(t("Your workouts will be listed here."))}`;
}

// ----- The live session --------------------------------------------------------------------------------------

function sessionPanel(p: PluginItem, a: Active, now: number): string {
  const total = a.sets.reduce((sum, sets) => sum + sets.length, 0);
  const done = a.sets.reduce((sum, sets) => sum + sets.filter(set => set.done).length, 0);
  const current = a.sets.findIndex(sets => sets.some(set => !set.done));
  const resting = a.restUntil && a.restUntil > now;
  const rest = resting ? `<div class="fit-rest" data-rest><svg viewBox="0 0 48 48" aria-hidden="true"><circle class="dial-track" cx="24" cy="24" r="20.5"/><circle class="dial-time" cx="24" cy="24" r="20.5" pathLength="100" data-rest-ring stroke-dasharray="${(((a.restUntil! - now) / (a.restSpan || 1)) * 100).toFixed(2)} 100"/></svg><div><span>${t("Rest")}</span><b data-until="${a.restUntil}" data-span="${a.restSpan}">0:00</b></div><button type="button" class="button small" data-rest-add>+15 ${t("s")}</button><button type="button" class="button small ghost" data-rest-skip>${t("Skip rest")}</button></div>` : "";
  const units = a.units.map((unit, u) => {
    const item = exerciseOf(p, unit.exercise);
    const sets = a.sets[u] ?? [];
    const finished = sets.length > 0 && sets.every(set => set.done);
    const rows = sets.map((set, i) => {
      const setUnit: Unit = {exercise: unit.exercise, ...set, sets: 1};
      const fields = item.mode === "cardio"
        ? [stepper({field: `s:${u}:${i}:minutes`, value: full(setUnit, item).minutes, step: 1, min: 0, max: 600, label: t("Minutes"), unit: t("min"), small: true}), stepper({field: `s:${u}:${i}:distance`, value: full(setUnit, item).distance, step: 0.5, min: 0, max: 300, label: t("Distance"), unit: "km", small: true})]
        : item.mode === "time" ? [stepper({field: `s:${u}:${i}:seconds`, value: full(setUnit, item).seconds, step: 5, min: 0, max: 900, label: t("Seconds"), unit: t("s"), small: true})]
        : [stepper({field: `s:${u}:${i}:reps`, value: full(setUnit, item).reps, step: 1, min: 0, max: 200, label: t("Reps"), small: true}), ...(item.mode === "weight" ? [stepper({field: `s:${u}:${i}:weight`, value: shown(p, full(setUnit, item).weight), step: unitsOf(p) === "lb" ? 5 : 2.5, min: 0, max: 1500, label: t("Weight"), unit: unitsOf(p), small: true})] : [])];
      return `<div class="fit-set ${set.done ? "is-done" : ""}"><span class="fit-set-num">${i + 1}</span>${fields.join("")}<button type="button" class="fit-set-check" data-set-done="${u}:${i}" aria-pressed="${set.done}" aria-label="${e(set.done ? t("Set {n} done; undo", {n: i + 1}) : t("Mark set {n} done", {n: i + 1}))}">${icons.check}</button></div>`;
    }).join("");
    return `<li class="fit-su ${u === current ? "is-current" : ""} ${finished ? "is-done" : ""}"><header><span class="fit-unit-num">${finished ? icons.check : u + 1}</span><div><strong>${e(exName(item))}</strong><small>${e(unitTarget(p, unit))}</small></div><span class="fit-su-count">${sets.filter(set => set.done).length}/${sets.length}</span></header><div class="fit-sets">${rows}</div>${item.mode === "cardio" ? "" : `<button type="button" class="text-button fit-set-add" data-set-add="${u}">${icons.plus}<span>${t("Add a set")}</span></button>`}</li>`;
  }).join("");
  return `<section class="card fit-session" id="fit-session"><header class="fit-session-head"><div><p class="eyebrow"><span class="live-dot"></span> ${t("WORKOUT IN PROGRESS")}</p><h2>${e(a.name)}</h2></div><div class="fit-session-clock"><time data-since="${a.startedAt}">0:00</time><small>${t("{done} of {total} sets", {done, total})}</small></div><div class="button-row"><button type="button" class="button primary" data-fit-finish>${icons.check}<span>${t("Finish workout")}</span></button><button type="button" class="button ghost danger" data-fit-discard>${t("Discard")}</button></div></header>
    <div class="fit-session-bar" role="progressbar" aria-label="${t("Sets done")}" aria-valuemin="0" aria-valuemax="${total}" aria-valuenow="${done}"><i style="--p:${total ? (done / total).toFixed(3) : 0}"></i></div>${rest}<ol class="fit-session-units">${units}</ol></section>`;
}
function newActive(p: PluginItem, name: string, units: Unit[], task: string | null): Active {
  return {
    name, task, startedAt: Date.now(), restUntil: null, restSpan: 0,
    units: units.map(unit => ({...unit})),
    sets: units.map(unit => {
      const item = exerciseOf(p, unit.exercise);
      const v = full(unit, item);
      const set: SetLog = item.mode === "cardio" ? {minutes: v.minutes, distance: v.distance, done: false} : item.mode === "time" ? {seconds: v.seconds, done: false} : item.mode === "weight" ? {reps: v.reps, weight: v.weight, done: false} : {reps: v.reps, done: false};
      return Array.from({length: item.mode === "cardio" ? 1 : v.sets}, () => ({...set}));
    })
  };
}
/** What a finished session logs: every set done, and what they add up to. */
function summarize(p: PluginItem, a: Active, end = Date.now()) {
  const body = bodyWeight(p);
  const units = a.units.map((unit, u) => ({exercise: unit.exercise, mode: exerciseOf(p, unit.exercise).mode, sets: (a.sets[u] ?? []).filter(set => set.done)})).filter(unit => unit.sets.length);
  const minutes = Math.max(1, Math.round((end - a.startedAt) / 60000));
  let volume = 0, sets = 0, workSeconds = 0;
  const met: number[] = [];
  for (const unit of units) {
    const item = exerciseOf(p, unit.exercise);
    for (const set of unit.sets) {
      if (item.mode === "weight") volume += (set.reps ?? 0) * (set.weight ?? 0);
      if (item.mode !== "cardio") sets += 1;
      workSeconds += item.mode === "cardio" ? (set.minutes ?? 0) * 60 : item.mode === "time" ? set.seconds ?? 0 : (set.reps ?? 0) * 3.5;
      met.push(item.met);
    }
  }
  const averageMet = met.length ? met.reduce((sum, value) => sum + value, 0) / met.length : 4;
  return {name: a.name, startedAt: localStamp(new Date(a.startedAt)), minutes, sets, volume: Math.round(volume), calories: Math.round(averageMet * body * Math.max(minutes * 60 * 0.6, workSeconds) / 3600), units};
}

/** Start a session, after asking when one is already going; then show it on the plugin's page. */
function start(p: PluginItem, host: PluginHost, name: string, units: Unit[], task: string | null) {
  if (!units.length) return;
  if (activeOf(p) && !window.confirm(t("A workout is already in progress. Replace it with “{name}”?", {name}))) return;
  void host.action(async () => {
    await saveState(host, p, {active: newActive(p, name, units, task)}, {now: true});
    viewOf(p).tab = "train";
    host.navigate(`plugin:${p.id}`);
    host.notify(t("“{name}” has started. Check off each set as you go.", {name}));
  });
}
async function finish(p: PluginItem, host: PluginHost) {
  const a = activeOf(p);
  if (!a) return;
  const summary = summarize(p, a);
  if (!summary.units.length) { host.notify(t("Check off at least one set first, or discard the workout.")); return; }
  await host.action(async () => {
    await flushState(p.id);
    const task = a.task ? host.snapshot().tasks.find(item => item.description === a.task) : undefined;
    let undo: () => Promise<unknown>;
    if (task) {
      const result = await host.request<{undo: Record<string, unknown>}>("task_delete", {description: task.description, action: "deleted", pluginLog: {kind: "session", data: summary}});
      undo = () => host.request("task_restore", result.undo);
    } else {
      const result = await host.request<{undo: Record<string, unknown>}>("plugin_log", {id: p.id, kind: "session", data: summary, asTask: true, description: a.name, priority: 5, at: localStamp(), startedAt: summary.startedAt});
      undo = () => host.request("plugin_log_delete", result.undo);
    }
    await host.request("plugin_state", {id: p.id, set: {active: null}});
    await host.reload();
    host.offerUndo(t("Logged “{name}”: {minutes} min, {sets} sets. Well done.", {name: a.name, minutes: summary.minutes, sets: summary.sets}), async () => {
      await undo();
      await host.reload();
      host.notify(t("“{name}” is off the log.", {name: a.name}));
    });
  });
}

// ----- Binding -----------------------------------------------------------------------------------------------

function defaultDue(): string {
  const due = new Date();
  due.setHours(due.getHours() + 2, 0, 0, 0);
  if (isoDay(due) !== isoDay(new Date())) { due.setTime(Date.now()); due.setHours(23, 59, 0, 0); }
  return `${isoDay(due)}T${String(due.getHours()).padStart(2, "0")}:${String(due.getMinutes()).padStart(2, "0")}`;
}
const freeName = (s: Snapshot, base: string) => { let name = base, n = 2; while (s.tasks.some(task => task.description === name)) name = `${base} (${n++})`; return name; };

async function plan(p: PluginItem, host: PluginHost, name: string, units: Unit[], due: string) {
  await host.action(async () => {
    if (!p.type) throw new Error(t("This plugin has no task type yet. Open Settings and save the plugin again."));
    const description = freeName(host.snapshot(), name);
    await host.request("task_add", {description, priority: 6, type: p.type, due, postpone: 0, subtasks: units.map(unit => ({text: unitLabel(p, unit).slice(0, 200), done: false})), plugin: {workout: {name, units}}});
    await host.reload();
    host.notify(due ? t("Planned “{name}”, timed for {time}. It’s on the board.", {name: description, time: formatDate(due, {weekday: "short", hour: "numeric", minute: "2-digit"})}) : t("Planned “{name}”. It’s on the board.", {name: description}));
  });
}

function bindSession(scope: HTMLElement, p: PluginItem, host: PluginHost) {
  const panel = scope.querySelector<HTMLElement>("#fit-session");
  const a = activeOf(p);
  if (!panel || !a) return;
  const keep = () => void saveState(host, p, {active: a});
  bindSteppers(panel, (field, value) => {
    const [, u, i, key] = field.split(":");
    const set = a.sets[Number(u)]?.[Number(i)];
    if (!set) return;
    (set as Record<string, number | boolean>)[key] = key === "weight" ? fromShown(p, value) : value;
    keep();
  });
  panel.querySelectorAll<HTMLButtonElement>("[data-set-done]").forEach(btn => btn.onclick = () => {
    const [u, i] = btn.dataset.setDone!.split(":").map(Number);
    const set = a.sets[u]?.[i];
    if (!set) return;
    set.done = !set.done;
    const rest = full(a.units[u], exerciseOf(p, a.units[u].exercise)).rest;
    const left = a.sets.some(sets => sets.some(entry => !entry.done));
    if (set.done && rest > 0 && left) { a.restUntil = Date.now() + rest * 1000; a.restSpan = rest * 1000; }
    else if (!set.done) a.restUntil = null;
    keep();
    host.redraw();
    if (set.done && !left) host.notify(t("Every set is done. Finish the workout to log it."));
  });
  panel.querySelectorAll<HTMLButtonElement>("[data-set-add]").forEach(btn => btn.onclick = () => {
    const sets = a.sets[Number(btn.dataset.setAdd)];
    if (!sets) return;
    const last = sets[sets.length - 1];
    sets.push({...(last ?? {}), done: false});
    keep();
    host.redraw();
  });
  const skip = panel.querySelector<HTMLButtonElement>("[data-rest-skip]");
  if (skip) skip.onclick = () => { a.restUntil = null; keep(); host.redraw(); };
  const more = panel.querySelector<HTMLButtonElement>("[data-rest-add]");
  if (more) more.onclick = () => { a.restUntil = (a.restUntil ?? Date.now()) + 15000; a.restSpan += 15000; keep(); host.redraw(); };
  panel.addEventListener("plugin-clock-done", () => { chime(); window.setTimeout(() => { if (a.restUntil && a.restUntil <= Date.now()) { a.restUntil = null; keep(); host.redraw(); } }, 1200); });
  panel.querySelector<HTMLButtonElement>("[data-fit-finish]")!.onclick = () => void finish(p, host);
  panel.querySelector<HTMLButtonElement>("[data-fit-discard]")!.onclick = () => {
    if (!window.confirm(t("Discard “{name}”? Nothing is logged.", {name: a.name}))) return;
    void host.action(async () => {
      await saveState(host, p, {active: null}, {now: true});
      host.redraw();
      host.notify(t("Discarded “{name}”.", {name: a.name}));
    });
  };
}

function bindBuilder(scope: HTMLElement, p: PluginItem, host: PluginHost, v: View) {
  const card = scope.querySelector<HTMLElement>(".fit-builder");
  if (!card) return;
  const summary = () => { const box = card.querySelector(".fit-summary"); if (box) box.innerHTML = builderSummary(p, v.draft.units); };
  const changed = (redraw = true) => { saveDraft(p, v); if (redraw) host.redraw(); else summary(); };
  card.querySelector<HTMLInputElement>('[name="fitDraftName"]')!.oninput = event => { v.draft.name = (event.target as HTMLInputElement).value; saveDraft(p, v); };
  bindSteppers(card, (field, value) => {
    const [, i, key] = field.split(":");
    const unit = v.draft.units[Number(i)];
    if (!unit) return;
    (unit as Record<string, number | string>)[key] = key === "weight" ? fromShown(p, value) : value;
    changed(false);
  });
  card.querySelectorAll<HTMLButtonElement>("[data-unit-remove]").forEach(btn => btn.onclick = () => { v.draft.units.splice(Number(btn.dataset.unitRemove), 1); changed(); });
  card.querySelectorAll<HTMLButtonElement>("[data-unit-move]").forEach(btn => btn.onclick = () => {
    const from = Number(btn.dataset.unitMove), to = from + Number(btn.dataset.by);
    if (to < 0 || to >= v.draft.units.length) return;
    const [unit] = v.draft.units.splice(from, 1);
    v.draft.units.splice(to, 0, unit);
    changed();
    card.ownerDocument.querySelector<HTMLElement>(`[data-unit-move="${to}"][data-by="${btn.dataset.by}"]`)?.focus();
  });
  card.querySelector<HTMLSelectElement>('[name="fitAdd"]')!.onchange = event => {
    const id = (event.target as HTMLSelectElement).value;
    if (!id) return;
    v.draft.units.push(unitFor(exerciseOf(p, id)));
    changed();
  };
  const clear = card.querySelector<HTMLButtonElement>("[data-fit-clear]");
  if (clear) clear.onclick = () => { v.draft = {name: "", units: []}; changed(); };
  const timed = card.querySelector<HTMLInputElement>('[name="fitTimed"]')!;
  const due = card.querySelector<HTMLInputElement>('[name="fitDue"]')!;
  timed.onchange = () => { v.timed = timed.checked; due.disabled = !timed.checked; card.querySelector(".timed-field")?.classList.toggle("is-on", timed.checked); if (timed.checked) due.focus(); };
  due.onchange = () => { v.due = due.value; };
  const name = () => v.draft.name.trim() || t("Workout");
  card.querySelector<HTMLButtonElement>("[data-fit-start-draft]")!.onclick = () => start(p, host, name(), v.draft.units, null);
  card.querySelector<HTMLButtonElement>("[data-fit-plan]")!.onclick = () => {
    if (v.timed && !due.value) { due.focus(); return; }
    void plan(p, host, name(), v.draft.units, v.timed ? due.value : "");
  };
  card.querySelector<HTMLButtonElement>("[data-fit-log-draft]")!.onclick = () => void host.action(async () => {
    const stats = workoutStats(p, v.draft.units);
    const fake = newActive(p, name(), v.draft.units, null);
    fake.sets.forEach(sets => sets.forEach(set => { set.done = true; }));
    fake.startedAt = Date.now() - stats.minutes * 60000;
    const summary = summarize(p, fake);
    const result = await host.request<{undo: Record<string, unknown>}>("plugin_log", {id: p.id, kind: "session", data: summary, asTask: true, description: name(), priority: 5, at: localStamp(), startedAt: summary.startedAt});
    await host.reload();
    host.offerUndo(t("Logged “{name}”: {minutes} min, {sets} sets. Well done.", {name: name(), minutes: summary.minutes, sets: summary.sets}), async () => {
      await host.request("plugin_log_delete", result.undo);
      await host.reload();
      host.notify(t("“{name}” is off the log.", {name: name()}));
    });
  });
  card.querySelector<HTMLButtonElement>("[data-fit-save-routine]")!.onclick = () => void host.action(async () => {
    const list = routinesOf(p);
    const existing = list.findIndex(routine => routine.name === name());
    const routine: Routine = {id: existing >= 0 ? list[existing].id : `r${Date.now().toString(36)}`, name: name(), units: v.draft.units.map(unit => ({...unit}))};
    const next = existing >= 0 ? list.map((item, i) => i === existing ? routine : item) : [...list, routine];
    await saveState(host, p, {routines: next}, {now: true});
    host.redraw();
    host.notify(existing >= 0 ? t("Updated the routine “{name}”.", {name: routine.name}) : t("Saved the routine “{name}”.", {name: routine.name}));
  });
}

function bindPage(scope: HTMLElement, p: PluginItem, host: PluginHost) {
  const v = viewOf(p);
  const redraw = () => host.redraw();
  bindSession(scope, p, host);
  scope.querySelectorAll<HTMLButtonElement>("[data-fit-tab]").forEach(btn => btn.onclick = () => { v.tab = btn.dataset.fitTab as Tab; remember(`${p.id}-tab`, v.tab); redraw(); scope.querySelector<HTMLElement>(`[data-fit-tab="${v.tab}"]`)?.focus(); });
  const pickMuscle = (muscle: Muscle | null) => { v.muscle = muscle; if (muscle) v.tab = "exercises"; redraw(); };
  scope.querySelectorAll<SVGGElement | HTMLElement>("[data-muscle]").forEach(el => {
    const muscle = el.getAttribute("data-muscle") as Muscle;
    const toggle = () => pickMuscle(v.muscle === muscle ? null : muscle);
    el.addEventListener("click", toggle);
    el.addEventListener("keydown", event => { const key = (event as KeyboardEvent).key; if (key === "Enter" || key === " ") { event.preventDefault(); toggle(); } });
  });
  bindBuilder(scope, p, host, v);
  // Routines
  const routine = (id: string | undefined) => routinesOf(p).find(item => item.id === id);
  scope.querySelectorAll<HTMLButtonElement>("[data-routine-start]").forEach(btn => btn.onclick = () => { const item = routine(btn.dataset.routineStart); if (item) start(p, host, item.name, item.units, null); });
  scope.querySelectorAll<HTMLButtonElement>("[data-routine-plan]").forEach(btn => btn.onclick = () => { const item = routine(btn.dataset.routinePlan); if (item) void plan(p, host, item.name, item.units, v.timed ? v.due || defaultDue() : ""); });
  scope.querySelectorAll<HTMLButtonElement>("[data-routine-load]").forEach(btn => btn.onclick = () => {
    const item = routine(btn.dataset.routineLoad);
    if (!item) return;
    v.draft = {name: item.name, units: item.units.map(unit => ({...unit}))};
    saveDraft(p, v);
    redraw();
    scope.ownerDocument.querySelector<HTMLElement>(".fit-builder")?.scrollIntoView?.({block: "start", behavior: "smooth"});
  });
  scope.querySelectorAll<HTMLButtonElement>("[data-routine-delete]").forEach(btn => btn.onclick = () => {
    const list = routinesOf(p);
    const item = routine(btn.dataset.routineDelete);
    if (!item) return;
    void host.action(async () => {
      await saveState(host, p, {routines: list.filter(entry => entry !== item)}, {now: true});
      host.redraw();
      host.offerUndo(t("Deleted the routine “{name}”.", {name: item.name}), async () => {
        await saveState(host, p, {routines: list}, {now: true});
        host.redraw();
        host.notify(t("“{name}” is back.", {name: item.name}));
      });
    });
  });
  // Library
  scope.querySelectorAll<HTMLButtonElement>("[data-muscle-filter]").forEach(btn => btn.onclick = () => { v.muscle = isMuscle(btn.dataset.muscleFilter) ? btn.dataset.muscleFilter : null; redraw(); });
  scope.querySelectorAll<HTMLButtonElement>("[data-fit-mode]").forEach(btn => btn.onclick = () => { v.mode = btn.dataset.fitMode as Mode | "all"; redraw(); });
  const search = scope.querySelector<HTMLInputElement>('[name="fitSearch"]');
  if (search) search.oninput = () => {
    v.query = search.value;
    const grid = scope.querySelector<HTMLElement>("#fit-exercises");
    if (!grid) return;
    const fresh = document.createElement("div");
    fresh.innerHTML = libraryTab(p, v);
    grid.innerHTML = fresh.querySelector("#fit-exercises")?.innerHTML ?? "";
    bindLibraryCards(scope, p, host, v);
  };
  bindLibraryCards(scope, p, host, v);
  const open = scope.querySelector<HTMLButtonElement>("[data-ex-new]");
  if (open) open.onclick = () => { v.adding = true; redraw(); scope.ownerDocument.querySelector<HTMLInputElement>('[name="exName"]')?.focus(); };
  const form = scope.querySelector<HTMLFormElement>(".fit-new-exercise");
  if (form) {
    form.querySelector<HTMLButtonElement>("[data-ex-cancel]")!.onclick = () => { v.adding = false; redraw(); };
    form.onsubmit = event => {
      event.preventDefault();
      const data = new FormData(form);
      const name = String(data.get("exName") ?? "").trim();
      const muscles = data.getAll("exMuscle").filter(isMuscle);
      if (!name) return;
      const mode = String(data.get("exMode")) as Mode;
      const item: Exercise = {id: `c${Date.now().toString(36)}`, name, mode: mode in MODES ? mode : "reps", muscles: muscles.slice(0, 2), secondary: muscles.slice(2), met: mode === "cardio" ? 7 : 4, defaults: {}};
      void host.action(async () => {
        await saveState(host, p, {exercises: [...(Array.isArray(p.state.exercises) ? p.state.exercises : []), item]}, {now: true});
        v.adding = false;
        host.redraw();
        host.notify(t("Added “{name}” to your exercises.", {name}));
      });
    };
  }
  // Progress
  scope.querySelectorAll<HTMLButtonElement>("[data-fit-units]").forEach(btn => btn.onclick = () => { if (unitsOf(p) !== btn.dataset.fitUnits) { void saveState(host, p, {units: btn.dataset.fitUnits}); redraw(); } });
  const prefs = scope.querySelector<HTMLElement>(".fit-prefs");
  if (prefs) bindSteppers(prefs, (field, value) => { if (field === "goal") { void saveState(host, p, {goal: value}); } });
  const body = scope.querySelector<HTMLElement>(".fit-body");
  if (body) {
    bindSteppers(body, (field, value) => { if (field === "bodyGoal") void saveState(host, p, {bodyGoal: value ? round(fromShown(p, value), 2) : null}); });
    body.querySelector<HTMLFormElement>(".fit-body-form")!.onsubmit = event => {
      event.preventDefault();
      const weight = Number(body.querySelector<HTMLInputElement>('[data-field="bodyWeight"]')?.value);
      const fat = Number(body.querySelector<HTMLInputElement>('[name="bodyFat"]')?.value);
      if (!(weight > 0)) return;
      void host.action(async () => {
        const data: Record<string, number> = {weight: round(fromShown(p, weight), 2)};
        if (fat > 0) data.fat = round(fat, 1);
        const result = await host.request<{undo: Record<string, unknown>}>("plugin_log", {id: p.id, kind: "body", data, at: localStamp()});
        await host.reload();
        host.offerUndo(t("Logged {weight}.", {weight: `${num(weight)} ${unitsOf(p)}`}), async () => { await host.request("plugin_log_delete", result.undo); await host.reload(); });
      });
    };
  }
  scope.querySelectorAll<HTMLButtonElement>("[data-session-delete]").forEach(btn => btn.onclick = () => {
    const session = sessions(p).find(item => item.entry.id === btn.dataset.sessionDelete);
    if (!session || !window.confirm(t("Delete “{name}” from your workout log?", {name: session.name}))) return;
    void host.action(async () => {
      await host.request("plugin_log_delete", {id: p.id, entry: session.entry.id});
      await host.reload();
      host.notify(t("“{name}” is off the log.", {name: session.name}));
    });
  });
  bindTaskExtras(scope, p, host);
}
function bindLibraryCards(scope: HTMLElement, p: PluginItem, host: PluginHost, v: View) {
  scope.querySelectorAll<HTMLButtonElement>("[data-ex-add]").forEach(btn => btn.onclick = () => {
    const item = exerciseOf(p, btn.dataset.exAdd!);
    v.draft.units.push(unitFor(item));
    saveDraft(p, v);
    host.redraw();
    host.notify(t("Added {exercise} to your workout ({count} exercises). Adjust it under Train.", {exercise: exName(item), count: v.draft.units.length}));
  });
  scope.querySelectorAll<HTMLButtonElement>("[data-ex-delete]").forEach(btn => btn.onclick = () => {
    const list: Exercise[] = Array.isArray(p.state.exercises) ? p.state.exercises : [];
    const item = list.find(entry => entry.id === btn.dataset.exDelete);
    if (!item) return;
    void host.action(async () => {
      await saveState(host, p, {exercises: list.filter(entry => entry !== item)}, {now: true});
      host.redraw();
      host.offerUndo(t("Deleted “{name}”.", {name: item.name}), async () => { await saveState(host, p, {exercises: list}, {now: true}); host.redraw(); });
    });
  });
}
const workoutOf = (task: Pick<TaskItem, "plugin">): {name: string; units: Unit[]} | null => {
  const workout = (task.plugin as {workout?: {name?: unknown; units?: unknown}} | null | undefined)?.workout;
  return workout && Array.isArray(workout.units) ? {name: String(workout.name ?? ""), units: workout.units as Unit[]} : null;
};
function bindTaskExtras(scope: HTMLElement, p: PluginItem, host: PluginHost) {
  scope.querySelectorAll<HTMLButtonElement>("[data-fit-start-task]").forEach(btn => btn.onclick = () => {
    const task = host.snapshot().tasks[Number(btn.dataset.fitStartTask)];
    const workout = task && workoutOf(task);
    if (workout) start(p, host, task.description, workout.units, task.description);
  });
  scope.querySelectorAll<HTMLButtonElement>("[data-plugin-session]").forEach(btn => btn.onclick = () => host.navigate(`plugin:${p.id}`));
}

// ----- The module --------------------------------------------------------------------------------------------

/** What a new fitness plugin starts with: kilograms, three workouts a week, and five routines to begin from. */
export function starterState(): Record<string, unknown> {
  const u = (exercise: string, values: Defaults = {}): Unit => ({...unitFor(LIBRARY.find(item => item.id === exercise)!), ...values});
  return {
    units: "kg", goal: 3,
    routines: [
      {id: "full", name: t("Full body"), units: [u("squat", {sets: 3, reps: 8, weight: 50}), u("bench"), u("row"), u("plank")]},
      {id: "push", name: t("Push"), units: [u("bench", {sets: 4}), u("ohp"), u("incline"), u("lateral"), u("triext")]},
      {id: "pull", name: t("Pull"), units: [u("deadlift"), u("pullup"), u("row"), u("curl")]},
      {id: "legs", name: t("Legs"), units: [u("squat"), u("rdl"), u("lunge"), u("calf")]},
      {id: "core", name: t("Core & cardio"), units: [u("run", {minutes: 20, distance: 3}), u("plank"), u("twist"), u("crunch")]}
    ]
  };
}

export const fitness: PluginModule = {
  kind: "fitness",
  page(p, host) {
    const s = host.snapshot();
    const v = viewOf(p);
    const a = activeOf(p);
    const body = v.tab === "exercises" ? libraryTab(p, v) : v.tab === "progress" ? progressTab(p, s.today.date) : `<div class="fit-train">${builder(p, v, s)}<div class="fit-side">${routinesCard(p)}${plannedCard(p, host)}</div></div>`;
    return `${a ? sessionPanel(p, a, Date.now()) : ""}${hero(p, s.today.date)}${tabs(v)}<div class="fit-tab-body" role="tabpanel">${body}</div>`;
  },
  bindPage,
  activity(p, now) {
    const today = isoDay(new Date(now));
    const weeks = weekly(p, today, 12);
    const labels = weeks.map(week => formatDate(week.week, {month: "numeric", day: "numeric"}));
    const goal = goalOf(p);
    const current = weeks[weeks.length - 1];
    const recordsList = records(p).slice(0, 4);
    const body = p.log.filter(entry => entry.kind === "body" && Number(entry.data?.weight) > 0);
    const chart = line({points: body.slice(-60).map(entry => ({at: entry.at, value: bodyShown(p, Number(entry.data.weight))})), target: Number(p.state.bodyGoal) ? bodyShown(p, Number(p.state.bodyGoal)) : null, label: t("Body weight over time"), format: value => `${num(value)} ${unitsOf(p)}`});
    return `<section class="plugin-activity fit-activity">
      <article class="card fit-week"><p class="eyebrow">${icons.flame} ${t("THIS WEEK")}</p><div class="fit-week-main">${ring({value: current.sessions / goal, size: 112, label: t("{done} of {goal} workouts this week", {done: current.sessions, goal}), center: `${current.sessions}<small>/${goal}</small>`, tone: "sw-orange"})}<div class="pk-tiles">${tile(t("Minutes"), num(current.minutes, 0))}${tile(t("Volume"), `${compact(shown(p, current.volume))}<small>${unitsOf(p)}</small>`)}${tile(t("Workouts"), String(sessions(p).length), t("all time"))}${tile(t("Streak"), String(streak(p, today)), t("weeks at goal"))}</div></div></article>
      <article class="card fit-figure-card">${bodyFigure({load: muscleLoad(p, today), interactive: false, label: t("Muscles worked this week")})}${heatLegend()}</article>
      <article class="card fit-chart"><h3>${t("Workouts per week")}</h3>${bars({values: weeks.map(week => week.sessions), labels, tips: weeks.map((week, i) => t("Week of {date} · {count} workouts", {date: labels[i], count: week.sessions})), goal, label: t("Workouts per week"), tone: "sw-orange"})}</article>
      <article class="card fit-chart"><h3>${t("Volume per week")}</h3>${bars({values: weeks.map(week => shown(p, week.volume)), labels, tips: weeks.map((week, i) => `${labels[i]} · ${compact(shown(p, week.volume))} ${unitsOf(p)}`), label: t("Volume per week"), tone: "sw-violet"})}</article>
      <article class="card fit-records"><h3>${icons.trophy} ${t("Personal records")}</h3>${recordsList.length ? `<ol class="fit-record-list">${recordsList.map((record, i) => `<li style="--i:${i}"><span class="fit-medal">${icons.trophy}</span><div><strong>${e(exName(record.exercise))}</strong><small>${e(formatDate(record.at, {month: "short", day: "numeric"}))}</small></div><b>${e(record.best)}</b></li>`).join("")}</ol>` : empty(t("Finish a workout to set your first records."))}${chart ? `<h3>${icons.scale} ${t("Body weight")}</h3>${chart}` : ""}</article>
      <article class="card fit-history">${recentSessions(p, 6, false)}</article>
    </section>`;
  },
  timer(p, s, now) {
    const a = activeOf(p);
    const icon = pluginIcon(p);
    const head = (flag = "") => `<div class="timer-top"><span class="timer-dial plugin-dial" aria-hidden="true"><span class="dial-icon">${icon}</span></span><div class="timer-heading"><div class="timer-head"><span class="timer-kind">${icon}<span>${e(p.name)}</span></span>${flag}</div>`;
    if (a) {
      const total = a.sets.reduce((sum, sets) => sum + sets.length, 0);
      const done = a.sets.reduce((sum, sets) => sum + sets.filter(set => set.done).length, 0);
      const resting = a.restUntil && a.restUntil > now;
      return `<article class="timer timer-plugin plugin-fitness is-live" role="listitem">${head(`<span class="timer-flag">${icons.zap}<span>${t("Live")}</span></span>`)}<h4 title="${e(a.name)}">${e(a.name)}</h4></div></div>
        <p class="timer-clock"><time data-since="${a.startedAt}">0:00</time><span class="timer-sub">${t("{done} of {total} sets", {done, total})}${resting ? ` · ${t("rest")} <b data-until="${a.restUntil}" data-span="${a.restSpan}">0:00</b>` : ""}</span></p>
        <div class="timer-foot"><span class="timer-progress-mini"><i style="--p:${total ? (done / total).toFixed(3) : 0}"></i></span><button type="button" class="timer-complete" data-plugin-session="${e(p.id)}">${icons.arrow}<span>${t("Open")}</span></button></div></article>`;
    }
    const today = isoDay(new Date(now));
    const index = s.tasks.findIndex(task => task.type === p.type && !!workoutOf(task) && (!task.due || task.due.slice(0, 10) <= today));
    const goal = goalOf(p);
    const week = weekly(p, today, 1)[0];
    if (index >= 0) {
      const task = s.tasks[index];
      const stats = workoutStats(p, workoutOf(task)!.units);
      return `<article class="timer timer-plugin plugin-fitness" role="listitem">${head()}<h4 title="${e(task.description)}">${e(task.description)}</h4></div></div><p class="timer-clock plugin-clock-text"><strong>~${stats.minutes} ${t("min")}</strong><span class="timer-sub">${tn("{count} exercise", "{count} exercises", workoutOf(task)!.units.length)} · ${t("{done} of {goal} this week", {done: week.sessions, goal})}</span></p><div class="timer-foot"><button type="button" class="timer-complete" data-fit-start-task="${index}">${icons.zap}<span>${t("Start workout")}</span></button></div></article>`;
    }
    return `<article class="timer timer-plugin plugin-fitness" role="listitem">${head()}<h4>${t("{done} of {goal} workouts this week", {done: week.sessions, goal})}</h4></div></div><p class="timer-clock plugin-clock-text"><span class="timer-progress-mini wide"><i style="--p:${Math.min(1, week.sessions / goal).toFixed(3)}"></i></span><span class="timer-sub">${week.sessions >= goal ? t("Goal reached this week.") : t("Nothing planned for today.")}</span></p><div class="timer-foot"><button type="button" class="timer-complete" data-plugin-session="${e(p.id)}">${icons.plus}<span>${t("Plan a workout")}</span></button></div></article>`;
  },
  bindTimer(scope, p, host) {
    bindTaskExtras(scope, p, host);
    // A rest that ends while the session's page is not open still chimes, from its card in the timer row.
    if (scope.dataset.fitChime) return;
    scope.dataset.fitChime = "1";
    scope.addEventListener("plugin-clock-done", event => { if ((event.target as Element).closest(".plugin-fitness")) chime(); });
  },
  taskExtra(task, index, p) {
    const workout = workoutOf(task);
    if (!workout) return "";
    const stats = workoutStats(p, workout.units);
    const live = activeOf(p)?.task === task.description;
    return `<div class="plugin-task-extra fit-task-extra"><span>${pluginIcon(p)}${tn("{count} exercise", "{count} exercises", workout.units.length)} · ~${stats.minutes} ${t("min")}</span>${live ? `<button type="button" class="button small" data-plugin-session="${e(p.id)}">${icons.zap}<span>${t("In progress")}</span></button>` : `<button type="button" class="button small primary" data-fit-start-task="${index}">${icons.zap}<span>${t("Start workout")}</span></button>`}</div>`;
  },
  bindTaskExtras,
  subtasksFor(data, p) {
    const workout = workoutOf({plugin: data});
    return workout ? workout.units.map(unit => ({text: unitLabel(p, unit).slice(0, 200), done: false})) : [];
  },
  describeData(data, p) {
    const workout = workoutOf({plugin: data});
    if (!workout) return "";
    const stats = workoutStats(p, workout.units);
    return `${tn("{count} exercise", "{count} exercises", workout.units.length)} · ~${stats.minutes} ${t("min")} · ${stats.muscles.slice(0, 3).map(muscle => t(MUSCLES[muscle])).join(", ")}`;
  }
};
