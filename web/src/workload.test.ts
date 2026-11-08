// Tests for the workload: how it is counted, its levels, and the theme and header chip that follow it.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderApp } from "./render";
import type { BridgeClient, Snapshot, TaskItem } from "./types";
import { asThresholds, DEFAULT_THRESHOLDS, describeLoad, levelOf, levelRanges, loadScale, setLoadThresholds, taskLoad, thresholdProblem, workload } from "./workload";

const task = (description: string, priority: number, extra: Partial<TaskItem> = {}): TaskItem =>
  ({description, priority, type: "1", typeName: "Coding", alarmFrom: null, procrastinated: false, procrastinateDays: null, ...extra});

describe("workload", () => {
  it("counts each open task by its importance and the share of it still left", () => {
    const tasks = [
      task("Untouched", 8),
      task("Half done", 6, {progress: 50}),
      task("Finished", 10, {progress: 100}),
      task("Postponed", 9, {alarmFrom: "2026-11-10"}),
      task("A quarter checked off", 4, {subtasks: [{text: "a", done: true}, {text: "b", done: false}, {text: "c", done: false}, {text: "d", done: false}]})
    ];
    // 0.8 + 0.3 + 0 + 0.4 × 0.75; the postponed task waits for its day.
    expect(workload(tasks, "2026-11-07")).toEqual({points: 1.4, level: "light", counted: 4, importance: 7, done: 44});
    expect(taskLoad(tasks[3], "2026-11-07")).toBe(0);
    expect(taskLoad(tasks[3], "2026-11-10")).toBe(0.9);
    expect(describeLoad(workload(tasks, "2026-11-07"))).toBe("Workload 1.4 · Light. 4 open tasks, importance 7 on average, 44% done.");
    expect(workload([], "2026-11-07")).toEqual({points: 0, level: "clear", counted: 0, importance: null, done: 0});
  });

  it("names five levels, judged on the number as shown", () => {
    expect([0, 0.04, 0.1, 2.4, 2.5, 5.44, 5.5, 8.9, 9, 20].map(levelOf)).toEqual(["clear", "clear", "light", "light", "steady", "steady", "busy", "busy", "heavy", "heavy"]);
    // 5.49 shows as 5.5, so it is busy.
    expect(levelOf(5.49)).toBe("busy");
  });

  it("starts the levels where they are set to", () => {
    setLoadThresholds([1, 4, 12]);
    expect([0, 0.9, 1, 3.9, 4, 11.9, 12].map(levelOf)).toEqual(["clear", "light", "steady", "steady", "busy", "busy", "heavy"]);
    expect(levelRanges()).toEqual(["0", "under 1", "1 – 4", "4 – 12", "12 and up"]);
    // Gauges top out a third past the start of Heavy.
    expect(loadScale()).toBe(16);
    setLoadThresholds(DEFAULT_THRESHOLDS);
    expect(levelRanges()).toEqual(["0", "under 2.5", "2.5 – 5.5", "5.5 – 9", "9 and up"]);
    expect(loadScale()).toBe(12);
  });

  it("only takes numbers that can start the levels", () => {
    expect(thresholdProblem([2, 4, 8])).toBeNull();
    expect(thresholdProblem([2, Number.NaN, 8])).toBe("Enter a number for each level.");
    expect(thresholdProblem([0, 4, 8])).toBe("Each level needs to start above 0.");
    expect(thresholdProblem([2, 4, 101])).toBe("Keep each number at 100 or below.");
    expect(thresholdProblem([2, 8, 4])).toBe("Each level needs to start higher than the one before it.");
    // Compared as shown, to one decimal: 4.01 and 4 are the same number.
    expect(thresholdProblem([2, 4, 4.01])).toBe("Each level needs to start higher than the one before it.");
    expect(asThresholds([1.26, 4, 8])).toEqual([1.3, 4, 8]);
    expect(asThresholds([4, 2, 8])).toBeNull();
    expect(asThresholds("2,4,8")).toBeNull();
    // Numbers that cannot start the levels leave the defaults in place.
    setLoadThresholds([5, 5, 5]);
    expect(levelRanges()).toEqual(["0", "under 2.5", "2.5 – 5.5", "5.5 – 9", "9 and up"]);
  });
});

let root: HTMLElement;
let send: ReturnType<typeof vi.fn>;
let client: BridgeClient;
let data: Snapshot;
const flush = () => new Promise(resolve => setTimeout(resolve, 0));
const click = (selector: string) => root.querySelector<HTMLElement>(selector)!.click();

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector("#app")!;
  window.localStorage.clear();
  vi.stubGlobal("matchMedia", vi.fn((query: string) => ({matches: query.includes("reduced-motion"), addEventListener: vi.fn()})));
  data = {
    config: {rootDir: "/local/config", activeId: 0, activeConfigDir: "/local/config/user_config_0", tasksPath: "", deadlinesPath: "", habitsPath: "", recordsPath: ""},
    today: {date: "2026-11-07", weekday: "saturday", parity: "odd"},
    schedule: {isSkipped: false, current: null, next: null, timeToNext: null, events: [], hasSyncedOverlay: false},
    taskTypes: {"1": "Coding"},
    tasks: Array.from({length: 12}, (_, i) => task(`Task ${i}`, 9)),
    deadlines: [], habits: [], history: []
  };
  send = vi.fn(async (command: string) => {
    if (command === "status_snapshot") return structuredClone(data);
    if (command === "workspace_info") return {configIds: [0], activeId: 0, mode: "p", rootDir: "/local/config"};
    return {};
  });
  client = {send};
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers(); delete document.documentElement.dataset.load; document.documentElement.style.removeProperty("--day-progress"); setLoadThresholds(DEFAULT_THRESHOLDS); });

describe("the workload theme", () => {
  it("tints the workspace by workload, shows it in the header, and can be turned off", async () => {
    await renderApp(root, client);
    expect(document.documentElement.dataset.load).toBe("heavy");
    const chip = root.querySelector<HTMLElement>(".load-chip")!;
    expect(chip.classList.contains("load-heavy")).toBe(true);
    expect(chip.textContent).toBe("Heavy10.8");
    expect(chip.dataset.tip).toContain("Workload 10.8 · Heavy. 12 open tasks, importance 9 on average, 0% done.");
    click('[data-page="settings"]');
    // Settings explain the levels and mark the current one.
    expect(root.querySelector(".load-step.is-now")?.textContent).toBe("Heavy9 and up");
    const toggle = root.querySelector<HTMLInputElement>('[data-pref="workloadTheme"]')!;
    expect(toggle.checked).toBe(true);
    toggle.checked = false; toggle.dispatchEvent(new Event("change"));
    expect(document.documentElement.dataset.load).toBeUndefined();
    expect(JSON.parse(window.localStorage.getItem("schedule-everything-display")!).workloadTheme).toBe(false);
    await renderApp(root, client);
    expect(document.documentElement.dataset.load).toBeUndefined();
    // The chip stays, so the workload is always one glance away.
    expect(root.querySelector(".load-chip")?.textContent).toBe("Heavy10.8");
  });

  it("eases toward calm as today's progress bar fills, while the workload keeps the level", async () => {
    vi.useFakeTimers({toFake: ["Date"]});
    vi.setSystemTime(new Date(2026, 10, 7, 12, 0));
    const pull = () => document.documentElement.style.getPropertyValue("--day-progress");
    // Without timed work due today there is no bar, so only the workload counts.
    await renderApp(root, client);
    expect(document.documentElement.dataset.load).toBe("heavy");
    expect(pull()).toBe("0.000");
    // Two timed tasks due tonight, one finished today and one half done: the bar is at 75%.
    data.timedDone = [{description: "Morning review", type: "1", due: "2026-11-07T10:00", endedAt: "2026-11-07T09:50:00", priority: 9}];
    data.tasks = [...data.tasks, task("Ship slides", 3, {due: "2026-11-07T18:00", progress: 50})];
    await renderApp(root, client);
    expect(document.documentElement.dataset.load).toBe("heavy");
    expect(pull()).toBe("0.750");
    // Weighing by importance weighs the bar, and the theme with it: (9 × 100 + 3 × 50) / (12 × 100).
    click('[data-page="settings"]');
    const weigh = root.querySelector<HTMLInputElement>('[data-pref="weighted"]')!;
    weigh.checked = true; weigh.dispatchEvent(new Event("change"));
    expect(pull()).toBe("0.875");
    // Moving a slider moves the theme with the bar.
    click('[data-page="today"]');
    await flush();
    const slider = root.querySelector<HTMLInputElement>('#timer-strip input[data-progress="12"]')!;
    slider.value = "100"; slider.dispatchEvent(new Event("change"));
    expect(pull()).toBe("1.000");
    expect(document.documentElement.dataset.load).toBe("heavy");
    // With the theme off, neither the workload nor progress tints the page.
    click('[data-page="settings"]');
    const toggle = root.querySelector<HTMLInputElement>('[data-pref="workloadTheme"]')!;
    toggle.checked = false; toggle.dispatchEvent(new Event("change"));
    expect(document.documentElement.dataset.load).toBeUndefined();
    expect(pull()).toBe("");
    vi.useRealTimers();
  });

  it("calms down as work gets done", async () => {
    data.tasks = [task("Ship it", 10), task("Review", 10), task("Write it up", 6, {subtasks: [{text: "Draft", done: false}]})];
    await renderApp(root, client);
    expect(document.documentElement.dataset.load).toBe("steady");
    const box = root.querySelector<HTMLInputElement>("input[data-subtask]")!;
    box.checked = true; box.dispatchEvent(new Event("change"));
    await flush();
    expect(document.documentElement.dataset.load).toBe("light");
    expect(root.querySelector(".load-chip")?.textContent).toBe("Light2.0");
  });
});

describe("where the workload levels start", () => {
  const fields = () => [...root.querySelectorAll<HTMLInputElement>("[data-threshold]")];
  /** Type a number into a level's field, then enter it. */
  const enter = (i: number, value: string) => {
    const field = fields()[i];
    field.value = value;
    field.dispatchEvent(new Event("input"));
    field.dispatchEvent(new Event("change"));
  };
  const ranges = () => [...root.querySelectorAll(".load-step small")].map(el => el.textContent);
  const saved = () => JSON.parse(window.localStorage.getItem("schedule-everything-display") ?? "{}").loadThresholds;

  it("is set in Settings, and the theme, the header, and the scale follow", async () => {
    await renderApp(root, client);
    click('[data-page="settings"]');
    expect(fields().map(field => field.value)).toEqual(["2.5", "5.5", "9"]);
    expect(root.querySelector<HTMLButtonElement>("[data-thresholds-reset]")!.disabled).toBe(true);
    // 12 tasks of importance 9 make 10.8: heavy until Heavy starts at 12.
    enter(2, "12");
    expect(document.documentElement.dataset.load).toBe("busy");
    expect(root.querySelector(".load-chip")?.textContent).toBe("Busy10.8");
    expect(ranges()).toEqual(["0", "under 2.5", "2.5 – 5.5", "5.5 – 12", "12 and up"]);
    expect(root.querySelector(".load-step.is-now")?.textContent).toBe("Busy5.5 – 12");
    expect(root.querySelector(".load-now")?.textContent).toContain("Right now it is 10.8: busy.");
    expect(saved()).toEqual([2.5, 5.5, 12]);
    expect(root.querySelector(".notice-text")?.textContent).toBe("Workload levels updated: Steady from 2.5, Busy from 5.5, Heavy from 12.");
    expect(root.querySelector<HTMLButtonElement>("[data-thresholds-reset]")!.disabled).toBe(false);
    // Numbers are kept to one decimal, as workloads are shown.
    enter(0, "3.04");
    expect(fields()[0].value).toBe("3");
    expect(saved()).toEqual([3, 5.5, 12]);
    // A new page keeps them.
    await renderApp(root, client);
    expect(document.documentElement.dataset.load).toBe("busy");
    click('[data-page="settings"]');
    expect(fields().map(field => field.value)).toEqual(["3", "5.5", "12"]);
  });

  it("says what is wrong instead of saving numbers that cannot start the levels", async () => {
    await renderApp(root, client);
    click('[data-page="settings"]');
    const problem = root.querySelector<HTMLElement>(".threshold-problem")!;
    expect(problem.hidden).toBe(true);
    enter(1, "10");
    expect(problem.hidden).toBe(false);
    expect(problem.textContent).toBe("Each level needs to start higher than the one before it.");
    expect(fields().map(field => field.getAttribute("aria-invalid"))).toEqual(["false", "false", "true"]);
    expect(saved()).toBeUndefined();
    expect(document.documentElement.dataset.load).toBe("heavy");
    enter(1, "");
    expect(problem.textContent).toBe("Enter a number for each level.");
    expect(fields()[1].getAttribute("aria-invalid")).toBe("true");
    // Fixing it saves all three.
    enter(1, "6");
    expect(problem.hidden).toBe(true);
    expect(fields().map(field => field.getAttribute("aria-invalid"))).toEqual(["false", "false", "false"]);
    expect(saved()).toEqual([2.5, 6, 9]);
  });

  it("goes back to the defaults", async () => {
    window.localStorage.setItem("schedule-everything-display", JSON.stringify({loadThresholds: [1, 2, 20]}));
    await renderApp(root, client);
    expect(document.documentElement.dataset.load).toBe("busy");
    click('[data-page="settings"]');
    expect(ranges()).toEqual(["0", "under 1", "1 – 2", "2 – 20", "20 and up"]);
    click("[data-thresholds-reset]");
    expect(fields().map(field => field.value)).toEqual(["2.5", "5.5", "9"]);
    expect(saved()).toEqual([2.5, 5.5, 9]);
    expect(document.documentElement.dataset.load).toBe("heavy");
    expect(root.querySelector(".notice-text")?.textContent).toBe("Workload levels are back to the defaults: Steady from 2.5, Busy from 5.5, Heavy from 9.");
    expect(root.querySelector<HTMLButtonElement>("[data-thresholds-reset]")!.disabled).toBe(true);
    expect(document.activeElement).toBe(fields()[0]);
  });

  it("ignores saved numbers that cannot start the levels", async () => {
    window.localStorage.setItem("schedule-everything-display", JSON.stringify({loadThresholds: [5, 3, 1]}));
    await renderApp(root, client);
    expect(document.documentElement.dataset.load).toBe("heavy");
    click('[data-page="settings"]');
    expect(fields().map(field => field.value)).toEqual(["2.5", "5.5", "9"]);
  });
});
