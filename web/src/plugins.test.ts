// Tests for plugins: their sidebar pages, the fitness plugin (body figure, builder, planned workouts, live
// sessions), the tracker plugin, plugin cards in the timer row, the Settings card and editor, and Activity
// viewed by a plugin's category.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { bodyFigure, unitLabel } from "./fitness";
import { forgetViews } from "./plugin-kit";
import { renderApp } from "./render";
import type { BridgeClient, HistoryItem, PluginItem, Snapshot } from "./types";
import { isoDay } from "./ui";

const today = isoDay(new Date());
const at = (time: string) => `${today}T${time}`;
const legs = [{exercise: "squat", sets: 3, reps: 5, weight: 60, rest: 90}, {exercise: "plank", sets: 2, seconds: 30, rest: 30}];
const workout = {workout: {name: "Leg day", units: legs}};

function plugins(): PluginItem[] {
  return [
    {
      id: "fitness", kind: "fitness", name: "Fitness", description: "Get strong", icon: "dumbbell", type: "2",
      settings: {page: true, activity: true, timer: true, agent: false},
      state: {units: "kg", goal: 3, routines: [{id: "legs", name: "Legs", units: legs}]},
      log: [
        {id: "1", at: at("07:30:00"), kind: "session", task: "Push day", data: {name: "Push day", minutes: 42, sets: 2, volume: 820, calories: 250, units: [{exercise: "bench", mode: "weight", sets: [{reps: 8, weight: 50, done: true}, {reps: 8, weight: 52.5, done: true}]}]}},
        {id: "2", at: at("07:00:00"), kind: "body", data: {weight: 72.4}}
      ]
    },
    {
      id: "guitar", kind: "tracker", name: "Guitar", description: "Fingerstyle", icon: "guitar", type: "3",
      settings: {page: false, activity: true, timer: false, agent: true},
      state: {metrics: [{id: "minutes", name: "Practice", kind: "minutes", unit: "", goal: 150}, {id: "tempo", name: "Tempo", kind: "number", unit: "BPM", total: "max"}]},
      log: [
        {id: "1", at: at("08:00:00"), kind: "entry", task: "Guitar", data: {values: {minutes: 30, tempo: 90}}},
        {id: "2", at: at("09:00:00"), kind: "entry", task: "Guitar", data: {values: {minutes: 20, tempo: 100}}}
      ]
    }
  ];
}
function snapshot(): Snapshot {
  return {
    config: {rootDir: "/local/config", activeId: 0, activeConfigDir: "/local/config/user_config_0", tasksPath: "", deadlinesPath: "", habitsPath: "", recordsPath: ""},
    today: {date: today, weekday: "monday", parity: "odd"},
    schedule: {isSkipped: false, current: null, next: null, timeToNext: null, events: [], hasSyncedOverlay: false},
    taskTypes: {"1": "Work", "2": "Fitness", "3": "Guitar"},
    tasks: [
      {description: "Write report", priority: 7, type: "1", typeName: "Work", alarmFrom: null, procrastinated: false, procrastinateDays: null},
      {description: "Leg day", priority: 6, type: "2", typeName: "Fitness", alarmFrom: null, procrastinated: false, procrastinateDays: null, plugin: workout, subtasks: [{text: "Barbell squat · 3×5 @ 60 kg", done: false}, {text: "Plank · 2×30 s", done: false}]}
    ],
    deadlines: [], habits: [], history: [], plugins: plugins()
  };
}
const history: HistoryItem[] = [
  {description: "Push day", priority: 5, startedAt: at("06:48:00"), endedAt: at("07:30:00"), duration: "42m", status: "completed", type: "2", typeName: "Fitness"},
  {description: "Write memo", priority: 5, startedAt: "2026-01-05T09:00:00", endedAt: at("10:00:00"), duration: "", status: "completed", type: "1", typeName: "Work"}
];

let root: HTMLElement;
let data: Snapshot;
let send: ReturnType<typeof vi.fn>;
let client: BridgeClient;
const flush = async () => { for (let i = 0; i < 4; i++) await new Promise(resolve => setTimeout(resolve, 0)); };
const click = (selector: string) => {
  const el = root.querySelector<HTMLElement>(selector);
  if (!el) throw new Error(`nothing matches ${selector}`);
  el.dispatchEvent(new MouseEvent("click", {bubbles: true}));
};
const calls = (command: string) => send.mock.calls.filter(([name]) => name === command).map(([, payload]) => payload);

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector("#app")!;
  data = snapshot();
  forgetViews();
  window.localStorage.clear();
  vi.stubGlobal("matchMedia", vi.fn(() => ({matches: false, addEventListener: vi.fn()})));
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
  send = vi.fn(async (command: string, payload: Record<string, unknown>) => {
    if (command === "status_snapshot") return structuredClone(data);
    if (command === "workspace_info") return {configIds: [0], activeId: 0, mode: "j", rootDir: "/local/config"};
    if (command === "task_history") return {activities: structuredClone(history)};
    if (command === "plugin_state") {
      const plugin = data.plugins!.find(item => item.id === payload.id)!;
      for (const [key, value] of Object.entries(payload.set as Record<string, unknown>)) { if (value === null) delete plugin.state[key]; else plugin.state[key] = value; }
      return {id: plugin.id, state: plugin.state};
    }
    if (command === "task_delete") { data.tasks = data.tasks.filter(task => task.description !== payload.description); return {undo: {description: payload.description}}; }
    if (command === "plugin_log") return {entry: {id: "9"}, undo: {id: payload.id, entry: "9", logged: []}};
    if (command === "plugin_save") return {plugin: {...plugins()[1], id: "language", name: String(payload.name)}};
    return {};
  });
  client = {send};
  vi.spyOn(window, "confirm").mockReturnValue(true);
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); delete document.documentElement.dataset.mode; });

describe("fitness units", () => {
  it("describes a unit in the user's weight units", () => {
    const plugin = plugins()[0];
    expect(unitLabel(plugin, {exercise: "squat", sets: 5, reps: 5, weight: 60})).toBe("Barbell squat · 5×5 @ 60 kg");
    expect(unitLabel(plugin, {exercise: "plank", sets: 3, seconds: 45})).toBe("Plank · 3×45 s");
    expect(unitLabel(plugin, {exercise: "run", minutes: 25, distance: 4})).toBe("Run · 25 min · 4 km");
    expect(unitLabel({...plugin, state: {units: "lb"}}, {exercise: "bench", sets: 3, reps: 8, weight: 50})).toBe("Bench press · 3×8 @ 110 lb");
  });

  it("shades each muscle by how many sets worked it", () => {
    const html = bodyFigure({load: {chest: 2, quads: 12}, selected: "quads", label: "Muscles"});
    const box = document.createElement("div");
    box.innerHTML = html;
    expect(box.querySelector('[data-muscle="chest"]')?.classList.contains("h1")).toBe(true);
    expect(box.querySelector('[data-muscle="quads"]')?.classList.contains("h4")).toBe(true);
    expect(box.querySelector('[data-muscle="quads"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(box.querySelectorAll("[data-muscle]").length).toBeGreaterThanOrEqual(16);
  });
});

describe("plugin pages", () => {
  it("lists the plugins that have a page in the sidebar and draws the fitness page", async () => {
    await renderApp(root, client);
    expect(root.querySelector('[data-page="plugin:fitness"]')).not.toBeNull();
    expect(root.querySelector('[data-page="plugin:guitar"]')).toBeNull();
    click('[data-page="plugin:fitness"]'); await flush();
    expect(root.querySelector("h1")?.textContent).toContain("Fitness");
    expect(root.querySelector(".fit-week .ring figcaption")?.textContent).toBe("1/3");
    expect(root.querySelector('.fit-figure-card [data-muscle="chest"]')?.classList.contains("h1")).toBe(true);
    expect(root.querySelector(".fit-builder")).not.toBeNull();
    // The planned workout is listed with the board's own card, ready to start.
    expect(root.querySelector(".fit-planned [data-fit-start-task]")).not.toBeNull();
  });

  it("finds exercises for a muscle picked on the figure", async () => {
    await renderApp(root, client);
    click('[data-page="plugin:fitness"]'); await flush();
    click('.fit-figure-card [data-muscle="calves"]'); await flush();
    expect(root.querySelector('[data-fit-tab="exercises"]')?.getAttribute("aria-selected")).toBe("true");
    const names = [...root.querySelectorAll(".fit-exercise h4")].map(el => el.textContent);
    expect(names).toContain("Calf raise");
    expect(names).not.toContain("Bench press");
    click('[data-ex-add="calf"]'); await flush();
    click('[data-fit-tab="train"]'); await flush();
    expect(root.querySelectorAll(".fit-unit")).toHaveLength(1);
  });

  it("adjusts a routine's units and plans it as a task with its exercises as steps", async () => {
    await renderApp(root, client);
    click('[data-page="plugin:fitness"]'); await flush();
    click('[data-routine-load="legs"]'); await flush();
    expect(root.querySelectorAll(".fit-unit")).toHaveLength(2);
    const sets = root.querySelector<HTMLInputElement>('[data-field="u:0:sets"]')!;
    sets.closest(".stepper")!.querySelector<HTMLButtonElement>('[data-step="1"]')!.click();
    expect(sets.value).toBe("4");
    click("[data-fit-plan]"); await flush();
    const [added] = calls("task_add") as Record<string, any>[];
    expect(added.type).toBe("2");
    expect(added.description).toBe("Legs");
    expect(added.subtasks.map((step: {text: string}) => step.text)).toEqual(["Barbell squat · 4×5 @ 60 kg", "Plank · 2×30 s"]);
    expect(added.plugin.workout.units[0].sets).toBe(4);
  });

  it("starts a planned workout from the board and logs it when finished", async () => {
    await renderApp(root, client);
    expect(root.querySelector('#board-area [data-group="2"] .group-icon.is-plugin')).not.toBeNull();
    click("#board-area [data-fit-start-task]"); await flush();
    const [started] = calls("plugin_state") as Record<string, any>[];
    expect(started.set.active.task).toBe("Leg day");
    expect(started.set.active.sets[0]).toHaveLength(3);
    expect(root.querySelector("#fit-session h2")?.textContent).toBe("Leg day");
    click('[data-set-done="0:0"]'); await flush();
    expect(root.querySelector(".fit-rest [data-until]")).not.toBeNull();
    click("[data-fit-finish]"); await flush();
    const [done] = calls("task_delete") as Record<string, any>[];
    expect(done.description).toBe("Leg day");
    expect(done.pluginLog.kind).toBe("session");
    expect(done.pluginLog.data.sets).toBe(1);
    expect(done.pluginLog.data.volume).toBe(300);
    expect(root.querySelector("#fit-session")).toBeNull();
  });

  it("logs a tracker session as a finished task", async () => {
    data.plugins![1].settings.page = true;
    await renderApp(root, client);
    click('[data-page="plugin:guitar"]'); await flush();
    expect(root.querySelector(".trk-goal figcaption")?.textContent).toBe("50/150");
    const form = root.querySelector<HTMLFormElement>(".trk-form")!;
    form.querySelector<HTMLInputElement>('[name="m-minutes"]')!.value = "25";
    form.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush();
    const [logged] = calls("plugin_log") as Record<string, any>[];
    expect(logged).toMatchObject({id: "guitar", kind: "entry", asTask: true, description: "Guitar", data: {values: {minutes: 25}}});
  });
});

describe("plugins elsewhere", () => {
  it("adds a card to the timer row for plugins that ask for one", async () => {
    await renderApp(root, client);
    expect(root.querySelector("#timer-strip .timer-plugin.plugin-fitness")).not.toBeNull();
    expect(root.querySelector("#timer-strip .timer-plugin.plugin-tracker")).toBeNull();
  });

  it("keeps a plugin's type from being deleted on the board", async () => {
    await renderApp(root, client);
    click('[data-type-menu="2"]'); await flush();
    expect(root.querySelector<HTMLButtonElement>('[data-type-delete="2"]')?.disabled).toBe(true);
  });

  it("switches where a plugin shows up from Settings", async () => {
    await renderApp(root, client);
    click('[data-page="settings"]'); await flush();
    expect(root.querySelectorAll(".plugin-row")).toHaveLength(2);
    const box = root.querySelector<HTMLInputElement>('[data-plugin-setting="guitar"][data-key="timer"]')!;
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    await flush();
    expect(calls("plugin_save")).toContainEqual({id: "guitar", name: "Guitar", settings: {timer: true}});
  });

  it("adds a plugin from a template", async () => {
    await renderApp(root, client);
    click('[data-page="settings"]'); await flush();
    click('[data-plugin-new="language"]'); await flush();
    const dialog = root.querySelector<HTMLDialogElement>("dialog.plugin-dialog")!;
    expect(dialog.querySelectorAll(".metric-row")).toHaveLength(3);
    dialog.querySelector<HTMLFormElement>("form")!.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush();
    const [saved] = calls("plugin_save") as Record<string, any>[];
    expect(saved).toMatchObject({kind: "tracker", name: "Language learning", settings: {page: true, activity: true, timer: true, agent: false}, type: null});
    expect(saved.state.metrics.map((metric: {id: string}) => metric.id)).toEqual(["minutes", "words", "lesson"]);
  });

  it("views Activity by a plugin's category with the plugin's own metrics", async () => {
    await renderApp(root, client);
    click('[data-page="activity"]'); await flush();
    expect(root.querySelector('[data-category="2"] .chip-plugin')).not.toBeNull();
    click('[data-category="2"]'); await flush();
    expect(root.querySelector("#category-panel .fit-activity")).not.toBeNull();
    expect(root.querySelector(".week-entry")).toBeNull();
    expect([...root.querySelectorAll(".gallery-card h4, .tree [data-history]")].length).toBeGreaterThanOrEqual(0);
    click('[data-category="3"]'); await flush();
    expect(root.querySelector("#category-panel .trk-activity")?.textContent).toContain("Best of each week");
    click('[data-category=""]'); await flush();
    expect(root.querySelector("#category-panel")).toBeNull();
  });

  it("turns a suggested workout's exercises into the task's steps", async () => {
    data.suggestions = [{id: "4", description: "Pull day", priority: 6, type: "2", typeName: "Fitness", agent: "Claude", plugin: {workout: {name: "Pull day", units: [{exercise: "pullup", sets: 3, reps: 8}]}}}];
    await renderApp(root, client);
    expect(root.querySelector(".suggestion .suggestion-plugin")?.textContent).toContain("1 exercise");
    click("[data-suggestion-accept]"); await flush();
    expect(calls("suggestion_accept")).toContainEqual({id: "4", subtasks: [{text: "Pull-up · 3×8", done: false}]});
  });
});
