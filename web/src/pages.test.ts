// Tests for P/J overview layouts, the activity gallery and weekly overview, and the year calendar.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { filterHistory, groupHistory, defaultGalleryFilter } from "./activity";
import { axisTones, layoutTree, packLabels, treeLanes } from "./tree";
import { deadlinesByDay, deadlineSwatch, monthCells, nextOccurrence } from "./calendar";
import { renderApp } from "./render";
import type { BridgeClient, ConferencePayload, HistoryItem, Snapshot, WeekDay, WeeklyOverview, WeekStats } from "./types";
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

const conferencePayload: ConferencePayload = {
  year: 2026, source: "https://github.com/huggingface/ai-deadlines", fetchedAt: "2026-10-01T10:00:00+00:00", error: null,
  conferences: [{id: "neurips", name: "NeurIPS", group: "ml"}, {id: "cvpr", name: "CVPR", group: "vision"}, {id: "icra", name: "ICRA", group: "robotics"}],
  events: [
    {id: "neurips2026-submission-1", conference: "neurips", name: "NeurIPS", group: "ml", edition: 2026, fullName: "Conference on Neural Information Processing Systems", link: "https://neurips.cc/", place: "Sydney, Australia", kind: "submission", label: "Paper submission deadline", date: "2026-10-12", endDate: null, time: "03:59", sourceTime: "Oct 11, 23:59 AoE"},
    {id: "cvpr2026-rebuttal-0", conference: "cvpr", name: "CVPR", group: "vision", edition: 2026, link: "javascript:alert(1)", place: "Denver, USA", kind: "rebuttal", label: "Rebuttal Period", date: "2026-10-14", endDate: "2026-10-16", time: "03:59", sourceTime: "Oct 13, 23:59 – Oct 15, 23:59 AoE"},
    {id: "cvpr2026-conference", conference: "cvpr", name: "CVPR", group: "vision", edition: 2026, link: "javascript:alert(1)", place: "Denver, USA", kind: "conference", label: "CVPR 2026", date: "2026-10-20", endDate: "2026-10-23", time: null, sourceTime: "October 20-23, 2026"},
    {id: "icra2027-abstract-0", conference: "icra", name: "ICRA", group: "robotics", edition: 2027, link: null, place: "", kind: "abstract", label: "Abstract deadline", date: "2026-11-02", endDate: null, time: "23:59", sourceTime: "Nov 2, 23:59 PST"},
    {id: "icra2027-rebuttal-0", conference: "icra", name: "ICRA", group: "robotics", edition: 2027, link: null, place: "", kind: "rebuttal", label: "Author + Reviewer + AC Discussion End", date: "2026-11-09", endDate: null, time: "04:59", sourceTime: "Nov 8, 23:59 AoE"}
  ]
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
    if (command === "conference_calendar") return structuredClone(conferencePayload);
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
  beforeEach(() => window.localStorage.setItem("schedule-everything-history-layout", "gallery"));
  afterEach(() => window.localStorage.clear());
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

describe("activity tree", () => {
  beforeEach(() => window.localStorage.clear());
  async function openActivity() {
    await renderApp(root, client);
    click('[data-page="activity"]');
    await flush(); await flush();
  }
  const titles = () => [...root.querySelectorAll(".tree-node strong")].map(el => el.textContent);

  it("is the default layout: one node per event, newest first, in type lanes on both sides of the trunk", async () => {
    await openActivity();
    expect(root.querySelector('[data-layout="tree"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(root.querySelector(".gallery-card")).toBeNull();
    expect(root.querySelector('[name="gallerySort"]')).toBeNull();
    expect(titles()).toEqual(["Ship release", "Wrong task", "Draft essay", "<img src=x onerror=alert(1)>"]);
    expect(root.querySelector(".tree-node img")).toBeNull();
    const nodes = [...root.querySelectorAll<HTMLElement>(".tree-node")];
    expect(nodes.map(node => node.className.match(/side-[lr]/)![0])).toEqual(["side-l", "side-l", "side-r", "side-l"]);
    expect(nodes.map(node => node.style.getPropertyValue("--depth"))).toEqual(["1", "1", "1", "2"]);
    expect(nodes[0].className).toContain("sw-blue");
    expect(nodes[1].className).toContain("status-cancelled");
    const ys = nodes.map(node => Number(node.dataset.y));
    expect(ys).toEqual([...ys].sort((a, b) => a - b));
    expect([...root.querySelectorAll(".tree-lane-head")].map(el => el.textContent)).toEqual(["Coding", "Writing", "Reading"]);
    expect([...root.querySelectorAll(".tree-mark")].map(el => el.textContent)).toEqual(["October 2026", "Today", "3 quiet days", "Thu, Oct 1", "8 quiet days", "September 2026", "Tue, Sep 22", "20 quiet days", "Tue, Sep 1"]);
    expect(root.querySelector<HTMLElement>(".tree-trunk")?.getAttribute("style")).toContain("var(--c-blue)");
    expect(root.querySelectorAll(".tm-ticks line")).toHaveLength(4);
  });

  it("opens events in newest-first order and keeps filters and search", async () => {
    await openActivity();
    click('.tree-node [data-history="2"]');
    const dialog = root.querySelector<HTMLDialogElement>("dialog.lightbox")!;
    expect(dialog.querySelector("h2")?.textContent).toBe("Wrong task");
    expect(dialog.textContent).toContain("2 of 4");
    dialog.querySelector<HTMLButtonElement>('[data-lightbox="next"]')!.click();
    expect(dialog.querySelector("h2")?.textContent).toBe("Draft essay");
    dialog.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape"}));
    click('[data-filter-status="completed"]');
    expect(titles()).toEqual(["Ship release", "Draft essay"]);
    const search = root.querySelector<HTMLInputElement>('[name="gallerySearch"]')!;
    search.value = "essay"; search.dispatchEvent(new Event("input"));
    expect(titles()).toEqual(["Draft essay"]);
    expect(root.querySelector('[name="gallerySearch"]')).toBe(search);
    expect(root.querySelector(".tree.single")).not.toBeNull();
  });

  it("shows an event's details while hovering it, without opening a dialog", async () => {
    await openActivity();
    const card = root.querySelector<HTMLElement>('.tree-node [data-history="2"]')!;
    card.dispatchEvent(new MouseEvent("pointerover", {bubbles: true}));
    const peek = document.querySelector<HTMLElement>(".peek")!;
    expect(peek.classList.contains("visible")).toBe(true);
    expect(peek.getAttribute("aria-hidden")).toBe("true");
    expect(peek.querySelector(".peek-title strong")?.textContent).toBe("Wrong task");
    expect(peek.textContent).toContain("Cancelled");
    expect(peek.textContent).toContain("Coding");
    expect(peek.querySelector(".peek-foot b")?.textContent).toBe("2/10");
    expect(root.querySelector("dialog")).toBeNull();
    // Moving to a neighbor swaps the card's contents; leaving hides it after a short grace period.
    const next = root.querySelector<HTMLElement>('.tree-node [data-history="0"]')!;
    card.dispatchEvent(new MouseEvent("pointerout", {bubbles: true, relatedTarget: next}));
    next.dispatchEvent(new MouseEvent("pointerover", {bubbles: true}));
    expect(peek.querySelector(".peek-title strong")?.textContent).toBe("Ship release");
    // Its only completed peer took just as long.
    expect(peek.querySelector(".peek-note")?.textContent).toBe("About as quick as your other completed tasks.");
    vi.useFakeTimers();
    try {
      next.dispatchEvent(new MouseEvent("pointerout", {bubbles: true, relatedTarget: root}));
      expect(peek.classList.contains("visible")).toBe(true);
      vi.advanceTimersByTime(200);
      expect(peek.classList.contains("visible")).toBe(false);
    } finally { vi.useRealTimers(); }
    // Clicking still opens the full story.
    next.dispatchEvent(new MouseEvent("pointerover", {bubbles: true}));
    next.click();
    expect(peek.classList.contains("visible")).toBe(false);
    expect(root.querySelector("dialog.lightbox h2")?.textContent).toBe("Ship release");
  });

  it("puts lane labels in a band above the events", async () => {
    await openActivity();
    const tree = root.querySelector<HTMLElement>(".tree")!;
    expect(tree.style.getPropertyValue("--depths")).toBe("2");
    expect(tree.querySelector(".tree-stage > .tree-lanes")).not.toBeNull();
    expect(tree.querySelector(".tree-window > [data-tree-viewport]")).not.toBeNull();
    expect(tree.querySelector('.tree-lane-head[data-tip="Writing"]')).not.toBeNull();
  });

  it("jumps to a rhythm day instead of filtering, and remembers the layout", async () => {
    await openActivity();
    click('[data-heat-day="2026-10-01"]');
    expect(root.querySelector('[data-action="clear-day"]')).toBeNull();
    expect(titles()).toHaveLength(4);
    expect(root.querySelector('.tree-node[data-day="2026-10-01"]')?.classList.contains("flash")).toBe(true);
    click('[data-layout="gallery"]');
    expect(root.querySelectorAll(".gallery-card")).toHaveLength(4);
    expect(window.localStorage.getItem("schedule-everything-history-layout")).toBe("gallery");
    click('[data-layout="tree"]');
    expect(root.querySelectorAll(".tree-node")).toHaveLength(4);
  });

  it("highlights the events at the center of the view as the tree scrolls", async () => {
    Object.defineProperty(HTMLElement.prototype, "clientHeight", {configurable: true, get() { return this.hasAttribute("data-tree-viewport") ? 400 : 0; }});
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { callback(0); return 1; });
    vi.stubGlobal("matchMedia", vi.fn((query: string) => ({matches: query.includes("reduced-motion"), addEventListener: vi.fn()})));
    try {
      await openActivity();
      const viewport = root.querySelector<HTMLElement>("[data-tree-viewport]")!;
      const nodes = [...root.querySelectorAll<HTMLElement>(".tree-node")];
      expect(root.querySelector<HTMLElement>(".tree-canvas")?.style.getPropertyValue("--pad")).toBe("200px");
      expect(nodes[0].classList.contains("is-focus")).toBe(true);
      expect(root.querySelector("[data-tree-range]")?.textContent).toContain("Oct");
      expect(root.querySelector('[data-heat-day="2026-10-05"]')?.classList.contains("in-view")).toBe(true);
      let top = Number(nodes[2].dataset.y);
      Object.defineProperty(viewport, "scrollTop", {configurable: true, get: () => top, set: value => { top = value; }});
      viewport.dispatchEvent(new Event("scroll"));
      expect(nodes[0].classList.contains("is-focus")).toBe(false);
      expect(nodes[2].classList.contains("is-focus")).toBe(true);
      expect(nodes[2].style.getPropertyValue("--f")).toBe("1.000");
      expect(root.querySelector("[data-tree-summary]")?.textContent).toMatch(/Writing/);
      expect(root.querySelector<HTMLElement>(".tree")?.style.getPropertyValue("--view-c")).toBe("var(--c-orange)");
      expect(root.querySelector('[data-heat-day="2026-09-22"]')?.classList.contains("in-view")).toBe(true);
      expect(root.querySelector('[data-heat-day="2026-10-05"]')?.classList.contains("in-view")).toBe(false);
      expect(root.querySelector<HTMLElement>("[data-tree-window]")?.style.height).not.toBe("");
      nodes[2].querySelector<HTMLElement>(".tn-card")!.dispatchEvent(new KeyboardEvent("keydown", {key: "ArrowDown", bubbles: true}));
      expect(document.activeElement).toBe(nodes[3].querySelector(".tn-card"));
      expect(top).toBe(Number(nodes[3].dataset.y));
    } finally {
      delete (HTMLElement.prototype as {clientHeight?: number}).clientHeight;
    }
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

describe("AI conferences", () => {
  const openRoutines = async () => { await renderApp(root, client); click('[data-page="routines"]'); await flush(); await flush(); };
  beforeEach(() => window.localStorage.clear());

  it("is off by default and shows submission days and conference days once turned on", async () => {
    await openRoutines();
    expect(send).not.toHaveBeenCalledWith("conference_calendar", expect.anything());
    expect(root.querySelector(".conf-deadline, .conf-range")).toBeNull();
    click("[data-conf-toggle]"); await flush(); await flush();
    expect(send).toHaveBeenCalledWith("conference_calendar", {year: 2026});
    expect(root.querySelector("[data-conf-toggle]")?.getAttribute("aria-pressed")).toBe("true");
    const deadline = root.querySelector('[data-day="2026-10-12"]')!;
    expect(deadline.className).toContain("conf-deadline");
    expect(deadline.querySelector(".conf-flag")).not.toBeNull();
    expect(deadline.getAttribute("aria-label")).toContain("NeurIPS 2026 paper submission deadline (03:59)");
    expect(root.querySelector('[data-day="2026-10-20"]')?.className).toContain("conf-start");
    expect(root.querySelector('[data-day="2026-10-21"]')?.className).toContain("conf-range");
    expect(root.querySelector('[data-day="2026-10-23"]')?.className).toContain("conf-end");
    expect(root.querySelector('[data-day="2026-10-24"]')?.className).not.toContain("conf-range");
    expect([...root.querySelectorAll(".conf-item strong")].map(el => el.textContent)).toEqual(["NeurIPS 2026", "CVPR 2026", "CVPR 2026", "ICRA 2027", "ICRA 2027"]);
    expect(JSON.parse(window.localStorage.getItem("schedule-everything-conferences")!)).toEqual({enabled: true, selected: null});
    // Month counts stay about your own deadlines.
    expect(root.querySelector(".month-count")?.textContent).toBe("2");
  });

  it("marks rebuttal periods apart from deadlines and conference days", async () => {
    window.localStorage.setItem("schedule-everything-conferences", JSON.stringify({enabled: true, selected: null}));
    await openRoutines();
    const first = root.querySelector('[data-day="2026-10-14"]')!;
    expect(first.className).toContain("conf-rebuttal rebuttal-start");
    expect(first.className).not.toMatch(/conf-deadline|conf-range|rebuttal-end/);
    expect(first.querySelector(".conf-flag")).toBeNull();
    expect(first.getAttribute("style")).toBe("--rebuttal-c: var(--c-blue)");
    const middle = root.querySelector('[data-day="2026-10-15"]')!;
    expect(middle.className).not.toMatch(/rebuttal-(start|end)/);
    for (const text of [middle.getAttribute("aria-label"), middle.getAttribute("data-tip")]) expect(text).toContain("CVPR 2026 rebuttal period, Oct 14 – Oct 16 (ends 03:59)");
    expect(root.querySelector('[data-day="2026-10-16"]')?.className).toContain("conf-rebuttal rebuttal-end");
    expect(root.querySelector('[data-day="2026-10-17"]')?.className).not.toContain("conf-rebuttal");
    // When only the last day is published, that day gets a short mark of its own; acronyms stay capitalized.
    const single = root.querySelector('[data-day="2026-11-09"]')!;
    expect(single.className).toContain("conf-rebuttal rebuttal-start rebuttal-end");
    expect(single.getAttribute("aria-label")).toContain("ICRA 2027 author + reviewer + AC discussion end (04:59)");
    const items = [...root.querySelectorAll(".conf-item")].filter(item => item.querySelector(".conf-kind-rebuttal"));
    expect(items.map(item => item.querySelector("small")?.textContent)).toEqual(["Oct 14 – Oct 16 · Rebuttal Period", "Author + Reviewer + AC Discussion End · Nov 9"]);
    click('[data-day="2026-10-15"]');
    const card = root.querySelector(".day-popover .pop-conf")!;
    expect(card.querySelector("i")?.className).toBe("conf-kind-rebuttal");
    expect(card.querySelector("strong")?.textContent).toBe("CVPR 2026 · Rebuttal Period");
    expect(card.querySelector("small")?.textContent).toBe("Oct 14 – Oct 16 · ends 03:59 your time · Oct 13, 23:59 – Oct 15, 23:59 AoE");
    expect(card.querySelector("[data-conf-add]")).toBeNull();
    // Unselecting a conference hides its rebuttal dates too.
    click("[data-conf-menu]");
    const cvpr = root.querySelector<HTMLInputElement>('[data-conf-pick="cvpr"]')!;
    cvpr.checked = false; cvpr.dispatchEvent(new Event("change"));
    expect(root.querySelector('[data-day="2026-10-15"]')?.className).not.toContain("conf-rebuttal");
  });

  it("filters conferences, remembers the choice, and refreshes on request", async () => {
    window.localStorage.setItem("schedule-everything-conferences", JSON.stringify({enabled: true, selected: null}));
    await openRoutines();
    expect(send).toHaveBeenCalledWith("conference_calendar", {year: 2026});
    click("[data-conf-menu]");
    expect(root.querySelectorAll("[data-conf-pick]")).toHaveLength(3);
    const neurips = root.querySelector<HTMLInputElement>('[data-conf-pick="neurips"]')!;
    neurips.checked = false; neurips.dispatchEvent(new Event("change"));
    expect(document.activeElement?.getAttribute("data-conf-pick")).toBe("neurips");
    expect(root.querySelector('[data-day="2026-10-12"]')?.className).not.toContain("conf-deadline");
    expect(JSON.parse(window.localStorage.getItem("schedule-everything-conferences")!).selected).toEqual(["cvpr", "icra"]);
    expect(root.querySelector("[data-conf-toggle] b")?.textContent).toBe("2");
    click("[data-conf-all]");
    expect(root.querySelector('[data-day="2026-10-12"]')?.className).toContain("conf-deadline");
    click("[data-conf-refresh]"); await flush(); await flush();
    expect(send).toHaveBeenCalledWith("conference_calendar", {year: 2026, refresh: true});
    // Escape works even when focus has fallen back to <body>.
    document.body.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    expect(root.querySelector(".conf-menu")).toBeNull();
    click('[data-cal="next"]'); await flush(); await flush();
    expect(send).toHaveBeenCalledWith("conference_calendar", {year: 2027});
  });

  it("lists conference dates in the day card and copies a deadline into yours", async () => {
    window.localStorage.setItem("schedule-everything-conferences", JSON.stringify({enabled: true, selected: null}));
    await openRoutines();
    click('[data-day="2026-10-21"]');
    const range = root.querySelector(".day-popover .pop-conf")!;
    expect(range.textContent).toContain("CVPR 2026");
    expect(range.textContent).toContain("Denver, USA");
    expect(range.querySelector("a")).toBeNull();
    expect(range.querySelector("[data-conf-add]")).toBeNull();
    click('[data-day="2026-10-12"]');
    const card = root.querySelector(".day-popover .pop-conf")!;
    expect(card.textContent).toContain("03:59 your time · Oct 11, 23:59 AoE");
    expect(card.querySelector("a")?.getAttribute("href")).toBe("https://neurips.cc/");
    click("[data-conf-add]"); await flush(); await flush();
    expect(send).toHaveBeenCalledWith("deadline_add", {event: "NeurIPS 2026 paper submission deadline", date: "2026-10-12", type: "", color: "violet", repeat: ""});
  });

  it("shows fetch errors without breaking the calendar", async () => {
    window.localStorage.setItem("schedule-everything-conferences", JSON.stringify({enabled: true, selected: ["neurips"]}));
    send.mockImplementation(async (command: string) => command === "status_snapshot" ? structuredClone(snapshot) : command === "workspace_info" ? {configIds: [0], mode: "j"} : command === "conference_calendar" ? {...structuredClone(conferencePayload), events: [], fetchedAt: null, error: "Could not fetch conference dates: offline"} : {});
    await openRoutines();
    expect(root.querySelector(".cal-side .conf-error")?.textContent).toContain("offline");
    expect(root.querySelectorAll(".month")).toHaveLength(12);
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

  it("colors the trunk by the type that dominates nearby events", () => {
    const at = (type: string, day: number) => ({...history[0], type, endedAt: `2026-10-${String(day).padStart(2, "0")}T10:00:00`});
    const run = [at("1", 9), at("1", 8), at("2", 7), at("1", 6), at("1", 5)];
    const tones = axisTones(run, snapshot.taskTypes);
    expect(tones[2].type).toBe("1");
    expect(tones[2].share).toBeGreaterThan(0.5);
    expect(tones[0]).toMatchObject({type: "1", tone: "blue"});
    // A tie goes to the event's own type.
    expect(axisTones([at("1", 9), at("2", 9)], snapshot.taskTypes, 1).map(tone => tone.type)).toEqual(["1", "2"]);
  });

  it("gives every type its own lane, alternating sides of the trunk", () => {
    const lanes = treeLanes(history, snapshot.taskTypes);
    expect(lanes.map(lane => [lane.name, lane.side, lane.depth])).toEqual([["Coding", -1, 1], ["Writing", 1, 1], ["Reading", -1, 2]]);
    expect(treeLanes([history[1]], snapshot.taskTypes)[0]).toMatchObject({side: 1, depth: 1});
    const layout = layoutTree(filterHistory(history, defaultGalleryFilter()), snapshot.taskTypes, "2026-10-05");
    expect(layout.height).toBe(layout.nodes[3].y);
    expect(layout.top).toBeLessThan(layout.nodes[0].marks[0].y);
    for (let i = 1; i < layout.nodes.length; i++) expect(layout.nodes[i].y - layout.nodes[i - 1].y).toBeGreaterThanOrEqual(64);
  });

  it("stacks lane labels in rows and keeps them inside the tree", () => {
    // Three wide labels over neighboring lanes need three rows; the label at the edge shifts inward.
    const {placements, rows} = packLabels([{x: 100, width: 80}, {x: 130, width: 80}, {x: 160, width: 80}, {x: 290, width: 60}, {x: 395, width: 40}], 400, 8, 10);
    expect(rows).toBe(3);
    expect(placements.map(place => place.row)).toEqual([0, 1, 2, 0, 0]);
    expect(placements[4].shift).toBe(-25);
    expect(placements[0].shift).toBe(0);
    expect(packLabels([], 400).rows).toBe(1);
  });

  it("floats tooltips on the page so no card can cut them off", async () => {
    await renderApp(root, client);
    click('[data-page="activity"]');
    await flush(); await flush();
    const cell = root.querySelector<HTMLElement>('.heat-cell[data-heat-day="2026-10-05"]')!;
    cell.dispatchEvent(new MouseEvent("pointerover", {bubbles: true}));
    const tip = document.querySelector<HTMLElement>(".float-tip")!;
    expect(tip.textContent).toBe("Oct 5: 1 completed");
    expect(tip.classList.contains("visible")).toBe(true);
    expect(tip.parentElement).toBe(document.body);
    cell.dispatchEvent(new MouseEvent("pointerout", {bubbles: true, relatedTarget: document.body}));
    expect(tip.classList.contains("visible")).toBe(false);
  });

  it("sorts and groups history by recency", () => {
    const sorted = filterHistory(history, {...defaultGalleryFilter(), sort: "longest"});
    expect(sorted[0].status).toBe("dropped");
    const groups = groupHistory(filterHistory(history, defaultGalleryFilter()), "2026-10-05", "newest");
    expect(groups.map(group => group.label)).toEqual(["This week", "Last week", "September 2026"]);
    expect(groups[2].items.map(item => item.description)).toEqual(["Draft essay", "<img src=x onerror=alert(1)>"]);
  });
});
