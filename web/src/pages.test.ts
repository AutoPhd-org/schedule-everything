// Tests for P/J overview layouts, the activity gallery and weekly overview, and the year calendar.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { filterHistory, groupHistory, defaultGalleryFilter } from "./activity";
import { deadlinesByDay, deadlineSwatch, monthCells, nextOccurrence } from "./calendar";
import { renderApp } from "./render";
import type { BridgeClient, HistoryItem, Snapshot, WeekDay, WeeklyOverview, WeekStats } from "./types";
import { addDays } from "./ui";

const snapshot: Snapshot = {
  config: {rootDir: "/local/config", activeId: 0, activeConfigDir: "/local/config/user_config_0", tasksPath: "", deadlinesPath: "", habitsPath: "", recordsPath: ""},
  today: {date: "2026-10-05", weekday: "monday", parity: "even"},
  schedule: {isSkipped: false, current: "Write proposal", next: "Break", timeToNext: "15m", events: [{time: "09:00", label: "Write proposal", block: "pomodoro", syncable: true}], hasSyncedOverlay: false},
  taskTypes: {"1": "Coding", "2": "Writing", "3": "Reading"},
  tasks: [
    {description: "Write notes", priority: 10, type: "2", typeName: "Writing", alarmFrom: null, procrastinated: false, procrastinateDays: null},
    {description: "Fix build", priority: 6, type: "1", typeName: "Coding", alarmFrom: null, procrastinated: false, procrastinateDays: null}
  ],
  deadlines: [
    {event: "Conference", deadline: "2026-10-06", daysLeft: 1, status: "urgent", type: "2", typeName: "Writing"},
    {event: "Rent", deadline: "2026-10-30", daysLeft: 25, status: "ok", type: null, typeName: null, color: "red"}
  ],
  habits: [{id: "1", description: "Exercise", completed: false}],
  history: []
};

const history: HistoryItem[] = [
  {description: "Ship release", priority: 9, startedAt: "2026-10-03T09:00:00", endedAt: "2026-10-05T10:00:00", duration: "2d 1h", status: "completed", type: "1", typeName: "Coding"},
  {description: "Draft essay", priority: 5, startedAt: "2026-09-20T09:00:00", endedAt: "2026-09-22T10:00:00", duration: "2d 1h", status: "completed", type: "2", typeName: "Writing"},
  {description: "Wrong task", priority: 2, startedAt: "2026-10-01T09:00:00", endedAt: "2026-10-01T09:30:00", duration: "30m", status: "cancelled", type: "1", typeName: "Coding"},
  {description: "<img src=x onerror=alert(1)>", priority: 4, startedAt: "2026-08-01T09:00:00", endedAt: "2026-09-01T11:00:00", duration: "31d 2h", status: "dropped", type: "3", typeName: "Reading"}
];

const week = (weekStart: string, completed: number, extra: Partial<WeekStats> = {}): WeekStats => ({weekStart, active: true, completed, cancelled: 0, dropped: 0, added: 2, habitRate: 0.5, avgPriority: 6, medianLeadHours: 12, byType: {"1": completed}, ...extra});
const days = (start: string, counts: number[], lastPast = 6): WeekDay[] => counts.map((completed, i) => ({date: addDays(start, i), completed, byType: (completed ? {"1": completed} : {}) as Record<string, number>, habitsDone: 1, habitsTotal: 1, future: i > lastPast}));
const overview: WeeklyOverview = {
  weekStart: "2026-10-05", weekEnd: "2026-10-11", today: "2026-10-05", isCurrentWeek: true, firstActivity: "2026-08-01", taskTypes: snapshot.taskTypes,
  current: week("2026-10-05", 3, {cancelled: 1, added: 5}), previous: week("2026-09-28", 1),
  baseline: {weeks: 4, completed: 2, cancelled: 0, dropped: 0, added: 2, habitRate: 0.4, avgPriority: 5, medianLeadHours: 20, byType: {"1": 2}},
  trend: ["2026-08-17", "2026-08-24", "2026-08-31", "2026-09-07", "2026-09-14", "2026-09-21", "2026-09-28", "2026-10-05"].map((start, i) => week(start, i % 4)),
  days: days("2026-10-05", [3, 0, 0, 0, 0, 0, 0], 0), previousDays: days("2026-09-28", [1, 0, 0, 0, 0, 0, 0]),
  habits: [{id: "1", description: "Exercise", days: [true, null, null, null, null, null, null], rate: 1, previousRate: 0.5}],
  streak: 2,
  highlights: [{description: "Ship release", priority: 9, type: "1", endedAt: "2026-10-05T10:00:00"}]
};

let root: HTMLElement;
let send: ReturnType<typeof vi.fn>;
let client: BridgeClient;
let mode = "j";
const flush = () => new Promise(resolve => setTimeout(resolve, 0));
const click = (selector: string) => root.querySelector<HTMLElement>(selector)!.click();

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector("#app")!;
  mode = "j";
  vi.stubGlobal("matchMedia", vi.fn(() => ({matches: false, addEventListener: vi.fn()})));
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
  send = vi.fn(async (command: string) => {
    if (command === "status_snapshot") return structuredClone(snapshot);
    if (command === "workspace_info") return {configIds: [0], activeId: 0, mode, rootDir: "/local/config"};
    if (command === "task_history") return {activities: structuredClone(history)};
    if (command === "weekly_overview") return structuredClone(overview);
    return {};
  });
  client = {send};
  vi.spyOn(window, "confirm").mockReturnValue(true);
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); delete document.documentElement.dataset.mode; });

describe("planning modes", () => {
  it("shows the full structured overview in J mode", async () => {
    await renderApp(root, client);
    expect(document.documentElement.dataset.mode).toBe("j");
    for (const selector of [".status-grid", ".focus-card", ".deadline-radar", ".timeline", "#task-board"]) expect(root.querySelector(selector)).not.toBeNull();
    expect(root.querySelector('[data-mode-choice="j"]')?.getAttribute("aria-pressed")).toBe("true");
  });

  it("keeps only the tasks-by-type board in P mode", async () => {
    mode = "p";
    await renderApp(root, client);
    expect(document.documentElement.dataset.mode).toBe("p");
    for (const selector of [".status-grid", ".focus-card", ".deadline-radar", ".timeline"]) expect(root.querySelector(selector)).toBeNull();
    expect(root.querySelector("h1")?.textContent).toContain("Tasks by type");
    expect(root.querySelector("#task-board.flow-board")).not.toBeNull();
    expect(root.querySelectorAll(".task-card")).toHaveLength(2);
    const search = root.querySelector<HTMLInputElement>('[name="search"]')!;
    search.value = "build"; search.dispatchEvent(new Event("input"));
    expect(root.querySelectorAll(".task-card")).toHaveLength(1);
  });

  it("switches modes from the header and redraws the matching layout", async () => {
    await renderApp(root, client);
    mode = "p";
    click('[data-mode-choice="p"]'); await flush(); await flush();
    expect(send).toHaveBeenCalledWith("service_action", {action: "mode", mode: "p"});
    expect(root.querySelector(".status-grid")).toBeNull();
    expect(root.querySelector('[data-mode-choice="p"]')?.getAttribute("aria-pressed")).toBe("true");
  });
});

describe("activity gallery", () => {
  async function openActivity() {
    await renderApp(root, client);
    click('[data-page="activity"]');
    await flush(); await flush();
  }

  it("loads full history and the current week, and renders an escaped gallery", async () => {
    await openActivity();
    expect(send).toHaveBeenCalledWith("task_history", {all: true});
    expect(send).toHaveBeenCalledWith("weekly_overview", {});
    expect(root.querySelectorAll(".gallery-card")).toHaveLength(4);
    expect(root.querySelector(".gallery img")).toBeNull();
    expect(root.textContent).toContain("<img src=x onerror=alert(1)>");
    expect(root.querySelector(".week-entry .we-big")?.textContent).toContain("3");
    expect(root.querySelector(".gallery-card.status-cancelled.size-s")).not.toBeNull();
    expect(root.querySelector(".gallery-card.status-completed.size-l")?.textContent).toContain("Ship release");
  });

  it("filters by outcome, type, day, and search", async () => {
    await openActivity();
    click('[data-filter-status="cancelled"]');
    expect([...root.querySelectorAll(".gallery-card h4")].map(el => el.textContent)).toEqual(["Wrong task"]);
    click('[data-filter-status="all"]');
    click('[data-filter-type="2"]');
    expect([...root.querySelectorAll(".gallery-card h4")].map(el => el.textContent)).toEqual(["Draft essay"]);
    click('[data-filter-type="2"]');
    click('[data-heat-day="2026-10-05"]');
    expect([...root.querySelectorAll(".gallery-card h4")].map(el => el.textContent)).toEqual(["Ship release"]);
    click('[data-action="clear-day"]'); await flush();
    const search = root.querySelector<HTMLInputElement>('[name="gallerySearch"]')!;
    search.value = "essay"; search.dispatchEvent(new Event("input"));
    expect(root.querySelectorAll(".gallery-card")).toHaveLength(1);
    expect(root.querySelector('[name="gallerySearch"]')).toBe(search);
  });

  it("opens a lightbox and steps through the visible items", async () => {
    await openActivity();
    click('[data-history="0"]');
    const dialog = root.querySelector<HTMLDialogElement>("dialog.lightbox")!;
    expect(dialog.hasAttribute("open")).toBe(true);
    expect(dialog.querySelector("h2")?.textContent).toBe("Ship release");
    expect(dialog.textContent).toContain("1 of 4");
    dialog.querySelector<HTMLButtonElement>('[data-lightbox="next"]')!.click();
    expect(dialog.querySelector("h2")?.textContent).toBe("Wrong task");
    dialog.dispatchEvent(new KeyboardEvent("keydown", {key: "ArrowLeft"}));
    expect(dialog.querySelector("h2")?.textContent).toBe("Ship release");
    dialog.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape"}));
    expect(root.querySelector("dialog.lightbox")).toBeNull();
  });

  it("shows the weekly overview with comparisons and navigates weeks", async () => {
    await openActivity();
    click('[data-action="open-week"]'); await flush();
    expect(root.querySelector(".week-bento")).not.toBeNull();
    expect(root.querySelector(".hero-number strong")?.textContent).toBe("3");
    expect(root.querySelector(".hero-deltas")?.textContent).toContain("+2 vs last week");
    expect(root.querySelectorAll(".trend .tb")).toHaveLength(8);
    expect(root.querySelectorAll(".daily .day-col")).toHaveLength(7);
    expect(root.querySelector(".hd.done")).not.toBeNull();
    expect(root.querySelector('[data-week="next"]')?.hasAttribute("disabled")).toBe(true);
    click('[data-week="prev"]'); await flush(); await flush();
    expect(send).toHaveBeenCalledWith("weekly_overview", {date: "2026-09-28"});
    click('[data-action="close-week"]'); await flush();
    expect(root.querySelector(".gallery")).not.toBeNull();
  });
});

describe("year calendar", () => {
  async function openRoutines() {
    await renderApp(root, client);
    click('[data-page="routines"]');
  }

  it("draws twelve months with colored deadline days and upcoming chips", async () => {
    await openRoutines();
    expect(root.querySelector(".cal-year")?.textContent).toBe("2026");
    expect(root.querySelectorAll(".month")).toHaveLength(12);
    expect(root.querySelector('[data-day="2026-10-06"]')?.className).toContain("sw-orange");
    expect(root.querySelector('[data-day="2026-10-30"]')?.className).toContain("sw-red");
    expect(root.querySelector('[data-day="2026-10-05"]')?.className).toContain("is-today");
    expect([...root.querySelectorAll(".ddl-chip strong")].map(el => el.textContent)).toEqual(["Conference", "Rent"]);
    click('[data-cal="next"]');
    expect(root.querySelector(".cal-year")?.textContent).toBe("2027");
    click('[data-cal="today"]');
    expect(root.querySelector(".cal-year")?.textContent).toBe("2026");
  });

  it("adds a colored deadline from a day popover", async () => {
    await openRoutines();
    click('[data-day="2026-10-10"]');
    const popover = root.querySelector<HTMLElement>(".day-popover")!;
    expect(popover.dataset.popoverDay).toBe("2026-10-10");
    popover.querySelector<HTMLInputElement>('[name="popEvent"]')!.value = "Submit draft";
    popover.querySelector<HTMLInputElement>('[name="popColor"][value="violet"]')!.checked = true;
    popover.querySelector<HTMLSelectElement>('[name="popType"]')!.value = "3";
    popover.querySelector("form")!.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("deadline_add", {event: "Submit draft", date: "2026-10-10", type: "3", color: "violet", repeat: ""});
    expect(root.querySelector(".day-popover")).toBeNull();
  });

  it("recolors, moves by drag and drop, and protects past days", async () => {
    await openRoutines();
    click('[data-day="2026-10-06"]');
    click('[data-pop-color="aqua"][data-pop-index="0"]'); await flush(); await flush();
    expect(send).toHaveBeenCalledWith("deadline_update", {originalEvent: "Conference", event: "Conference", date: "2026-10-06", color: "aqua"});

    const drop = new Event("drop", {bubbles: true, cancelable: true});
    Object.defineProperty(drop, "dataTransfer", {value: {getData: () => "1"}});
    root.querySelector('[data-day="2026-10-20"]')!.dispatchEvent(drop);
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("deadline_update", {originalEvent: "Rent", event: "Rent", date: "2026-10-20"});
    expect(root.querySelector("#notice")?.textContent).toContain("Moved “Rent”");

    click('[data-day="2026-10-01"]');
    expect(root.querySelector(".day-popover .pop-form")).toBeNull();
    expect(root.querySelector(".pop-past")).not.toBeNull();
    root.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    expect(root.querySelector(".day-popover")).toBeNull();
  });

  it("saves deadline colors from the edit dialog", async () => {
    await openRoutines();
    click('.deadline-list [data-deadline="1"][data-operation="edit"]');
    const dialog = root.querySelector("dialog")!;
    expect(dialog.querySelector<HTMLInputElement>('[name="color"][value="red"]')!.checked).toBe(true);
    dialog.querySelector<HTMLInputElement>('[name="color"][value="blue"]')!.checked = true;
    dialog.querySelector("form")!.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush();
    expect(send).toHaveBeenCalledWith("deadline_update", expect.objectContaining({originalEvent: "Rent", color: "blue"}));
  });
});

describe("recurring events", () => {
  const recurring = () => {
    const data = structuredClone(snapshot);
    data.deadlines.push({event: "Team report", deadline: "2026-10-09", daysLeft: 4, status: "soon", type: "1", typeName: "Coding", repeat: "biweekly", repeatDay: null});
    send.mockImplementation(async (command: string) => command === "status_snapshot" ? structuredClone(data) : command === "workspace_info" ? {configIds: [0], mode: "j"} : {});
  };

  it("shows every occurrence through the end of the year and lists the series once", async () => {
    recurring();
    await renderApp(root, client);
    click('[data-page="routines"]');
    const base = root.querySelector('[data-day="2026-10-09"]')!;
    expect(base.className).toContain("repeats");
    expect(base.className).not.toContain("occurrence");
    for (const day of ["2026-10-23", "2026-11-06", "2026-12-18"]) expect(root.querySelector(`[data-day="${day}"]`)?.className).toContain("occurrence");
    expect(root.querySelector('[data-day="2026-10-16"]')?.className).not.toContain("has-ddl");
    expect(root.querySelector('[data-day="2026-11-06"]')?.getAttribute("aria-label")).toContain("Team report (repeats every 2 weeks)");
    const chips = [...root.querySelectorAll(".ddl-chip")].map(chip => chip.textContent ?? "");
    expect(chips.filter(text => text.includes("Team report"))).toHaveLength(1);
    expect(chips.find(text => text.includes("Team report"))).toContain("↻ Every 2 weeks");
    expect(root.querySelector(".deadline-list .repeat-badge")?.textContent).toBe("Every 2 weeks");
    click('[data-cal="next"]');
    expect(root.querySelector('[data-day="2027-01-01"]')?.className).toContain("occurrence");
  });

  it("adds a repeating event from a day card and edits the rule in the dialog", async () => {
    recurring();
    await renderApp(root, client);
    click('[data-page="routines"]');
    click('[data-day="2026-10-12"]');
    const popover = root.querySelector<HTMLElement>(".day-popover")!;
    popover.querySelector<HTMLInputElement>('[name="popEvent"]')!.value = "Gym";
    popover.querySelector<HTMLSelectElement>('[name="popRepeat"]')!.value = "weekly";
    popover.querySelector("form")!.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("deadline_add", expect.objectContaining({event: "Gym", date: "2026-10-12", repeat: "weekly"}));

    click('[data-day="2026-11-06"]');
    expect(root.querySelector(".day-popover .pop-copy small")?.textContent).toContain("↻ Every 2 weeks, next on");
    click('.day-popover [data-pop-edit="2"]');
    const dialog = root.querySelector("dialog")!;
    expect(dialog.querySelector<HTMLSelectElement>('[name="repeat"]')!.value).toBe("biweekly");
    dialog.querySelector<HTMLSelectElement>('[name="repeat"]')!.value = "monthly";
    dialog.querySelector("form")!.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush();
    expect(send).toHaveBeenCalledWith("deadline_update", expect.objectContaining({originalEvent: "Team report", repeat: "monthly"}));
  });
});

describe("page helpers", () => {
  it("computes repeat occurrences like the backend", () => {
    expect(nextOccurrence("2026-11-04", "weekly")).toBe("2026-11-11");
    expect(nextOccurrence("2026-12-30", "biweekly")).toBe("2027-01-13");
    expect(nextOccurrence("2026-01-31", "monthly", 31)).toBe("2026-02-28");
    expect(nextOccurrence("2026-02-28", "monthly", 31)).toBe("2026-03-31");
    expect(nextOccurrence("2026-12-15", "monthly")).toBe("2027-01-15");
    const days = deadlinesByDay([{event: "Rent", deadline: "2026-10-31", daysLeft: 0, status: "ok", type: null, typeName: null, repeat: "monthly", repeatDay: 31}], {}, "2026-12-31");
    expect([...days.keys()]).toEqual(["2026-10-31", "2026-11-30", "2026-12-31"]);
    expect(days.get("2026-11-30")![0].occurrence).toBe(true);
  });

  it("builds Monday-first month grids", () => {
    const cells = monthCells(2026, 10);
    expect(cells.slice(0, 7)).toEqual([null, null, null, null, null, null, "2026-11-01"]);
    expect(cells.length % 7).toBe(0);
    expect(cells.filter(Boolean)).toHaveLength(30);
  });

  it("prefers a deadline's own color, then its type color", () => {
    expect(deadlineSwatch(snapshot.deadlines[1], snapshot.taskTypes)).toBe("red");
    expect(deadlineSwatch(snapshot.deadlines[0], snapshot.taskTypes)).toBe("orange");
    expect(deadlineSwatch({...snapshot.deadlines[0], type: null, color: "url(x)"}, snapshot.taskTypes)).toBe("violet");
  });

  it("sorts and groups history by recency", () => {
    const sorted = filterHistory(history, {...defaultGalleryFilter(), sort: "longest"});
    expect(sorted[0].status).toBe("dropped");
    const groups = groupHistory(filterHistory(history, defaultGalleryFilter()), "2026-10-05", "newest");
    expect(groups.map(group => group.label)).toEqual(["This week", "Last week", "September 2026"]);
    expect(groups[2].items.map(item => item.description)).toEqual(["Draft essay", "<img src=x onerror=alert(1)>"]);
  });
});
