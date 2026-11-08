// Tests for the timers above the task board, today's timed-work bar, and task progress.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderApp } from "./render";
import { clock, collectTimers, progressOf, pruneClosed, runway, span, timerKey, todayProgress, urgency } from "./timers";
import type { BridgeClient, DeadlineItem, Snapshot, TaskItem } from "./types";

const task = (description: string, extra: Partial<TaskItem> = {}): TaskItem =>
  ({description, priority: 5, type: "1", typeName: "Coding", alarmFrom: null, procrastinated: false, procrastinateDays: null, ...extra});
const deadline = (event: string, day: string, extra: Partial<DeadlineItem> = {}): DeadlineItem =>
  ({event, deadline: day, daysLeft: 0, status: "today", type: null, typeName: null, ...extra});
/** 3 pm on the snapshot's day. */
const NOW = new Date(2026, 10, 5, 15, 0).getTime();

const snapshot: Snapshot = {
  config: {rootDir: "/local/config", activeId: 0, activeConfigDir: "/local/config/user_config_0", tasksPath: "", deadlinesPath: "", habitsPath: "", recordsPath: ""},
  today: {date: "2026-11-05", weekday: "thursday", parity: "odd"},
  schedule: {isSkipped: false, current: null, next: null, timeToNext: null, events: [], hasSyncedOverlay: false},
  taskTypes: {"1": "Coding", "2": "Writing"},
  tasks: [
    task("Ship slides", {due: "2026-11-05T18:00", progress: 40}),
    task("Fix flaky test", {due: "2026-11-05T13:30", progress: 20, type: "2", typeName: "Writing"}),
    task("Plan trip", {due: "2026-11-06T09:00", progress: 10}),
    task("Read paper", {progress: 60})
  ],
  deadlines: [
    deadline("Camera ready", "2026-11-05"),
    deadline("Grant report", "2026-11-06", {daysLeft: 1, status: "urgent"}),
    deadline("Old form", "2026-11-04", {daysLeft: -1, status: "overdue"})
  ],
  habits: [],
  history: [],
  timedDone: [{description: "Morning review", type: "1", due: "2026-11-05T10:00", endedAt: "2026-11-05T09:50:00"}]
};

describe("which timers run", () => {
  it("times tasks due by tonight, overdue ones first, and deadlines in their last 24 hours", () => {
    const timers = collectTimers(snapshot, NOW, new Set());
    expect(timers.map(timer => timer.kind === "task" ? timer.task.description : timer.deadline.event)).toEqual(["Fix flaky test", "Ship slides", "Camera ready"]);
    expect(timers.map(timer => timer.kind === "task" && timer.overdue)).toEqual([true, false, false]);
    // Tomorrow's task and deadline wait for tomorrow; a passed deadline has no timer.
    const tomorrow = collectTimers(snapshot, new Date(2026, 10, 6, 0, 0, 1).getTime(), new Set());
    expect(tomorrow.map(timer => timer.kind === "task" ? timer.task.description : timer.deadline.event)).toEqual(["Fix flaky test", "Ship slides", "Plan trip", "Grant report"]);
  });

  it("leaves out deadline timers that were closed, until the deadline comes around again", () => {
    const key = timerKey(snapshot.deadlines[0]);
    expect(collectTimers(snapshot, NOW, new Set([key])).some(timer => timer.kind === "deadline")).toBe(false);
    expect(pruneClosed([key, timerKey(snapshot.deadlines[2]), "not json"], "2026-11-05")).toEqual([key]);
  });

  it("measures today's timed work with finished tasks at 100%", () => {
    const today = todayProgress(snapshot, NOW);
    // Morning review (done, 100), Fix flaky test (20), Ship slides (40); Plan trip is due tomorrow.
    expect(today.parts.map(part => [part.description, part.progress])).toEqual([["Morning review", 100], ["Fix flaky test", 20], ["Ship slides", 40]]);
    expect(Math.round(today.value * 100)).toBe(53);
    expect(today).toMatchObject({done: 1, overdue: 1});
    // Finished work from another day does not count today.
    expect(todayProgress({...snapshot, timedDone: [{...snapshot.timedDone![0], endedAt: "2026-11-04T09:50:00"}]}, NOW).done).toBe(0);
  });

  it("weighs today's timed work by importance when asked", () => {
    const weighted = structuredClone(snapshot);
    weighted.tasks[0].priority = 9;
    weighted.tasks[1].priority = 3;
    weighted.timedDone![0].priority = 6;
    // Every task the same: (100 + 20 + 40) / 3. By importance: (6 × 100 + 3 × 20 + 9 × 40) / (18 × 100).
    expect(Math.round(todayProgress(weighted, NOW).value * 100)).toBe(53);
    expect(todayProgress(weighted, NOW, true)).toMatchObject({weighted: true, done: 1, overdue: 1});
    expect(Math.round(todayProgress(weighted, NOW, true).value * 100)).toBe(57);
    // A task finished before its importance was recorded counts as 5.
    delete weighted.timedDone![0].priority;
    expect(todayProgress(weighted, NOW, true).parts[0].priority).toBe(5);
  });

  it("tells how close a timer is and how much of its runway is left", () => {
    const HOUR = 3_600_000;
    expect(urgency("task", NOW + 4 * HOUR, NOW)).toBe("calm");
    expect(urgency("task", NOW + 2 * HOUR, NOW)).toBe("soon");
    expect(urgency("task", NOW + 30 * 60_000, NOW)).toBe("close");
    expect(urgency("task", NOW, NOW)).toBe("overdue");
    expect(urgency("task", NOW + 2 * HOUR, NOW, true)).toBe("ready");
    expect(urgency("task", NOW - 1, NOW, true)).toBe("overdue");
    expect(urgency("deadline", NOW + 5 * HOUR, NOW)).toBe("soon");
    expect(urgency("deadline", NOW + 7 * HOUR, NOW)).toBe("calm");
    // A task's runway is its day up to its finish time; a deadline's is its last 24 hours.
    expect(runway("task", new Date(2026, 10, 5, 18, 0).getTime(), NOW)).toBeCloseTo(3 / 18);
    expect(runway("deadline", NOW + 6 * HOUR, NOW)).toBe(0.25);
    expect(runway("task", NOW - HOUR, NOW)).toBe(0);
  });

  it("measures progress by subtasks while a task has them", () => {
    expect(progressOf({progress: 30})).toBe(30);
    expect(progressOf({progress: 30, subtasks: [{text: "a", done: true}, {text: "b", done: false}, {text: "c", done: false}]})).toBe(33);
    expect(progressOf({subtasks: Array.from({length: 8}, (_, i) => ({text: String(i), done: i === 0}))})).toBe(13);
    expect(progressOf({progress: 30, subtasks: []})).toBe(30);
  });

  it("formats running clocks", () => {
    expect(clock(2 * 3600_000 + 14 * 60_000 + 5_000)).toBe("02:14:05");
    expect(clock(-(90 * 60_000))).toBe("01:30:00");
    expect(clock(26 * 3600_000)).toBe("1d 2h");
    expect(span(3 * 3600_000 + 5 * 60_000)).toBe("3h 05m");
    expect(span(45 * 60_000)).toBe("45m");
    expect(span(20_000)).toBe("<1m");
  });
});

let root: HTMLElement;
let send: ReturnType<typeof vi.fn>;
let client: BridgeClient;
let data: Snapshot;
const flush = () => new Promise(resolve => setTimeout(resolve, 0));
const timers = () => [...root.querySelectorAll<HTMLElement>("#timer-strip .timer")];
const titles = () => timers().map(timer => timer.querySelector("h4")?.textContent);

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector("#app")!;
  window.localStorage.clear();
  vi.stubGlobal("matchMedia", vi.fn((query: string) => ({matches: query.includes("reduced-motion"), addEventListener: vi.fn()})));
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
  vi.useFakeTimers({toFake: ["Date"]});
  vi.setSystemTime(NOW);
  data = structuredClone(snapshot);
  send = vi.fn(async (command: string, payload: Record<string, unknown>) => {
    if (command === "status_snapshot") return structuredClone(data);
    if (command === "workspace_info") return {configIds: [0], activeId: 0, mode: "j", rootDir: "/local/config"};
    if (command === "task_progress") data.tasks.find(item => item.description === payload.description)!.progress = Number(payload.progress);
    if (command === "task_update") {
      const item = data.tasks.find(entry => entry.description === payload.originalDescription)!;
      item.due = payload.due ? String(payload.due) : null;
    }
    return {};
  });
  client = {send};
});
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("timer strip", () => {
  it("runs above the board with task and deadline timers told apart", async () => {
    await renderApp(root, client);
    const strip = root.querySelector("#timer-strip")!;
    expect(strip.compareDocumentPosition(root.querySelector("#board-area")!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(titles()).toEqual(["Fix flaky test", "Ship slides", "Camera ready"]);
    const [overdue, running, ending] = timers();
    expect(overdue.classList.contains("timer-task")).toBe(true);
    expect(overdue.classList.contains("is-overdue")).toBe(true);
    expect(overdue.querySelector(".timer-clock")?.textContent).toContain("01:30:00overdue · was due");
    expect(running.classList.contains("is-overdue")).toBe(false);
    expect(running.querySelector("[data-clock='clock']")?.textContent).toBe("03:00:00");
    expect(running.querySelector("[data-timer-close]")).toBeNull();
    expect(ending.classList.contains("timer-deadline")).toBe(true);
    expect(ending.querySelector("[data-clock='clock']")?.textContent).toBe("09:00:00");
    // Only deadline timers can be closed; only task timers carry progress.
    expect(ending.querySelector("[data-timer-close]")).not.toBeNull();
    expect(ending.querySelector("input[data-progress]")).toBeNull();
    // Today's bar: three timed tasks due today, one finished.
    const bar = strip.querySelector(".today-bar")!;
    expect(bar.getAttribute("aria-valuenow")).toBe("53");
    expect(bar.querySelectorAll("i")).toHaveLength(3);
    expect(strip.querySelector(".today-meta")?.textContent).toBe("1 of 3 done · 1 overdue");
    // The overdue task's card on the board looks different too.
    const card = [...root.querySelectorAll(".task-card")].find(el => el.textContent?.includes("Fix flaky test"))!;
    expect(card.classList.contains("is-overdue")).toBe(true);
    expect(card.querySelector(".task-timer")?.textContent).toContain("Was due");
  });

  it("closes a deadline timer without touching the deadline, and can bring it back", async () => {
    await renderApp(root, client);
    root.querySelector<HTMLButtonElement>("[data-timer-close]")!.click();
    expect(titles()).toEqual(["Fix flaky test", "Ship slides"]);
    expect(send).not.toHaveBeenCalledWith("deadline_delete", expect.anything());
    expect(send).not.toHaveBeenCalledWith("deadline_update", expect.anything());
    // Still listed with the deadlines.
    expect(root.querySelector(".deadline-radar")?.textContent).toContain("Camera ready");
    expect(JSON.parse(window.localStorage.getItem("schedule-everything-closed-timers")!)).toEqual([timerKey(snapshot.deadlines[0])]);
    expect(root.querySelector("#notice .notice-text")?.textContent).toContain("The deadline itself is unchanged.");

    // Stays closed after a reload.
    await renderApp(root, client);
    expect(titles()).toEqual(["Fix flaky test", "Ship slides"]);
  });

  it("undoes closing a deadline timer from the notice", async () => {
    await renderApp(root, client);
    root.querySelector<HTMLButtonElement>("[data-timer-close]")!.click();
    root.querySelector<HTMLButtonElement>("[data-notice-undo]")!.click();
    await flush();
    expect(titles()).toContain("Camera ready");
    expect(JSON.parse(window.localStorage.getItem("schedule-everything-closed-timers")!)).toEqual([]);
  });

  it("sets progress from a card or a timer, and today's bar follows", async () => {
    await renderApp(root, client);
    const slider = [...root.querySelectorAll<HTMLInputElement>("#board-area input[data-progress]")].find(input => input.getAttribute("aria-label") === "Progress of Ship slides")!;
    slider.value = "100";
    slider.dispatchEvent(new Event("input"));
    // Every copy of the slider shows the new value right away.
    expect(root.querySelector<HTMLInputElement>(`#timer-strip input[data-progress="${slider.dataset.progress}"]`)!.value).toBe("100");
    slider.dispatchEvent(new Event("change"));
    expect(root.querySelector(".today-bar")?.getAttribute("aria-valuenow")).toBe("73");
    await new Promise(resolve => setTimeout(resolve, 300));
    expect(send).toHaveBeenCalledWith("task_progress", {description: "Ship slides", progress: 100});
    expect(send.mock.calls.filter(([command]) => command === "task_progress")).toHaveLength(1);
  });

  it("makes a task timed from its card, and stops timing it", async () => {
    await renderApp(root, client);
    const card = (name: string) => [...root.querySelectorAll<HTMLElement>(".task-card")].find(el => el.querySelector("h4")?.textContent === name)!;
    card("Read paper").querySelector<HTMLButtonElement>('[data-operation="timer"]')!.click();
    expect(root.querySelector<HTMLInputElement>("dialog [name='timed']")!.checked).toBe(true);
    expect(root.querySelector<HTMLInputElement>("dialog [name='due']")!.disabled).toBe(false);
    root.querySelector<HTMLInputElement>("dialog [name='due']")!.value = "2026-11-05T21:00";
    root.querySelector("dialog form")!.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_update", expect.objectContaining({originalDescription: "Read paper", due: "2026-11-05T21:00"}));
    expect(titles()).toContain("Read paper");

    card("Ship slides").querySelector<HTMLButtonElement>('[data-operation="untime"]')!.click();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_update", {originalDescription: "Ship slides", description: "Ship slides", priority: 5, type: "1", due: ""});
    expect(titles()).not.toContain("Ship slides");
    expect(card("Ship slides").classList.contains("is-timed")).toBe(false);
  });

  it("gives every timer a dial for how close it is", async () => {
    await renderApp(root, client);
    const [overdue, running, ending] = timers();
    expect(overdue.classList.contains("urgency-overdue")).toBe(true);
    expect(overdue.querySelector(".timer-flag")?.textContent).toBe("Overdue");
    expect(overdue.querySelector("[data-dial]")?.getAttribute("stroke-dasharray")).toBe("0.00 100");
    // Three hours before 6 pm: an hourglass, with 3 of the day's 18 hours to go.
    expect(running.classList.contains("urgency-soon")).toBe(true);
    expect(running.querySelector(".timer-flag")).toBeNull();
    expect(running.querySelector("[data-dial]")?.getAttribute("stroke-dasharray")).toBe("16.67 100");
    // The deadline ends at midnight: 9 of its last 24 hours left, and a flag.
    expect(ending.classList.contains("urgency-calm")).toBe(true);
    expect(ending.querySelector("[data-dial]")?.getAttribute("stroke-dasharray")).toBe("37.50 100");
    expect(root.querySelector(".today-meta")?.querySelectorAll("svg")).toHaveLength(2);
  });

  it("moves a timer on to its next stage as time passes", async () => {
    data.tasks[2].due = "2026-11-05T20:00";
    await renderApp(root, client);
    const find = (name: string) => timers().find(timer => timer.querySelector("h4")?.textContent === name)!;
    expect(find("Plan trip").classList.contains("urgency-calm")).toBe(true);
    expect(find("Plan trip").querySelector(".live-clock")).not.toBeNull();
    vi.setSystemTime(new Date(2026, 10, 5, 17, 30));
    await new Promise(resolve => setTimeout(resolve, 1100));
    expect(find("Ship slides").classList.contains("urgency-close")).toBe(true);
    expect(find("Ship slides").querySelector(".timer-flag")?.textContent).toBe("Last hour");
    expect(find("Plan trip").classList.contains("urgency-soon")).toBe(true);
  });

  it("offers to complete a task whose work is all done", async () => {
    data.tasks[0].progress = 100;
    await renderApp(root, client);
    const ready = timers().find(timer => timer.querySelector("h4")?.textContent === "Ship slides")!;
    expect(ready.classList.contains("urgency-ready")).toBe(true);
    expect(ready.querySelector(".timer-flag")?.textContent).toBe("All done");
    // Overdue work stays overdue, done or not.
    expect(timers()[0].querySelector(".timer-complete")).toBeNull();
    ready.querySelector<HTMLButtonElement>(".timer-complete")!.click();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_delete", {description: "Ship slides", action: "deleted"});
  });

  it("weighs today's bar by importance once that is turned on in Settings", async () => {
    data.tasks[0].priority = 9;
    data.tasks[1].priority = 3;
    data.timedDone![0].priority = 6;
    await renderApp(root, client);
    expect(root.querySelector(".today-bar")?.getAttribute("aria-valuenow")).toBe("53");
    expect(root.querySelector(".today-weighted")).toBeNull();
    root.querySelector<HTMLButtonElement>('[data-page="settings"]')!.click();
    const toggle = root.querySelector<HTMLInputElement>('[data-pref="weighted"]')!;
    expect(toggle.checked).toBe(false);
    toggle.checked = true; toggle.dispatchEvent(new Event("change"));
    expect(root.querySelector("#notice .notice-text")?.textContent).toBe("Today’s progress now weighs each task by its importance.");
    expect(JSON.parse(window.localStorage.getItem("schedule-everything-display")!).weighted).toBe(true);
    root.querySelector<HTMLButtonElement>('[data-page="today"]')!.click();
    const bar = root.querySelector(".today-bar")!;
    expect(bar.getAttribute("aria-valuenow")).toBe("57");
    expect(bar.getAttribute("aria-valuetext")).toContain("weighted by importance");
    expect([...bar.querySelectorAll<HTMLElement>("i")].map(segment => segment.style.flexGrow)).toEqual(["6", "3", "9"]);
    expect(root.querySelector(".today-weighted")?.textContent).toBe("By importance");
  });

  it("explains itself when nothing is timed", async () => {
    data.tasks = [task("Read paper")];
    data.deadlines = [];
    data.timedDone = [];
    await renderApp(root, client);
    expect(root.querySelector(".timer-empty")?.textContent).toContain("No timers right now.");
    expect(root.querySelector(".today-meta")?.textContent).toBe("No timed tasks due today");
  });
});
