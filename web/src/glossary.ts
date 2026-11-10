// Glossary page: every number the workspace shows, explained. Each metric is a card with a small animated picture
// of it, the way it is counted (a formula of colored terms), an example, your own value when it is known, and
// links to the places that show it. The pictures play once a card scrolls into view. Rendering is pure (state in,
// HTML out); bindGlossary adds the search, the section jumps, the workload try-it sliders, and the scroll-in
// animations, and hands every "Find it on" link to render.ts, which owns navigation and loading.
import { STATUS_ICON, STATUS_LABEL } from "./activity";
import { delta, pctText, ring, seeded } from "./charts";
import { openProgress } from "./daily";
import { MUSCLES, type Muscle } from "./fitness";
import { msg, t, tn } from "./i18n";
import { progressOf, todayProgress } from "./timers";
import type { DailyOverview, PluginKind, Snapshot, WeeklyOverview } from "./types";
import { e, formatHours, icons, sw, typeSwatch, type Swatch } from "./ui";
import { levelInfo, levelRanges, loadLevels, loadScale, workload } from "./workload";

/** What the glossary draws from: the snapshot, and the daily and weekly overviews once they are loaded. */
export type GlossaryContext = {snapshot: Snapshot; daily: DailyOverview | null; week: WeeklyOverview | null; weighted: boolean; now: number};
export type SectionId = "basics" | "day" | "week" | "history" | "plugins";
/** Where a metric shows: the task board, the daily overview, the weekly overview, the Activity history, Settings,
 *  or a plugin. */
type Spot = "board" | "day" | "week" | "activity" | "settings" | "plugins";
/** One piece of a formula: a value (a colored pill), a function word, or an operator (drawn as it is). */
type Part = {v: string} | {fn: string} | {op: string};
type Term = {
  id: string;
  section: SectionId;
  /** Name, gist, and note are message keys. */
  name: string;
  gist: string;
  note?: string;
  icon: string;
  tone: Swatch;
  formula?: Part[];
  where: Spot[];
  /** Plugin kinds whose pages show it. */
  kinds?: PluginKind[];
  wide?: boolean;
  art: (c: GlossaryContext) => string;
  yours?: (c: GlossaryContext) => string | null;
  extra?: (c: GlossaryContext) => string;
};

const v = (key: string): Part => ({v: key});
const fn = (key: string): Part => ({fn: key});
const op = (symbol: string): Part => ({op: symbol});

const SECTIONS: {id: SectionId; name: string; blurb: string; icon: string; tone: Swatch}[] = [
  {id: "basics", name: msg("Building blocks"), blurb: msg("The few things every number here is made of."), icon: icons.blocks, tone: "violet"},
  {id: "day", name: msg("Your day"), blurb: msg("The daily overview at the top of the Overview page."), icon: icons.today, tone: "aqua"},
  {id: "week", name: msg("Your week"), blurb: msg("The weekly overview, opened from the Activity page."), icon: icons.week, tone: "blue"},
  {id: "history", name: msg("Your history"), blurb: msg("The Activity page: your rhythm and every task you closed."), icon: icons.history, tone: "magenta"},
  {id: "plugins", name: msg("Plugins"), blurb: msg("Fitness and trackers, on their own pages and in Activity by category."), icon: icons.puzzle, tone: "orange"}
];

const SPOTS: Record<Exclude<Spot, "plugins">, string> = {board: msg("Task board"), day: msg("Daily overview"), week: msg("Weekly overview"), activity: msg("Activity"), settings: msg("Settings")};
const OUTCOME_TONE = {completed: "sw-aqua", cancelled: "sw-yellow", dropped: "sw-magenta"} as const;

// ----- Pictures -----------------------------------------------------------------------------------------------
// Small, made-up examples of each metric. Every animated element gets --i (its place) for a staggered entrance.

function bars(values: number[], options: {line?: number; label?: string; now?: boolean; thin?: boolean} = {}): string {
  const max = Math.max(1, ...values, options.line ?? 0);
  const last = values.length - 1;
  const line = options.line === undefined ? "" : `<span class="ga-line" style="--h:${(options.line / max).toFixed(3)}">${options.label ? `<em>${e(options.label)}</em>` : ""}</span>`;
  return `<div class="ga-bars ${options.thin ? "thin" : ""}">${values.map((value, i) => `<i class="${options.now && i === last ? "is-now" : ""}" style="--h:${(value / max).toFixed(3)}; --i:${i}"></i>`).join("")}${line}</div>`;
}
const dots = (on: number) => `<div class="ga-dots">${Array.from({length: 10}, (_, i) => `<i class="${i < on ? "on" : ""}" style="--i:${i}"></i>`).join("")}</div>`;
const big = (value: string, small = "") => `<strong class="ga-big">${value}${small ? `<small>${small}</small>` : ""}</strong>`;

function journey(): string {
  return `<div class="ga-journey"><i class="ga-stop"></i><b class="ga-run"></b><i class="ga-stop is-end"></i><span>${t("Added")}</span><em>${icons.hourglass}${formatHours(30)}</em><span>${t("Completed")}</span></div>`;
}

function gauge(c: GlossaryContext): string {
  const load = workload(c.snapshot.tasks, c.snapshot.today.date);
  const info = levelInfo(load.level);
  const fraction = Math.min(load.points, loadScale()) / loadScale();
  return `<div class="ga-gauge load-${load.level}"><svg viewBox="0 0 120 70"><path class="ga-gauge-track" d="M10 62 A50 50 0 0 1 110 62"/><path class="ga-gauge-fill" d="M10 62 A50 50 0 0 1 110 62" pathLength="100" stroke-dasharray="${(fraction * 100).toFixed(2)} 100"/><g class="ga-needle" style="--a:${(fraction * 180 - 90).toFixed(1)}deg"><line x1="60" y1="62" x2="60" y2="20"/><circle cx="60" cy="62" r="5"/></g></svg><span class="ga-gauge-read">${info.icon}<b>${load.points.toFixed(1)}</b>${e(info.label)}</span></div>`;
}

function heat(): string {
  const random = seeded(7);
  const cells = Array.from({length: 7 * 14}, (_, i) => {
    const week = Math.floor(i / 7);
    const roll = random() * (0.5 + week / 18);
    const level = roll < 0.18 ? 0 : roll < 0.4 ? 1 : roll < 0.62 ? 2 : roll < 0.85 ? 3 : 4;
    return `<i class="heat-cell l${level}" style="--d:${week}"></i>`;
  }).join("");
  return `<div class="ga-heat">${cells}</div><div class="ga-scale">${t("Less")}${[0, 1, 2, 3, 4].map(level => `<span><i class="heat-cell l${level}"></i>${["0", "1", "2", "3–4", "5+"][level]}</span>`).join("")}${t("More")}</div>`;
}

function spread(highlight: number, label: string): string {
  const random = seeded(3);
  const spots = Array.from({length: 26}, () => random() ** 1.6);
  return `<div class="ga-spread">${spots.map((x, i) => `<i style="--x:${x.toFixed(3)}; --i:${i}"></i>`).join("")}<b style="--x:${highlight}"><em>${e(label)}</em></b></div>`;
}

function muscleHeat(): string {
  const rows: [Muscle, number][] = [["chest", 4], ["shoulders", 3], ["triceps", 2], ["abs", 1], ["calves", 0]];
  return `<div class="ga-muscles">${rows.map(([muscle, level], i) => `<span class="h${level}" style="--i:${i}">${e(t(MUSCLES[muscle]))}</span>`).join("")}</div><div class="ga-scale">${t("Rested")}${[0, 1, 2, 3, 4].map(level => `<i class="ga-h h${level}"></i>`).join("")}${t("Worked hard")}</div>`;
}

// ----- Your numbers -------------------------------------------------------------------------------------------

const weekOf = (c: GlossaryContext) => c.week && c.week.isCurrentWeek ? c.week : null;
const dayOf = (c: GlossaryContext) => c.daily && c.daily.date === c.snapshot.today.date ? c.daily : null;
const percent = (value: number | null) => value === null ? null : `${Math.round(value * 100)}%`;
const decimal = (value: number) => String(Math.round(value * 10) / 10);

// ----- The metrics --------------------------------------------------------------------------------------------

const TERMS: Term[] = [
  {
    id: "importance", section: "basics", name: msg("Importance"), icon: icons.flag, tone: "violet", where: ["board"],
    gist: msg("How much a task matters, from 1 to 10. You choose it for every task."),
    note: msg("It weighs your workload, picks your highlights, and can weigh today’s progress."),
    art: () => `${big("8", "/10")}${dots(8)}`,
    yours: c => { const load = workload(c.snapshot.tasks, c.snapshot.today.date); return load.importance === null ? null : `${decimal(load.importance)}<small>/10</small>`; }
  },
  {
    id: "progress", section: "basics", name: msg("Task progress"), icon: icons.subtasks, tone: "aqua", where: ["board"],
    gist: msg("How far along a task is, from 0 to 100%."),
    formula: [v(msg("Subtasks checked")), op("÷"), v(msg("All subtasks"))],
    note: msg("A task without subtasks uses the progress you set on it. 3 of 4 subtasks checked is 75%."),
    art: () => `<div class="ga-checks">${[true, true, true, false].map((done, i) => `<span class="${done ? "done" : ""}" style="--i:${i}"><i>${done ? icons.check : ""}</i><b></b></span>`).join("")}</div><div class="ga-meter"><i style="--p:.75"></i><b>75%</b></div>`,
    yours: c => { const steps = c.snapshot.tasks.flatMap(task => task.subtasks ?? []); return steps.length ? `${steps.filter(step => step.done).length}<small>/${steps.length}</small>` : null; }
  },
  {
    id: "outcomes", section: "basics", name: msg("Completed, cancelled, dropped"), icon: icons.check, tone: "aqua", where: ["activity", "week"],
    gist: msg("The three ways a task leaves your list. Only completed tasks count as finished."),
    art: () => `<div class="ga-seg">${([["completed", 6], ["cancelled", 2], ["dropped", 1]] as const).map(([status, size], i) => `<i class="${OUTCOME_TONE[status]}" style="flex-grow:${size}; --i:${i}">${STATUS_ICON[status]}</i>`).join("")}</div>`,
    extra: () => `<ul class="g-rows">${([["completed", msg("You finished it.")], ["cancelled", msg("It was added by mistake, or isn’t needed anymore.")], ["dropped", msg("You chose to let it go, to make room.")]] as const).map(([status, text]) => `<li class="${OUTCOME_TONE[status]}">${STATUS_ICON[status]}<b>${t(STATUS_LABEL[status])}</b><span>${t(text)}</span></li>`).join("")}</ul>`,
    yours: c => { const day = dayOf(c); return day ? `<span class="g-mini">${icons.check}${day.completed}</span><span class="g-mini">${icons.ban}${day.cancelled}</span><span class="g-mini">${icons.dropped}${day.dropped}</span>` : null; }
  },
  {
    id: "time-to-finish", section: "basics", name: msg("Time to finish"), icon: icons.hourglass, tone: "yellow", where: ["activity"],
    gist: msg("How long a task stayed on your list, from the moment you added it to the moment you closed it."),
    formula: [v(msg("Closed at")), op("−"), v(msg("Added at"))],
    note: msg("Added Monday at 9:00 and completed Tuesday at 15:00 is 30h."),
    art: journey
  },
  {
    id: "finished-today", section: "day", name: msg("Finished today"), icon: icons.check, tone: "aqua", where: ["day"],
    gist: msg("Tasks you completed today, with the hour you finished each one."),
    formula: [v(msg("Completed today")), op("−"), v(msg("Yesterday")), op("="), v(msg("The arrow chip"))],
    note: msg("A second chip compares today with your usual day. Green and pointing up means more than usual."),
    art: () => `<div class="ga-count">${big("5")}<div class="ga-deltas">${delta(5, 3, {label: t("yesterday")})}${delta(5, 3.5, {label: t("your usual day"), digits: 1})}</div></div>${bars([0, 0, 0, 1, 2, 0, 1, 0, 0, 3, 1, 0], {thin: true})}`,
    yours: c => { const day = dayOf(c); return day ? String(day.completed) : null; }
  },
  {
    id: "usual-day", section: "day", name: msg("Your usual day"), icon: icons.target, tone: "aqua", where: ["day"],
    gist: msg("What an ordinary day looks like for you, so today is measured against something fair."),
    formula: [op("Σ"), v(msg("Tasks finished each day")), op("÷"), v(msg("28 days"))],
    note: msg("Days before your history began are left out, so a new workspace isn’t compared with empty days."),
    art: () => { const random = seeded(11); return bars(Array.from({length: 28}, () => Math.round(random() * 5)), {line: 2.5, label: t("usual {value}", {value: "2.5"}), thin: true}); },
    yours: c => { const day = dayOf(c); return day && day.usual !== null ? decimal(day.usual) : null; }
  },
  {
    id: "workload", section: "day", name: msg("Workload"), icon: icons.gauge, tone: "violet", where: ["day", "settings"], wide: true,
    gist: msg("How much open work is waiting for you, weighed by how much each task matters."),
    formula: [op("Σ"), v(msg("Importance")), op("÷ 10"), op("×"), v(msg("Share still left"))],
    note: msg("An importance-8 task counts 0.8 untouched, 0.4 half done, and nothing once it’s done. Tasks postponed to a later day wait until then."),
    art: gauge,
    extra: c => {
      const load = workload(c.snapshot.tasks, c.snapshot.today.date);
      const ranges = levelRanges();
      const levels = loadLevels().map((level, i) => `<li class="g-level load-${level.id} ${level.id === load.level ? "is-you" : ""}" style="--i:${i}" data-tip="${e(level.note)}">${level.icon}<b>${e(level.label)}</b><small>${e(ranges[i])}</small>${level.id === load.level ? `<em>${t("You are here")}</em>` : ""}</li>`).join("");
      const tryIt = `<div class="g-try"><span class="g-k">${icons.zap}${t("Try it")}</span><label><span>${t("Importance")}</span><input type="range" name="gTryImportance" min="1" max="10" step="1" value="8"><output data-try="importance">8</output></label><label><span>${t("Progress")}</span><input type="range" name="gTryProgress" min="0" max="100" step="10" value="50"><output data-try="progress">50%</output></label><div class="g-try-meter"><i data-try="meter" style="--p:.4"></i></div><p>${t("This task adds {points} to your workload.", {points: `<b data-try="points">0.4</b>`})}</p></div>`;
      return `<div class="g-extra"><div><span class="g-k">${icons.waves}${t("Levels")}</span><ol class="g-levels">${levels}</ol><p class="g-fine">${t("The page’s colors follow your level. Where each one starts is up to you in Settings.")}</p></div>${tryIt}</div>`;
    },
    yours: c => { const load = workload(c.snapshot.tasks, c.snapshot.today.date); return `${load.points.toFixed(1)} <small>${e(levelInfo(load.level).label)}</small>`; }
  },
  {
    id: "timed-work", section: "day", name: msg("Today’s timed work"), icon: icons.alarm, tone: "violet", where: ["day", "settings"],
    gist: msg("One bar for everything you planned for a time today, filling as that work gets done."),
    formula: [fn(msg("Average of")), v(msg("Progress of each timed task due by tonight"))],
    note: msg("Finished ones count as 100%. With “By importance” on in Settings, an importance-9 task moves the bar three times as much as an importance-3 one."),
    art: () => `${big("63", "%")}<div class="ga-timed">${[[1, 3], [1, 2], [0.5, 4], [0, 2]].map(([fill, size], i) => `<i style="--p:${fill}; flex-grow:${size}; --i:${i}"></i>`).join("")}</div>`,
    yours: c => { const today = todayProgress(c.snapshot, c.now, c.weighted); return today.parts.length ? percent(today.value) : null; }
  },
  {
    id: "open-progress", section: "day", name: msg("All open tasks"), icon: icons.target, tone: "aqua", where: ["day", "settings"],
    gist: msg("The average progress of all your open tasks, timed or not."),
    formula: [fn(msg("Average of")), v(msg("Progress of each open task"))],
    note: msg("Tasks postponed to a later day are left out. Two tasks at 0% and 50% make 25%."),
    art: () => ring({value: 0.62, size: 104, label: "", tone: "sw-aqua"}),
    yours: c => percent(openProgress(c.snapshot.tasks, c.snapshot.today.date, c.weighted))
  },
  {
    id: "streak", section: "day", name: msg("Streak"), icon: icons.flame, tone: "orange", where: ["day", "week"],
    gist: msg("Days in a row on which you finished at least one task."),
    formula: [fn(msg("Count")), v(msg("Days in a row with a finished task"))],
    note: msg("Today joins once you finish something. Until then the streak runs through yesterday, so it never breaks before the day is over."),
    art: () => `<div class="ga-streak">${big("5")}<div class="ga-flames">${Array.from({length: 7}, (_, i) => `<i class="${i < 5 ? "lit" : ""}" style="--i:${i}">${icons.flame}</i>`).join("")}</div></div>`,
    yours: c => { const streak = dayOf(c)?.streak ?? weekOf(c)?.streak; return streak === undefined ? null : `${streak} <small>${tn("day", "days", streak)}</small>`; }
  },
  {
    id: "habits", section: "day", name: msg("Habit consistency"), icon: icons.sprout, tone: "green", where: ["day", "week"],
    gist: msg("How many habit check-ins you made, out of all you could have made so far."),
    formula: [v(msg("Check-ins")), op("÷"), op("("), v(msg("Habits")), op("×"), v(msg("Days so far")), op(")")],
    note: msg("3 habits over 4 days is 12 check-ins, so making 9 of them is 75%. The weekly ring compares with last week, in points."),
    art: () => ring({value: 0.75, compare: 0.6, size: 104, label: "", tone: "sw-green"}),
    yours: c => {
      const rate = weekOf(c)?.current.habitRate;
      if (rate !== undefined && rate !== null) return pctText(rate);
      const habits = c.snapshot.habits;
      return habits.length ? `${habits.filter(habit => habit.completed).length}<small>/${habits.length}</small>` : null;
    }
  },
  {
    id: "highlights", section: "day", name: msg("Highlights"), icon: icons.trophy, tone: "yellow", where: ["day", "week"],
    gist: msg("The three most important tasks you finished that day or week, so big wins don’t get buried."),
    note: msg("When two are equally important, the one finished later comes first."),
    art: () => `<div class="ga-podium">${[[2, 0.62], [1, 1], [3, 0.42]].map(([rank, height], i) => `<span style="--h:${height}; --i:${i}">${rank === 1 ? icons.crown : ""}<b>${rank}</b></span>`).join("")}</div>`,
    yours: c => { const top = (dayOf(c)?.highlights ?? [])[0] ?? weekOf(c)?.highlights[0]; return top ? `<span class="g-clip">${e(top.description)}</span>` : null; }
  },
  {
    id: "week-completed", section: "week", name: msg("Tasks completed this week"), icon: icons.check, tone: "blue", where: ["week"],
    gist: msg("Tasks completed from Monday to Sunday, with the seven weeks before it as bars."),
    formula: [v(msg("This week")), op("−"), v(msg("Last week"))],
    note: msg("A second chip compares the week with your usual week, the dashed line across the bars."),
    art: () => bars([3, 5, 4, 6, 5, 7, 6, 8], {line: 5.5, label: t("usual {value}", {value: "5.5"}), now: true}),
    yours: c => { const week = weekOf(c); return week ? String(week.current.completed) : null; }
  },
  {
    id: "usual-week", section: "week", name: msg("Your usual week"), icon: icons.target, tone: "blue", where: ["week"],
    gist: msg("The average of the four weeks before the one you’re looking at."),
    formula: [op("Σ"), v(msg("Each earlier week")), op("÷"), v(msg("Weeks with activity"))],
    note: msg("Weeks before you started are skipped, so the comparison is fair from your second week."),
    art: () => `<div class="ga-merge">${bars([4, 6, 5, 7], {line: 5.5, label: t("usual {value}", {value: "5.5"})})}</div>`,
    yours: c => { const week = weekOf(c); return week && week.baseline.completed !== null ? decimal(week.baseline.completed) : null; }
  },
  {
    id: "effort-mix", section: "week", name: msg("Where your effort went"), icon: icons.palette, tone: "orange", where: ["week"],
    gist: msg("Tasks completed this week in each type, so you see where your time really went."),
    formula: [v(msg("Completed per type")), op("↔"), v(msg("Usual per type"))],
    note: msg("The tick on each bar is that type’s usual week. A bar past its tick means more than usual."),
    art: () => `<div class="ga-mix">${([["blue", 0.9, 0.6], ["orange", 0.55, 0.7], ["aqua", 0.3, 0.25]] as const).map(([tone, width, tick], i) => `<span class="sw-${tone}" style="--i:${i}"><b style="--w:${width}"></b><em style="--x:${tick}"></em></span>`).join("")}</div>`,
    yours: c => {
      const week = weekOf(c);
      const top = week ? Object.entries(week.current.byType).sort((a, b) => b[1] - a[1])[0] : null;
      return week && top ? `<span class="g-mini ${sw(typeSwatch(week.taskTypes, top[0]))}"><i class="g-swatch"></i>${e(week.taskTypes[top[0]] ?? top[0])} ${top[1]}</span>` : null;
    }
  },
  {
    id: "flow", section: "week", name: msg("Outcomes & flow"), icon: icons.flow, tone: "blue", where: ["week"],
    gist: msg("How many tasks arrived and how many left, so you know whether your list is growing."),
    formula: [v(msg("Closed")), op("−"), v(msg("Added")), op("="), v(msg("How much your list shrank"))],
    note: msg("Closed means completed, cancelled, and dropped together. Closing 9 and adding 6 shrinks your list by 3."),
    art: () => `<div class="ga-flow"><span class="sw-blue" style="--i:0"><small>${t("Added")}</small><b style="--w:.66"></b><em>6</em></span><span class="sw-aqua" style="--i:1"><small>${t("Closed")}</small><b style="--w:1"></b><em>9</em></span><p>${icons.down}<b>3</b></p></div>`,
    yours: c => { const week = weekOf(c); if (!week) return null; const {completed, cancelled, dropped, added} = week.current; return `${t("Added")} ${added} · ${t("Closed")} ${completed + cancelled + dropped}`; }
  },
  {
    id: "median-time", section: "week", name: msg("Time to finish (median)"), icon: icons.hourglass, tone: "yellow", where: ["week", "day"],
    gist: msg("The middle time to finish among the tasks you completed: half were quicker, half took longer."),
    formula: [fn(msg("Median of")), v(msg("Time to finish"))],
    note: msg("One task that sat for a month would drag an average along; the median barely moves. Lower is faster, so a drop is good news."),
    art: () => `<div class="ga-median">${[0.06, 0.12, 0.2, 0.3, 0.38, 0.5, 0.95].map((x, i) => `<i class="${i === 3 ? "is-mid" : ""}" style="--x:${x}; --i:${i}">${i === 3 ? `<em>${t("median")}</em>` : ""}</i>`).join("")}</div>`,
    yours: c => { const hours = weekOf(c)?.current.medianLeadHours ?? dayOf(c)?.medianLeadHours ?? null; return hours === null ? null : formatHours(hours); }
  },
  {
    id: "avg-importance", section: "week", name: msg("Importance of finished tasks"), icon: icons.flag, tone: "violet", where: ["week"],
    gist: msg("The average importance of the tasks you completed, from 1 to 10."),
    formula: [op("Σ"), v(msg("Importance")), op("÷"), v(msg("Tasks completed"))],
    note: msg("A high number means your week went to the things that matter most."),
    art: () => `<div class="ga-track"><span class="ga-link" style="--a:.5; --b:.71"></span><i class="ga-hollow" style="--x:.5"><em>${t("usual {value}", {value: "5.5"})}</em></i><i class="ga-bright" style="--x:.71"><em>7.4</em></i><small>1</small><small>10</small></div>`,
    yours: c => { const value = weekOf(c)?.current.avgPriority; return value === undefined || value === null ? null : `${decimal(value)}<small>/10</small>`; }
  },
  {
    id: "rhythm", section: "history", name: msg("Your rhythm"), icon: icons.grid, tone: "violet", where: ["activity"],
    gist: msg("One square for each day of the last 22 weeks. The deeper the color, the more you finished."),
    note: msg("Pick a square to see what you finished that day."),
    art: heat
  },
  {
    id: "pace", section: "history", name: msg("Pace among your tasks"), icon: icons.zap, tone: "magenta", where: ["activity"],
    gist: msg("How one task’s time to finish compares with every other task you completed."),
    formula: [v(msg("Tasks that took longer")), op("÷"), v(msg("Other completed tasks"))],
    note: msg("“Quicker than 80%” means four in five of your completed tasks took longer. Hover over a task, or open it, to see its pace."),
    art: () => spread(0.2, "80%")
  },
  {
    id: "share", section: "history", name: msg("Share of everything"), icon: icons.percent, tone: "magenta", where: ["activity"],
    gist: msg("With Activity viewed by one category, that category’s part of everything you finished."),
    formula: [v(msg("Finished in this category")), op("÷"), v(msg("All finished tasks"))],
    note: msg("Cancelled and dropped tasks are left out of both."),
    art: () => `${big("30", "%")}<div class="ga-seg is-share">${[[3, true], [4, false], [2, false], [1, false]].map(([size, mine], i) => `<i class="${mine ? "sw-magenta" : "is-other"}" style="flex-grow:${size}; --i:${i}"></i>`).join("")}</div>`
  },
  {
    id: "weekly-goal", section: "plugins", name: msg("Weekly goal"), icon: icons.target, tone: "orange", where: ["plugins"], kinds: ["fitness", "tracker"],
    gist: msg("Rings that fill toward the goal you set for each week, starting on Monday."),
    formula: [v(msg("This week")), op("÷"), v(msg("Weekly goal"))],
    note: msg("Fitness counts workouts. A tracker adds up each number’s week as a total, its best, or its average, as you chose for it."),
    art: () => ring({value: 0.75, size: 104, label: "", center: "3<small>/4</small>", tone: "sw-orange"})
  },
  {
    id: "plugin-streak", section: "plugins", name: msg("Plugin streaks"), icon: icons.flame, tone: "orange", where: ["plugins"], kinds: ["fitness", "tracker"],
    gist: msg("Fitness counts weeks in a row that reached your workout goal. A tracker counts days in a row with something logged."),
    note: msg("This week joins the fitness streak as soon as it reaches the goal."),
    art: () => `<div class="ga-weeks">${[1, 1, 0.5, 1, 1, 1].map((fill, i) => `<span class="${fill === 1 && i > 2 ? "lit" : ""}" style="--p:${fill}; --i:${i}">${fill === 1 && i > 2 ? icons.flame : ""}</span>`).join("")}</div>${big("3")}`
  },
  {
    id: "volume", section: "plugins", name: msg("Volume"), icon: icons.dumbbell, tone: "violet", where: ["plugins"], kinds: ["fitness"],
    gist: msg("How much weight you moved: every set of a lifting exercise, added up."),
    formula: [op("Σ"), v(msg("Weight")), op("×"), v(msg("Reps"))],
    note: msg("3 sets of 5 at 60 kg is 900 kg. Bodyweight, timed, and cardio exercises add no volume."),
    art: () => `<div class="ga-barbell"><span class="ga-plates">${[0, 1, 2].map(i => `<i style="--i:${i}"></i>`).join("")}</span><b></b><span class="ga-plates is-right">${[0, 1, 2].map(i => `<i style="--i:${i}"></i>`).join("")}</span></div><p class="ga-sum">3 × 5 × 60 kg = <b>900 kg</b></p>`
  },
  {
    id: "calories", section: "plugins", name: msg("Calories (estimated)"), icon: icons.flame, tone: "red", where: ["plugins"], kinds: ["fitness"],
    gist: msg("A rough estimate of the energy a workout burned."),
    formula: [v("MET"), op("×"), v(msg("Body weight in kg")), op("×"), v(msg("Hours"))],
    note: msg("MET is how hard an exercise is next to resting: about 10 for a run, 3 for a plank. Without a logged weight, 70 kg is used."),
    art: () => `<div class="ga-burn">${icons.flame}${big("~320", t("kcal"))}</div>`
  },
  {
    id: "muscle-heat", section: "plugins", name: msg("Muscle heat"), icon: icons.muscle, tone: "orange", where: ["plugins"], kinds: ["fitness"],
    gist: msg("How hard each muscle worked over the last 7 days, shaded on the body figure."),
    formula: [op("Σ"), v(msg("Sets done")), fn(msg("per muscle"))],
    note: msg("A muscle an exercise works less gets half a set; a cardio exercise counts as two sets."),
    art: muscleHeat
  },
  {
    id: "one-rep-max", section: "plugins", name: msg("Estimated 1RM"), icon: icons.medal, tone: "yellow", where: ["plugins"], kinds: ["fitness"],
    gist: msg("The heaviest single lift your best set suggests you could manage. Personal records are ranked by it."),
    formula: [v(msg("Weight")), op("×"), op("("), op("1 +"), v(msg("Reps")), op("÷ 30"), op(")")],
    note: msg("5 reps at 80 kg suggests about 93 kg. This is the Epley formula."),
    art: () => `<div class="ga-lift">${bars([5, 5, 5, 5, 5, 9.3], {now: true})}<p><span>80 kg × 5</span>${icons.arrow}<b>${t("1RM ~{weight}", {weight: "93 kg"})}</b></p></div>`
  }
];

/** Every metric's id, in the order the page lists them. */
export const GLOSSARY_IDS = TERMS.map(term => term.id);
export const GLOSSARY_SECTIONS = SECTIONS.map(section => section.id);
const yoursBadge = (value: string) => `<span class="g-yours" data-tip="${e(t("Your number right now"))}"><small>${t("Yours")}</small><b>${value}</b></span>`;

// ----- The page -----------------------------------------------------------------------------------------------

function formula(parts: Part[]): {html: string; text: string} {
  const text = parts.map(part => "v" in part ? t(part.v) : "fn" in part ? t(part.fn) : part.op).join(" ");
  const html = parts.map((part, i) => "v" in part ? `<span class="g-v" style="--i:${i}">${t(part.v)}</span>` : "fn" in part ? `<span class="g-fn" style="--i:${i}">${t(part.fn)}</span>` : `<span class="g-op" style="--i:${i}">${e(part.op)}</span>`).join("");
  return {text, html};
}

function spots(term: Term, s: Snapshot): {html: string; text: string} {
  const links: [string, string][] = term.where.flatMap((spot): [string, string][] => {
    if (spot !== "plugins") return [[spot, t(SPOTS[spot])]];
    const plugins = (s.plugins ?? []).filter(plugin => term.kinds?.includes(plugin.kind) && plugin.settings.page);
    return plugins.length ? plugins.map((plugin): [string, string] => [`plugin:${plugin.id}`, plugin.name]) : [["settings", t("Add a plugin in Settings")]];
  });
  return {
    text: links.map(([, label]) => label).join(" "),
    html: links.map(([target, label]) => `<button type="button" class="g-spot" data-glossary-jump="${e(target)}">${e(label)}${icons.arrow}</button>`).join("")
  };
}

function card(term: Term, c: GlossaryContext): string {
  const name = t(term.name);
  const math = term.formula ? formula(term.formula) : null;
  const where = spots(term, c.snapshot);
  const yours = term.yours?.(c) ?? null;
  const search = [name, t(term.gist), term.note ? t(term.note) : "", math?.text ?? "", where.text].join(" ").toLowerCase();
  return `<article class="g-term ${sw(term.tone)} ${term.wide ? "is-wide" : ""}" id="g-${term.id}" data-term="${term.id}" data-search="${e(search)}" tabindex="-1" aria-labelledby="g-${term.id}-name">
    <div class="g-art" aria-hidden="true"><div class="g-art-inner">${term.art(c)}</div></div>
    <div class="g-body">
      <header class="g-head"><span class="g-icon">${term.icon}</span><h3 id="g-${term.id}-name">${e(name)}</h3>${yours ? yoursBadge(yours) : ""}</header>
      <p class="g-gist">${t(term.gist)}</p>
      ${math ? `<div class="g-formula"><span class="g-k">${icons.sigma}${t("How it’s counted")}</span><p class="g-eq" role="math" aria-label="${e(math.text)}">${math.html}</p></div>` : ""}
      ${term.extra?.(c) ?? ""}
      ${term.note ? `<p class="g-note">${icons.bulb}<span>${t(term.note)}</span></p>` : ""}
      <footer class="g-where"><span class="g-k">${icons.pin}${t("Find it on")}</span>${where.html}</footer>
    </div>
  </article>`;
}

export function glossaryPage(c: GlossaryContext): string {
  const toc = SECTIONS.map((section, i) => {
    const count = TERMS.filter(term => term.section === section.id).length;
    return `<button type="button" class="g-toc-item sw-${section.tone}" style="--i:${i}" data-glossary-section="${section.id}"><span class="g-toc-icon">${section.icon}</span><strong>${t(section.name)}</strong><small>${tn("{count} metric", "{count} metrics", count)}</small></button>`;
  }).join("");
  const sections = SECTIONS.map(section => `<section class="g-section sw-${section.tone}" id="g-sec-${section.id}" data-section="${section.id}" aria-labelledby="g-sec-${section.id}-title">
      <header class="g-sec-head"><span class="g-sec-badge">${section.icon}</span><div><h2 id="g-sec-${section.id}-title">${t(section.name)}</h2><p class="muted">${t(section.blurb)}</p></div></header>
      <div class="g-grid">${TERMS.filter(term => term.section === section.id).map(term => card(term, c)).join("")}</div>
    </section>`).join("");
  return `<div class="page-heading g-heading"><div><p class="eyebrow">${icons.glossary} ${t("WHAT THE NUMBERS MEAN")}</p><h1>${t("Every number, explained")}<span class="accent-dot">.</span></h1><p class="muted">${t("What each number in your workspace measures, how it’s counted, and where to find it. When your own number is ready, it sits beside the name.")}</p></div>
      <label class="search-pill g-search">${icons.search}<span class="sr-only">${t("Search the glossary")}</span><input name="glossarySearch" type="search" placeholder="${t("Search the glossary…")}" autocomplete="off"></label></div>
    <nav class="g-toc" aria-label="${t("Glossary sections")}">${toc}</nav>
    ${sections}
    <div class="g-none empty" hidden><span class="empty-blob">${icons.search}</span><p>${t("No metric matches your search.")}</p></div>`;
}

// ----- Behavior -----------------------------------------------------------------------------------------------

const reveal = (term: Element) => term.classList.add("is-seen");

/** Scroll to a metric (and flash it) or to a section. */
export function showInGlossary(root: HTMLElement, target: string, reducedMotion: boolean): void {
  const metric = GLOSSARY_IDS.includes(target);
  const element = metric || (GLOSSARY_SECTIONS as string[]).includes(target) ? root.querySelector<HTMLElement>(metric ? `#g-${target}` : `#g-sec-${target}`) : null;
  if (!element) return;
  element.scrollIntoView?.({behavior: reducedMotion ? "auto" : "smooth", block: "start"});
  if (!metric) return;
  reveal(element);
  element.classList.remove("is-flash");
  void element.offsetWidth;
  element.classList.add("is-flash");
  element.focus({preventScroll: true});
}

/** Search, section jumps, the try-it sliders, and the pictures that play as cards scroll into view. `jump`
 *  follows a "Find it on" link. */
export function bindGlossary(root: HTMLElement, options: {jump: (target: string) => void; reducedMotion: boolean}): void {
  const page = root.querySelector<HTMLElement>(".content");
  if (!page) return;
  const terms = [...page.querySelectorAll<HTMLElement>(".g-term")];

  // Pictures wait (paused on their first frame) until their card is on screen.
  if (options.reducedMotion || typeof IntersectionObserver !== "function") terms.forEach(reveal);
  else {
    const observer = new IntersectionObserver(entries => entries.forEach(entry => {
      if (!entry.isIntersecting) return;
      reveal(entry.target);
      observer.unobserve(entry.target);
    }), {threshold: 0.25});
    terms.forEach(term => observer.observe(term));
  }

  const search = page.querySelector<HTMLInputElement>('[name="glossarySearch"]');
  if (search) search.oninput = () => {
    const words = search.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
    for (const term of terms) {
      const hit = words.every(word => term.dataset.search!.includes(word));
      term.hidden = !hit;
      if (hit) reveal(term);
    }
    page.querySelectorAll<HTMLElement>(".g-section").forEach(section => { section.hidden = !section.querySelector(".g-term:not([hidden])"); });
    page.querySelector<HTMLElement>(".g-none")!.hidden = terms.some(term => !term.hidden);
  };

  page.querySelectorAll<HTMLButtonElement>("[data-glossary-section]").forEach(button => button.onclick = () => showInGlossary(page, button.dataset.glossarySection!, options.reducedMotion));
  page.querySelectorAll<HTMLButtonElement>("[data-glossary-jump]").forEach(button => button.onclick = () => options.jump(button.dataset.glossaryJump!));

  // Try it: one task's share of the workload, as its importance and progress change.
  const importance = page.querySelector<HTMLInputElement>('[name="gTryImportance"]');
  const progress = page.querySelector<HTMLInputElement>('[name="gTryProgress"]');
  const shown = (key: string) => page.querySelector<HTMLElement>(`[data-try="${key}"]`);
  const update = () => {
    if (!importance || !progress) return;
    const points = Number(importance.value) / 10 * (1 - Number(progress.value) / 100);
    shown("importance")!.textContent = importance.value;
    shown("progress")!.textContent = `${progressOf({progress: Number(progress.value)})}%`;
    shown("points")!.textContent = String(Math.round(points * 100) / 100);
    shown("meter")!.style.setProperty("--p", points.toFixed(3));
  };
  if (importance) importance.oninput = update;
  if (progress) progress.oninput = update;
}

/** Put your numbers beside the metrics again, after the daily or weekly overview has loaded. */
export function updateGlossaryValues(root: HTMLElement, c: GlossaryContext): void {
  for (const term of TERMS) {
    const head = root.querySelector(`#g-${term.id} .g-head`);
    if (!head) continue;
    head.querySelector(".g-yours")?.remove();
    const value = term.yours?.(c) ?? null;
    if (value) head.insertAdjacentHTML("beforeend", yoursBadge(value));
  }
}
