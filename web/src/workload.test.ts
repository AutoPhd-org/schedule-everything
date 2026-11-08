// Tests for the workload: how it is counted, its levels, and the theme and header chip that follow it.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderApp } from "./render";
import type { BridgeClient, Snapshot, TaskItem } from "./types";
import { describeLoad, levelOf, taskLoad, workload } from "./workload";

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
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); delete document.documentElement.dataset.load; });

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
