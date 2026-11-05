// Tests for the tasks-by-type board: every configured group, their order, folding, and adding tasks inline.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { groupTasks } from "./board";
import { renderApp } from "./render";
import type { BridgeClient, Snapshot, TaskItem } from "./types";

const task = (description: string, priority: number, type: string, extra: Partial<TaskItem> = {}): TaskItem =>
  ({description, priority, type, typeName: "", alarmFrom: null, procrastinated: false, procrastinateDays: null, ...extra});

const snapshot: Snapshot = {
  config: {rootDir: "/local/config", activeId: 0, activeConfigDir: "/local/config/user_config_0", tasksPath: "", deadlinesPath: "", habitsPath: "", recordsPath: ""},
  today: {date: "2026-10-05", weekday: "monday", parity: "even"},
  schedule: {isSkipped: false, current: null, next: null, timeToNext: null, events: [], hasSyncedOverlay: false},
  taskTypes: {"1": "Coding", "2": "Writing", "3": "Reading", "4": "Exercise"},
  tasks: [
    task("Fix build", 6, "1"),
    task("Refactor parser", 6, "1", {alarmFrom: "2026-10-09"}),
    task("Draft essay", 6, "2", {procrastinated: true, procrastinateDays: 3}),
    task("Read paper", 9, "3"),
    task("Old habit", 2, "9", {typeName: "Retired"})
  ],
  deadlines: [],
  habits: [],
  history: []
};

let root: HTMLElement;
let send: ReturnType<typeof vi.fn>;
let client: BridgeClient;
let data: Snapshot;
const flush = () => new Promise(resolve => setTimeout(resolve, 0));
const click = (selector: string) => root.querySelector<HTMLElement>(selector)!.click();
const names = () => [...root.querySelectorAll(".task-group h3")].map(el => el.textContent);
const group = (name: string) => [...root.querySelectorAll<HTMLElement>(".task-group")].find(el => el.querySelector("h3")?.textContent === name)!;

beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  root = document.querySelector("#app")!;
  window.localStorage.clear();
  vi.stubGlobal("matchMedia", vi.fn((query: string) => ({matches: query.includes("reduced-motion"), addEventListener: vi.fn()})));
  data = structuredClone(snapshot);
  send = vi.fn(async (command: string, payload: Record<string, unknown>) => {
    if (command === "status_snapshot") return structuredClone(data);
    if (command === "workspace_info") return {configIds: [0], activeId: 0, mode: "p", rootDir: "/local/config"};
    if (command === "task_add") data.tasks.push(task(String(payload.description), Number(payload.priority), String(payload.type)));
    return {};
  });
  client = {send};
  vi.spyOn(window, "confirm").mockReturnValue(true);
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); delete document.documentElement.dataset.mode; });

describe("task groups", () => {
  it("includes every configured type and orders groups by average importance, then size", () => {
    const groups = groupTasks(snapshot);
    // Reading averages 9; Coding and Writing both average 6, so the bigger Coding goes first; Retired averages 2;
    // Exercise has no tasks and comes last.
    expect(groups.map(g => g.name)).toEqual(["Reading", "Coding", "Writing", "Retired", "Exercise"]);
    expect(groups.map(g => g.tasks.length)).toEqual([1, 2, 1, 1, 0]);
    expect(groups.find(g => g.name === "Coding")).toMatchObject({average: 6, high: 0, later: 1, deferred: 0});
    expect(groups.find(g => g.name === "Writing")).toMatchObject({deferred: 1});
    expect(groups.find(g => g.name === "Exercise")).toMatchObject({average: null, tasks: []});
  });

  it("shows empty groups on the board with a way to add their first task", async () => {
    await renderApp(root, client);
    expect(names()).toEqual(["Reading", "Coding", "Writing", "Retired", "Exercise"]);
    const empty = group("Exercise");
    expect(empty.classList.contains("is-empty")).toBe(true);
    expect(empty.textContent).toContain("Nothing here yet.");
    expect(root.querySelectorAll("[data-compose]")).toHaveLength(5);
  });

  it("hides groups without matches while searching", async () => {
    await renderApp(root, client);
    const search = root.querySelector<HTMLInputElement>('[name="search"]')!;
    search.value = "paper"; search.dispatchEvent(new Event("input"));
    expect(names()).toEqual(["Reading"]);
    search.value = ""; search.dispatchEvent(new Event("input"));
    expect(names()).toHaveLength(5);
  });
});

describe("folding", () => {
  it("folds every group into its metrics and remembers the choice", async () => {
    await renderApp(root, client);
    expect(root.querySelector('[data-board-view="expanded"]')?.getAttribute("aria-pressed")).toBe("true");
    click('[data-board-view="folded"]');
    expect(root.querySelector('[data-board-view="folded"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(root.querySelector(".fold-switch")?.classList.contains("is-folded")).toBe(true);
    expect(root.querySelectorAll(".task-group.is-folded")).toHaveLength(5);
    expect(root.querySelector(".task-card")).toBeNull();
    expect(root.querySelector("#task-board")?.classList.contains("is-uniform")).toBe(true);
    const coding = group("Coding");
    expect(coding.querySelector(".ring figcaption")?.textContent).toBe("6.0");
    expect(coding.querySelectorAll(".tg-strip i")).toHaveLength(2);
    expect(coding.querySelector(".tg-share")?.textContent).toContain("40% of open tasks");
    expect(coding.textContent).toContain("1 later");
    expect(group("Writing").textContent).toContain("1 deferred");
    expect(group("Reading").textContent).toContain("1 important");
    expect(group("Exercise").textContent).toContain("No tasks yet.");
    expect(window.localStorage.getItem("schedule-everything-task-board")).toBe("folded");

    await renderApp(root, client);
    expect(root.querySelectorAll(".task-group.is-folded")).toHaveLength(5);
    click('[data-board-view="expanded"]');
    expect(root.querySelector(".task-group.is-folded")).toBeNull();
    expect(root.querySelectorAll(".task-card")).toHaveLength(5);
  });

  it("opens or folds one group at a time from its header", async () => {
    window.localStorage.setItem("schedule-everything-task-board", "folded");
    await renderApp(root, client);
    click('[data-group-toggle="1"]');
    expect(group("Coding").classList.contains("is-folded")).toBe(false);
    expect(group("Coding").querySelectorAll(".task-card")).toHaveLength(2);
    expect(group("Coding").querySelector("[data-group-toggle]")?.getAttribute("aria-expanded")).toBe("true");
    expect(root.querySelectorAll(".task-group.is-folded")).toHaveLength(4);
    expect(root.querySelector("#task-board")?.classList.contains("is-uniform")).toBe(false);
    expect(document.activeElement?.getAttribute("data-group-toggle")).toBe("1");
    click('[data-group-toggle="1"]');
    expect(root.querySelectorAll(".task-group.is-folded")).toHaveLength(5);
  });

  it("lists matching tasks while searching, even when folded", async () => {
    window.localStorage.setItem("schedule-everything-task-board", "folded");
    await renderApp(root, client);
    const search = root.querySelector<HTMLInputElement>('[name="search"]')!;
    search.value = "fix"; search.dispatchEvent(new Event("input"));
    expect([...root.querySelectorAll(".task-card h4")].map(el => el.textContent)).toEqual(["Fix build"]);
    expect(root.querySelector("[data-group-toggle]")).toBeNull();
  });
});

describe("adding a task inline", () => {
  const compose = (name: string) => group(name).querySelector<HTMLButtonElement>("[data-compose]")!.click();
  const form = () => root.querySelector<HTMLFormElement>("form.composer")!;
  const type = (text: string, priority?: number) => {
    const input = form().querySelector<HTMLInputElement>('[name="composerText"]')!;
    input.value = text; input.dispatchEvent(new Event("input", {bubbles: true}));
    if (priority !== undefined) {
      const range = form().querySelector<HTMLInputElement>('[name="composerPriority"]')!;
      range.value = String(priority); range.dispatchEvent(new Event("input", {bubbles: true}));
    }
  };
  const submit = () => form().dispatchEvent(new Event("submit", {cancelable: true}));

  it("adds a task to the group without a dialog and keeps the form open for the next one", async () => {
    await renderApp(root, client);
    compose("Exercise");
    expect(root.querySelector("dialog")).toBeNull();
    expect(form().dataset.composer).toBe("4");
    expect(document.activeElement?.getAttribute("name")).toBe("composerText");
    type("Morning run", 8);
    expect(form().querySelector(".composer-importance b")?.textContent).toBe("8");
    submit();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_add", {description: "Morning run", priority: 8, type: "4", postpone: 0});
    expect(root.querySelector("#notice")?.textContent).toContain("Added “Morning run” to Exercise.");
    // The new task is greeted, its group moves up to second (average 8, after Reading's 9), and the form
    // waits for another task.
    expect(names().slice(0, 3)).toEqual(["Reading", "Exercise", "Coding"]);
    expect(group("Exercise").querySelector(".task-card.just-added h4")?.textContent).toBe("Morning run");
    expect(form().dataset.composer).toBe("4");
    expect(form().querySelector<HTMLInputElement>('[name="composerText"]')!.value).toBe("");
    expect(form().querySelector<HTMLInputElement>('[name="composerPriority"]')!.value).toBe("8");
    expect(document.activeElement?.getAttribute("name")).toBe("composerText");
  });

  it("keeps an unsent draft across redraws and closes with Escape", async () => {
    await renderApp(root, client);
    compose("Coding");
    type("Write tests");
    const search = root.querySelector<HTMLInputElement>('[name="search"]')!;
    search.value = ""; search.dispatchEvent(new Event("input"));
    expect(form().querySelector<HTMLInputElement>('[name="composerText"]')!.value).toBe("Write tests");
    form().querySelector('[name="composerText"]')!.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    expect(root.querySelector("form.composer")).toBeNull();
    expect(document.activeElement?.getAttribute("data-compose")).toBe("1");
    expect(send).not.toHaveBeenCalledWith("task_add", expect.anything());
  });

  it("asks before replacing a task with the same name and ignores empty text", async () => {
    await renderApp(root, client);
    compose("Reading");
    submit();
    expect(form().classList.contains("shake")).toBe(true);
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    type("Fix build");
    submit();
    expect(window.confirm).toHaveBeenCalledWith("“Fix build” is already on your list under Coding. Replace it with this one?");
    expect(send).not.toHaveBeenCalledWith("task_add", expect.anything());
    expect(form().querySelector<HTMLInputElement>('[name="composerText"]')!.value).toBe("Fix build");
  });

  it("restores the text when saving fails", async () => {
    await renderApp(root, client);
    compose("Reading");
    type("Annotate notes", 4);
    send.mockRejectedValueOnce(new Error("Storage is busy"));
    submit();
    await flush(); await flush();
    expect(root.querySelector("#notice")?.textContent).toBe("Storage is busy");
    expect(form().querySelector<HTMLInputElement>('[name="composerText"]')!.value).toBe("Annotate notes");
  });

  it("works from a folded group too", async () => {
    window.localStorage.setItem("schedule-everything-task-board", "folded");
    await renderApp(root, client);
    compose("Writing");
    type("Outline chapter", 7);
    submit();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_add", {description: "Outline chapter", priority: 7, type: "2", postpone: 0});
    expect(group("Writing").classList.contains("is-folded")).toBe(true);
    expect(group("Writing").querySelectorAll(".tg-strip i")).toHaveLength(2);
    expect(group("Writing").classList.contains("just-added")).toBe(true);
  });
});
