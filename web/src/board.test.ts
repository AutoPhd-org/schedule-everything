// Tests for the tasks-by-type board: every configured group, their order, folding, and adding tasks inline.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { boardGroups, boardOrder, groupTasks } from "./board";
import { renderApp } from "./render";
import type { BridgeClient, Snapshot, Subtask, SuggestionItem, TaskItem } from "./types";

const task = (description: string, priority: number, type: string, extra: Partial<TaskItem> = {}): TaskItem =>
  ({description, priority, type, typeName: "", alarmFrom: null, procrastinated: false, procrastinateDays: null, ...extra});

const snapshot: Snapshot = {
  config: {rootDir: "/local/config", activeId: 0, activeConfigDir: "/local/config/user_config_0", tasksPath: "", deadlinesPath: "", habitsPath: "", recordsPath: ""},
  today: {date: "2026-10-05", weekday: "monday", parity: "even"},
  schedule: {isSkipped: false, current: null, next: null, timeToNext: null, events: [], hasSyncedOverlay: false},
  taskTypes: {"1": "Coding", "2": "Writing", "3": "Reading", "4": "Exercise"},
  tasks: [
    task("Fix build", 6, "1", {progress: 100}),
    task("Refactor parser", 6, "1", {due: "2026-10-05T09:30", progress: 50}),
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
const shelfNames = () => [...root.querySelectorAll("#task-shelf .shelf-item h4")].map(el => el.textContent);
/** A type's group on the board, or its pill on the shelf of types without tasks. */
const group = (name: string) => [...root.querySelectorAll<HTMLElement>(".task-group, .shelf-item")].find(el => el.querySelector("[data-type-title]")?.textContent === name)!;

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
    if (command === "task_delete") data.tasks = data.tasks.filter(item => item.description !== payload.description);
    if (command === "task_subtasks") {
      const item = data.tasks.find(entry => entry.description === payload.description)!;
      item.subtasks = structuredClone(payload.subtasks as Subtask[]);
      return {description: item.description, subtasks: item.subtasks};
    }
    if (command === "task_type_save") {
      const id = payload.id ? String(payload.id) : String(Math.max(...Object.keys(data.taskTypes).map(Number)) + 1);
      data.taskTypes[id] = String(payload.name);
      return {id, name: String(payload.name), taskTypes: data.taskTypes};
    }
    if (command === "task_type_delete") {
      const id = String(payload.id);
      const moveTo = payload.moveTo ? String(payload.moveTo) : "";
      delete data.taskTypes[id];
      const owned = data.tasks.filter(item => item.type === id);
      owned.forEach(item => { item.type = moveTo; });
      return {taskTypes: data.taskTypes, movedTasks: owned.length, movedDeadlines: 0};
    }
    return {};
  });
  client = {send};
  vi.spyOn(window, "confirm").mockReturnValue(true);
  // Noon on the snapshot's day, so "Refactor parser" (due 9:30) is overdue.
  vi.useFakeTimers({toFake: ["Date"]});
  vi.setSystemTime(new Date(2026, 9, 5, 12, 0));
});
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); delete document.documentElement.dataset.mode; });

describe("task groups", () => {
  it("includes every configured type and orders groups by average importance, then size", () => {
    const groups = groupTasks(snapshot);
    // Reading averages 9; Coding and Writing both average 6, so the bigger Coding goes first; Retired averages 2;
    // Exercise has no tasks and comes last.
    expect(groups.map(g => g.name)).toEqual(["Reading", "Coding", "Writing", "Retired", "Exercise"]);
    expect(groups.map(g => g.tasks.length)).toEqual([1, 2, 1, 1, 0]);
    expect(groups.find(g => g.name === "Coding")).toMatchObject({average: 6, progress: 75, high: 0, timed: 1, overdue: 1, deferred: 0});
    // Before its finish time a timed task is not overdue yet.
    expect(groupTasks(snapshot, new Date(2026, 9, 5, 9, 0).getTime()).find(g => g.name === "Coding")).toMatchObject({timed: 1, overdue: 0});
    expect(groups.find(g => g.name === "Writing")).toMatchObject({deferred: 1});
    expect(groups.find(g => g.name === "Exercise")).toMatchObject({average: null, progress: null, tasks: []});
  });

  it("keeps types without tasks on a shelf below the board, each ready for a first task", async () => {
    await renderApp(root, client);
    expect(names()).toEqual(["Reading", "Coding", "Writing", "Retired"]);
    expect(shelfNames()).toEqual(["Exercise"]);
    expect(root.querySelector("#task-shelf .shelf-heading")?.textContent).toContain("Types without tasks 1");
    const waiting = group("Exercise");
    expect(waiting.classList.contains("shelf-item")).toBe(true);
    expect(waiting.querySelector("[data-compose]")?.getAttribute("aria-label")).toBe("Add a task to Exercise");
    expect(root.querySelector("#task-board .task-group.is-empty")).toBeNull();
    expect(root.querySelectorAll("[data-compose]")).toHaveLength(5);
  });

  it("keeps the shelf to its New type button when every type has tasks", async () => {
    data.taskTypes = {"1": "Coding", "2": "Writing", "3": "Reading"};
    await renderApp(root, client);
    expect(root.querySelector("#task-shelf .type-shelf")?.classList.contains("is-bare")).toBe(true);
    expect(root.querySelector("#task-shelf .shelf-heading")).toBeNull();
    expect(root.querySelector("#task-shelf [data-type-new]")).not.toBeNull();
  });

  it("hides groups without matches while searching", async () => {
    await renderApp(root, client);
    const search = root.querySelector<HTMLInputElement>('[name="search"]')!;
    search.value = "paper"; search.dispatchEvent(new Event("input"));
    expect(names()).toEqual(["Reading"]);
    expect(root.querySelector("#task-shelf")?.innerHTML).toBe("");
    search.value = ""; search.dispatchEvent(new Event("input"));
    expect(names()).toHaveLength(4);
    expect(shelfNames()).toEqual(["Exercise"]);
  });
});

describe("folding", () => {
  it("folds every group into its metrics and remembers the choice", async () => {
    await renderApp(root, client);
    expect(root.querySelector('[data-board-view="expanded"]')?.getAttribute("aria-pressed")).toBe("true");
    click('[data-board-view="folded"]');
    expect(root.querySelector('[data-board-view="folded"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(root.querySelector(".fold-switch")?.classList.contains("is-folded")).toBe(true);
    expect(root.querySelectorAll(".task-group.is-folded")).toHaveLength(4);
    expect(root.querySelector(".task-card")).toBeNull();
    expect(root.querySelector("#task-board")?.classList.contains("is-uniform")).toBe(true);
    const coding = group("Coding");
    expect(coding.querySelector(".ring figcaption")?.textContent).toBe("6.0");
    expect(coding.querySelectorAll(".tg-strip i")).toHaveLength(2);
    expect(coding.querySelector(".tg-share")?.textContent).toContain("40% of open tasks");
    expect(coding.textContent).toContain("1 overdue");
    expect(coding.querySelector(".tg-progress b")?.textContent).toBe("75%");
    expect(coding.querySelector(".tg-progress")?.getAttribute("aria-label")).toBe("Average progress 75%, 1 of 2 tasks at 100%");
    expect(group("Reading").querySelector(".tg-progress b")?.textContent).toBe("0%");
    expect(group("Writing").textContent).toContain("1 deferred");
    expect(group("Reading").textContent).toContain("1 important");
    // The shelf of types without tasks looks the same either way.
    expect(shelfNames()).toEqual(["Exercise"]);
    expect(window.localStorage.getItem("schedule-everything-task-board")).toBe("folded");

    await renderApp(root, client);
    expect(root.querySelectorAll(".task-group.is-folded")).toHaveLength(4);
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
    expect(root.querySelectorAll(".task-group.is-folded")).toHaveLength(3);
    expect(root.querySelector("#task-board")?.classList.contains("is-uniform")).toBe(false);
    expect(document.activeElement?.getAttribute("data-group-toggle")).toBe("1");
    click('[data-group-toggle="1"]');
    expect(root.querySelectorAll(".task-group.is-folded")).toHaveLength(4);
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
    // The new task is greeted and the form waits for another task. The other groups stay where they were, and
    // Exercise joins the board at the end, right above the shelf it came from.
    expect(names()).toEqual(["Reading", "Coding", "Writing", "Retired", "Exercise"]);
    expect(shelfNames()).toEqual([]);
    expect(group("Exercise").querySelector(".task-card.just-added h4")?.textContent).toBe("Morning run");
    expect(form().dataset.composer).toBe("4");
    expect(form().querySelector<HTMLInputElement>('[name="composerText"]')!.value).toBe("");
    expect(form().querySelector<HTMLInputElement>('[name="composerPriority"]')!.value).toBe("8");
    expect(document.activeElement?.getAttribute("name")).toBe("composerText");
    // Opening the page again sorts the groups afresh: Exercise (average 8) comes second, after Reading's 9.
    click('[data-page="settings"]');
    click('[data-page="today"]');
    expect(names().slice(0, 3)).toEqual(["Reading", "Exercise", "Coding"]);
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
    expect(root.querySelector("#notice .notice-text")?.textContent).toBe("Storage is busy");
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

describe("managing types on the board", () => {
  const menu = (id: string) => click(`[data-type-menu="${id}"]`);
  const notice = () => root.querySelector("#notice")?.textContent;

  it("renames a type from its menu, or by double-clicking its name", async () => {
    await renderApp(root, client);
    menu("1");
    expect([...root.querySelectorAll(".type-menu [role='menuitem']")].map(el => el.textContent)).toEqual(["Rename", "Delete type"]);
    expect(document.activeElement?.getAttribute("data-type-rename")).toBe("1");
    click('[data-type-rename="1"]');
    const input = root.querySelector<HTMLInputElement>('[data-type-name-input="1"]')!;
    expect(document.activeElement).toBe(input);
    expect(input.value).toBe("Coding");
    expect(root.querySelector(".type-menu")).toBeNull();
    input.value = "Programming";
    input.dispatchEvent(new KeyboardEvent("keydown", {key: "Enter", bubbles: true}));
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_type_save", {id: "1", name: "Programming"});
    expect(group("Programming").classList.contains("greet")).toBe(true);
    expect(notice()).toContain("Renamed “Coding” to “Programming”.");

    root.querySelector<HTMLElement>('[data-type-title="2"]')!.dispatchEvent(new MouseEvent("dblclick", {bubbles: true}));
    const writing = root.querySelector<HTMLInputElement>('[data-type-name-input="2"]')!;
    writing.value = "Essays";
    writing.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    expect(root.querySelector('[data-type-name-input]')).toBeNull();
    expect(group("Writing")).toBeTruthy();
    expect(send).not.toHaveBeenCalledWith("task_type_save", expect.objectContaining({name: "Essays"}));
  });

  it("closes the type menu with Escape or a press elsewhere", async () => {
    await renderApp(root, client);
    menu("3");
    document.body.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    expect(root.querySelector(".type-menu")).toBeNull();
    expect(document.activeElement?.getAttribute("data-type-menu")).toBe("3");
    menu("3");
    root.querySelector(".page-heading")!.dispatchEvent(new Event("pointerdown", {bubbles: true}));
    expect(root.querySelector(".type-menu")).toBeNull();
    expect(root.querySelector(".group-badge.is-open")).toBeNull();
  });

  it("deletes a type after asking where its tasks go", async () => {
    await renderApp(root, client);
    menu("1");
    click('[data-type-delete="1"]');
    const panel = group("Coding").querySelector<HTMLElement>(".type-delete")!;
    expect(panel.textContent).toContain("Delete “Coding”?");
    expect(panel.textContent).toContain("Its 2 tasks move to");
    expect(group("Coding").querySelector(".task-card")).toBeNull();
    const select = panel.querySelector<HTMLSelectElement>("[data-type-move]")!;
    expect([...select.options].map(option => option.textContent)).toEqual(["Writing", "Reading", "Exercise"]);
    expect(select.value).toBe("2");
    select.value = "3"; select.dispatchEvent(new Event("change"));
    click("[data-type-delete-confirm]");
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_type_delete", {id: "1", moveTo: "3"});
    expect(names()).not.toContain("Coding");
    expect(group("Reading").querySelectorAll(".task-card")).toHaveLength(3);
    expect(notice()).toContain("Deleted the type “Coding”. 2 tasks moved to “Reading”.");
    expect(document.activeElement?.hasAttribute("data-type-new")).toBe(true);
  });

  it("keeps a type when the question is dismissed", async () => {
    await renderApp(root, client);
    menu("4");
    click('[data-type-delete="4"]');
    const panel = group("Exercise").querySelector<HTMLElement>(".type-delete")!;
    expect(panel.textContent).toContain("No tasks or deadlines use it.");
    expect(panel.querySelector("select")).toBeNull();
    click("[data-type-delete-cancel]");
    expect(root.querySelector(".type-delete")).toBeNull();
    expect(document.activeElement?.getAttribute("data-type-menu")).toBe("4");
    menu("4");
    click('[data-type-delete="4"]');
    group("Exercise").querySelector(".type-delete")!.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    expect(root.querySelector(".type-delete")).toBeNull();
    expect(send).not.toHaveBeenCalledWith("task_type_delete", expect.anything());
  });

  it("lets deadlines of a type without tasks lose their type", async () => {
    data.deadlines = [{event: "Marathon", deadline: "2026-10-20", daysLeft: 15, status: "ok", type: "4", typeName: "Exercise"}];
    await renderApp(root, client);
    menu("4");
    click('[data-type-delete="4"]');
    const select = group("Exercise").querySelector<HTMLSelectElement>("[data-type-move]")!;
    expect(group("Exercise").textContent).toContain("Its 1 deadline moves to");
    expect(select.options[0].textContent).toBe("No type");
    expect(select.value).toBe("");
    click("[data-type-delete-confirm]");
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_type_delete", {id: "4"});
  });

  it("keeps at least one type", async () => {
    data.taskTypes = {"1": "Coding"};
    data.tasks = data.tasks.filter(item => item.type === "1");
    await renderApp(root, client);
    menu("1");
    expect(root.querySelector<HTMLButtonElement>('[data-type-delete="1"]')!.disabled).toBe(true);
  });

  it("adds a type from the end of the shelf", async () => {
    await renderApp(root, client);
    expect(root.querySelector("#task-shelf .shelf-items")?.lastElementChild?.classList.contains("new-type")).toBe(true);
    click("[data-type-new]");
    const input = () => root.querySelector<HTMLInputElement>('[name="newTypeName"]')!;
    expect(document.activeElement).toBe(input());
    input().dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    expect(root.querySelector(".new-type-form")).toBeNull();
    expect(document.activeElement?.hasAttribute("data-type-new")).toBe(true);

    click("[data-type-new]");
    root.querySelector(".new-type-form")!.dispatchEvent(new Event("submit", {cancelable: true}));
    expect(root.querySelector(".new-type-form")?.classList.contains("shake")).toBe(true);
    input().value = "Errands";
    input().dispatchEvent(new Event("input", {bubbles: true}));
    root.querySelector(".new-type-form")!.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_type_save", {name: "Errands"});
    // A new type has no tasks yet, so it waits on the shelf, ready for its first one.
    expect(shelfNames()).toEqual(["Exercise", "Errands"]);
    expect(group("Errands").classList.contains("greet")).toBe(true);
    expect(root.querySelector(".new-type-form")).toBeNull();
    expect(notice()).toContain("Added the type “Errands”.");
    expect(document.activeElement?.getAttribute("data-compose")).toBe("5");
  });

  it("leaves types alone while searching", async () => {
    await renderApp(root, client);
    const search = root.querySelector<HTMLInputElement>('[name="search"]')!;
    search.value = "paper"; search.dispatchEvent(new Event("input"));
    expect(root.querySelector(".new-type")).toBeNull();
    expect(root.querySelector("[data-type-menu]")).toBeNull();
    expect(root.querySelector(".task-group .group-count")?.textContent).toBe("1");
  });
});

describe("type colors", () => {
  it("uses saved colors, so deleting a type never recolors the others", async () => {
    data.taskTypeColors = {"1": "violet", "3": "red", "4": "plaid"};
    await renderApp(root, client);
    expect(group("Coding").classList.contains("sw-violet")).toBe(true);
    expect(group("Reading").classList.contains("sw-red")).toBe(true);
    // Without a saved color (or with an unknown one), a type keeps the color of its place: Writing is second.
    expect(group("Writing").classList.contains("sw-orange")).toBe(true);
    expect(group("Exercise").classList.contains("sw-magenta")).toBe(false);
    expect(group("Exercise").classList.contains("sw-yellow")).toBe(true);
    delete data.taskTypes["2"];
    data.tasks = data.tasks.filter(item => item.type !== "2");
    await renderApp(root, client);
    expect(group("Reading").classList.contains("sw-red")).toBe(true);
  });
});

describe("keeping the board in place", () => {
  const compose = (name: string) => group(name).querySelector<HTMLButtonElement>("[data-compose]")!.click();
  const add = async (text: string, priority: number) => {
    const form = root.querySelector<HTMLFormElement>("form.composer")!;
    form.querySelector<HTMLInputElement>('[name="composerText"]')!.value = text;
    const range = form.querySelector<HTMLInputElement>('[name="composerPriority"]')!;
    range.value = String(priority); range.dispatchEvent(new Event("input", {bubbles: true}));
    form.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush(); await flush();
  };

  it("leaves every group in its place while the page stays open, and sorts them again on Refresh", async () => {
    await renderApp(root, client);
    compose("Retired");
    await add("Renew passport", 10);
    // Retired now averages 6 with two tasks, which would put it second, but nothing moves under the cursor.
    expect(names()).toEqual(["Reading", "Coding", "Writing", "Retired"]);
    expect(document.activeElement?.getAttribute("name")).toBe("composerText");
    click('[data-action="refresh"]');
    await flush(); await flush();
    expect(names()).toEqual(["Reading", "Coding", "Retired", "Writing"]);
  });

  it("keeps a finished group on the board until the page is opened again", async () => {
    await renderApp(root, client);
    group("Reading").querySelector<HTMLButtonElement>('[data-operation="deleted"]')!.click();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("task_delete", {description: "Read paper", action: "deleted"});
    expect(names()).toEqual(["Reading", "Coding", "Writing", "Retired"]);
    expect(group("Reading").classList.contains("is-cleared")).toBe(true);
    expect(group("Reading").textContent).toContain("All done here.");
    expect(group("Reading").querySelector("[data-compose]")).not.toBeNull();
    expect(shelfNames()).toEqual(["Exercise"]);
    click('[data-page="activity"]');
    click('[data-page="today"]');
    expect(names()).toEqual(["Coding", "Writing", "Retired"]);
    // The shelf lists types in their configured order.
    expect(shelfNames()).toEqual(["Reading", "Exercise"]);
  });

  it("orders the board as the page opens, then keeps that order", () => {
    expect(boardOrder(snapshot)).toEqual(["3", "1", "2", "9"]);
    // Spread puts the heaviest workload first: Reading 0.9, Writing 0.6, Coding 0.3 (one task done, one half), Retired 0.2.
    expect(boardOrder(snapshot, "spread")).toEqual(["3", "2", "1", "9"]);
    const later = structuredClone(snapshot);
    later.tasks = later.tasks.filter(item => item.type !== "3");
    later.tasks.push(task("Stretch", 10, "4"));
    // Reading keeps its place though its task is done; Exercise, new to the board, joins at the end.
    expect(boardGroups(later, ["3", "1", "2", "9"]).map(item => item.name)).toEqual(["Reading", "Coding", "Writing", "Retired", "Exercise"]);
    expect(boardGroups(later, null).map(item => item.name)).toEqual(["Exercise", "Coding", "Writing", "Retired"]);
  });
});

describe("subtasks", () => {
  const card = (name: string) => [...root.querySelectorAll<HTMLElement>("#board-area .task-card")].find(el => el.querySelector("h4")?.textContent === name)!;
  const field = () => root.querySelector<HTMLInputElement>('#board-area form.subtask-form [name="subtaskText"]')!;
  const write = (text: string) => {
    field().value = text;
    field().dispatchEvent(new Event("input", {bubbles: true}));
    field().form!.dispatchEvent(new Event("submit", {cancelable: true}));
  };
  const steps = (name: string) => [...card(name).querySelectorAll(".subtask-text")].map(el => el.textContent);
  const shown = (name: string) => card(name).querySelector(".task-progress-row .progress-control b")?.textContent;
  const settle = () => new Promise(resolve => setTimeout(resolve, 300));

  it("breaks a task into subtasks, whose completion sets its progress", async () => {
    await renderApp(root, client);
    expect(card("Read paper").querySelector("input[data-progress]")).not.toBeNull();
    card("Read paper").querySelector<HTMLButtonElement>("[data-subtask-open]")!.click();
    expect(document.activeElement).toBe(field());
    write("Skim the abstract");
    // The field stays open, empty, for the next one; the slider gives way to a meter that follows the subtasks.
    expect(document.activeElement).toBe(field());
    expect(field().value).toBe("");
    write("Take notes");
    expect(steps("Read paper")).toEqual(["Skim the abstract", "Take notes"]);
    expect(card("Read paper").querySelector("input[data-progress]")).toBeNull();
    expect(shown("Read paper")).toBe("0%");
    const box = card("Read paper").querySelector<HTMLInputElement>('input[data-subtask][data-step="0"]')!;
    box.checked = true; box.dispatchEvent(new Event("change"));
    expect(shown("Read paper")).toBe("50%");
    expect(card("Read paper").querySelector(".progress-meter")?.getAttribute("aria-valuetext")).toBe("50%, 1 of 2 subtasks done");
    expect(card("Read paper").querySelector(".subtask.is-done .subtask-text")?.textContent).toBe("Skim the abstract");
    expect(document.activeElement?.getAttribute("data-step")).toBe("0");
    // The saves wait for the clicks to settle and send the whole list once.
    expect(send).not.toHaveBeenCalledWith("task_subtasks", expect.anything());
    await settle();
    expect(send.mock.calls.filter(([command]) => command === "task_subtasks")).toHaveLength(1);
    expect(send).toHaveBeenCalledWith("task_subtasks", {description: "Read paper", subtasks: [{text: "Skim the abstract", done: true}, {text: "Take notes", done: false}]});
    // A redraw from the server keeps them, and Escape closes the field.
    await renderApp(root, client);
    expect(steps("Read paper")).toEqual(["Skim the abstract", "Take notes"]);
    expect(shown("Read paper")).toBe("50%");
  });

  it("closes the field with Escape and ignores empty subtasks", async () => {
    await renderApp(root, client);
    card("Draft essay").querySelector<HTMLButtonElement>("[data-subtask-open]")!.click();
    write("  ");
    expect(field().form!.classList.contains("shake")).toBe(true);
    field().dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    expect(root.querySelector("form.subtask-form")).toBeNull();
    expect(document.activeElement?.getAttribute("data-subtask-open")).toBe(card("Draft essay").querySelector("[data-subtask-open]")?.getAttribute("data-subtask-open"));
    await settle();
    expect(send).not.toHaveBeenCalledWith("task_subtasks", expect.anything());
  });

  it("removes a subtask, and Undo brings it back", async () => {
    data.tasks[3].subtasks = [{text: "Find the PDF", done: true}, {text: "Read it", done: false}];
    await renderApp(root, client);
    expect(shown("Read paper")).toBe("50%");
    card("Read paper").querySelector<HTMLButtonElement>('[data-subtask-remove][data-step="0"]')!.click();
    expect(steps("Read paper")).toEqual(["Read it"]);
    expect(shown("Read paper")).toBe("0%");
    expect(root.querySelector("#notice .notice-text")?.textContent).toBe("Removed the subtask “Find the PDF”.");
    root.querySelector<HTMLButtonElement>("[data-notice-undo]")!.click();
    await flush();
    expect(steps("Read paper")).toEqual(["Find the PDF", "Read it"]);
    expect(shown("Read paper")).toBe("50%");
    await settle();
    expect(send).toHaveBeenLastCalledWith("task_subtasks", {description: "Read paper", subtasks: [{text: "Find the PDF", done: true}, {text: "Read it", done: false}]});
  });

  it("rewords a subtask in place, and Undo puts the old wording back", async () => {
    data.tasks[3].subtasks = [{text: "Find the PDF", done: true}, {text: "Raed it", done: false}];
    await renderApp(root, client);
    const editor = () => card("Read paper").querySelector<HTMLInputElement>('form[data-subtask-edit] [name="subtaskEdit"]');
    card("Read paper").querySelector<HTMLButtonElement>('[data-subtask-edit-open][data-step="1"]')!.click();
    // The row becomes a field holding the subtask's wording, ready to type over.
    expect(document.activeElement).toBe(editor());
    expect(editor()!.value).toBe("Raed it");
    expect(card("Read paper").querySelector(".subtask.is-editing [data-subtask]")).toBeNull();
    // A redraw while it is open keeps what was typed.
    editor()!.value = "Read it"; editor()!.dispatchEvent(new Event("input", {bubbles: true}));
    const first = card("Read paper").querySelector<HTMLInputElement>('input[data-subtask][data-step="0"]')!;
    first.checked = false; first.dispatchEvent(new Event("change"));
    expect(editor()!.value).toBe("Read it");
    editor()!.form!.dispatchEvent(new Event("submit", {cancelable: true}));
    expect(editor()).toBeNull();
    expect(steps("Read paper")).toEqual(["Find the PDF", "Read it"]);
    // Rewording does not change how far along the task is.
    expect(shown("Read paper")).toBe("0%");
    expect(document.activeElement?.getAttribute("data-subtask-edit-open")).toBe("3");
    expect(root.querySelector("#notice .notice-text")?.textContent).toBe("Renamed the subtask “Raed it” to “Read it”.");
    await settle();
    expect(send).toHaveBeenLastCalledWith("task_subtasks", {description: "Read paper", subtasks: [{text: "Find the PDF", done: false}, {text: "Read it", done: false}]});
    root.querySelector<HTMLButtonElement>("[data-notice-undo]")!.click();
    await flush();
    expect(steps("Read paper")).toEqual(["Find the PDF", "Raed it"]);
    await settle();
    expect(send).toHaveBeenLastCalledWith("task_subtasks", {description: "Read paper", subtasks: [{text: "Find the PDF", done: false}, {text: "Raed it", done: false}]});
  });

  it("keeps a subtask as it was on Escape, and will not empty it", async () => {
    data.tasks[3].subtasks = [{text: "Read it", done: false}];
    await renderApp(root, client);
    const editor = () => card("Read paper").querySelector<HTMLInputElement>('form[data-subtask-edit] [name="subtaskEdit"]');
    card("Read paper").querySelector<HTMLButtonElement>("[data-subtask-edit-open]")!.click();
    editor()!.value = "   ";
    editor()!.form!.dispatchEvent(new Event("submit", {cancelable: true}));
    expect(editor()!.form!.classList.contains("shake")).toBe(true);
    editor()!.dispatchEvent(new KeyboardEvent("keydown", {key: "Escape", bubbles: true}));
    expect(editor()).toBeNull();
    expect(steps("Read paper")).toEqual(["Read it"]);
    expect(document.activeElement?.getAttribute("data-subtask-edit-open")).toBe("3");
    // Opening the field for a new subtask afterwards starts it empty.
    card("Read paper").querySelector<HTMLButtonElement>("[data-subtask-open]")!.click();
    expect(field().value).toBe("");
    await settle();
    expect(send).not.toHaveBeenCalledWith("task_subtasks", expect.anything());
  });

  it("sums subtasks up in folded groups and offers the next one on the timer", async () => {
    data.tasks[1].subtasks = [{text: "Write tests", done: true}, {text: "Fix the edge case", done: false}];
    await renderApp(root, client);
    const timer = root.querySelector<HTMLElement>("#timer-strip .timer-task")!;
    expect(timer.querySelector(".timer-steps-count")?.textContent).toBe("1/2");
    expect(timer.querySelector(".timer-next-text")?.textContent).toBe("NextFix the edge case");
    const next = timer.querySelector<HTMLInputElement>("input[data-subtask]")!;
    next.checked = true; next.dispatchEvent(new Event("change"));
    expect(root.querySelector("#timer-strip .timer-next-done")?.textContent).toBe("Every subtask is done");
    expect(shown("Refactor parser")).toBe("100%");
    expect(card("Refactor parser").classList.contains("is-full")).toBe(true);
    click('[data-board-view="folded"]');
    expect(group("Coding").querySelector(".tg-chip.steps")?.textContent).toBe("2/2 subtasks");
    // Fix build is 100% and Refactor parser now too.
    expect(group("Coding").querySelector(".tg-progress b")?.textContent).toBe("100%");
    await settle();
    expect(send).toHaveBeenCalledWith("task_subtasks", {description: "Refactor parser", subtasks: [{text: "Write tests", done: true}, {text: "Fix the edge case", done: true}]});
  });

  it("saves a waiting change before the task is completed", async () => {
    data.tasks[3].subtasks = [{text: "Read it", done: false}];
    await renderApp(root, client);
    const box = card("Read paper").querySelector<HTMLInputElement>("input[data-subtask]")!;
    box.checked = true; box.dispatchEvent(new Event("change"));
    card("Read paper").querySelector<HTMLButtonElement>('[data-operation="deleted"]')!.click();
    await flush(); await flush(); await flush();
    const commands = send.mock.calls.map(([command]) => command).filter(command => command === "task_subtasks" || command === "task_delete");
    expect(commands).toEqual(["task_subtasks", "task_delete"]);
  });
});

describe("spread layout", () => {
  it("places groups by workload and remembers the choice", async () => {
    await renderApp(root, client);
    expect(root.querySelector("#task-board.spread-board")).toBeNull();
    click('[data-board-layout="spread"]');
    expect(root.querySelector('[data-board-layout="spread"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(root.querySelector("#task-board.spread-board")).not.toBeNull();
    expect(root.querySelector(".spread-legend")?.textContent).toContain("the heaviest group sits in the middle");
    expect(names()).toEqual(["Reading", "Writing", "Coding", "Retired"]);
    expect([...root.querySelectorAll(".task-group .group-load b")].map(el => el.textContent)).toEqual(["0.9", "0.6", "0.3", "0.2"]);
    expect(group("Reading").dataset.load).toBe("0.9");
    expect(JSON.parse(window.localStorage.getItem("schedule-everything-display")!).layout).toBe("spread");
    // Folded groups keep their places instead of lining up in rows.
    click('[data-board-view="folded"]');
    expect(root.querySelector("#task-board")?.classList.contains("is-uniform")).toBe(false);

    await renderApp(root, client);
    expect(root.querySelector("#task-board.spread-board")).not.toBeNull();
    click('[data-board-layout="compact"]');
    expect(root.querySelector(".group-load")).toBeNull();
    expect(names()).toEqual(["Reading", "Coding", "Writing", "Retired"]);
  });
});

describe("suggestions from agents", () => {
  const offer = (id: string, description: string, type: string, extra: Partial<SuggestionItem> = {}): SuggestionItem =>
    ({id, description, priority: 7, type, typeName: null, agent: "Claude", source: "Gmail", reason: "Lee asked for comments by Thursday.", ...extra});
  const suggestion = (id: string) => root.querySelector<HTMLElement>(`.suggestion[data-suggestion="${id}"]`);
  const titles = (name: string) => [...group(name).querySelectorAll(".task-card .task-title")].map(el => el.textContent);
  const notice = () => root.querySelector("#notice .notice-text")?.textContent;
  let decided: SuggestionItem[];

  beforeEach(() => {
    decided = [];
    data.suggestions = [offer("1", "Reply to Lee", "2"), offer("2", "Stretch daily", "4", {priority: 4, source: null, reason: null})];
    const base = send.getMockImplementation()!;
    send.mockImplementation(async (command: string, payload: Record<string, unknown>) => {
      const items = data.suggestions ?? [];
      const item = items.find(entry => entry.id === payload.id);
      if (command === "suggestion_accept" && item) {
        data.suggestions = items.filter(entry => entry !== item);
        decided.push(item);
        const added = task(String(payload.description ?? item.description), Number(payload.priority ?? item.priority), String(payload.type ?? item.type));
        data.tasks.push(added);
        return {task: {description: added.description, type: added.type}, undo: {id: item.id, logged: ["stamp"]}};
      }
      if (command === "suggestion_reject" && item) {
        data.suggestions = items.filter(entry => entry !== item);
        decided.push(item);
        return {id: item.id, description: item.description, undo: {id: item.id}};
      }
      if (command === "suggestion_undo") {
        const back = decided.find(entry => entry.id === payload.id)!;
        data.tasks = data.tasks.filter(entry => entry.description !== back.description);
        data.suggestions = [...(data.suggestions ?? []), back];
        return {id: back.id};
      }
      return base(command, payload);
    });
  });

  it("waits in its type's group, drawn apart from the tasks and left out of every count", async () => {
    await renderApp(root, client);
    const writing = group("Writing");
    const card = suggestion("1")!;
    expect(writing.contains(card)).toBe(true);
    expect(card.classList.contains("task-card")).toBe(false);
    expect(card.querySelector(".suggestion-from")?.textContent).toBe("Suggested by Claude · from Gmail");
    expect(card.querySelector(".suggestion-title")?.textContent).toBe("Reply to Lee");
    expect(card.querySelector(".suggestion-reason")?.textContent).toBe("Lee asked for comments by Thursday.");
    expect(card.querySelector(".priority")?.textContent).toContain("Importance 7/10");
    expect(writing.querySelector(".suggestion-label")?.textContent).toContain("Suggested for you");
    // The group's tasks, its count, the sidebar, and its metrics are the user's own tasks only.
    expect(titles("Writing")).toEqual(["Draft essay"]);
    expect(writing.querySelector(".group-count")?.textContent).toBe("1");
    expect(root.querySelector(".nav-count")?.textContent).toBe("5");
    expect(groupTasks(data).find(g => g.name === "Writing")).toMatchObject({average: 6, high: 0});
    expect(root.querySelector(".suggestion-pill")?.textContent).toBe("2 suggested");
    // A type with only a suggestion comes up from the shelf to show it, without a count or an "all done".
    expect(names()).toContain("Exercise");
    expect(shelfNames()).toEqual([]);
    const exercise = group("Exercise");
    expect(exercise.querySelector(".suggestion-list")?.classList.contains("is-alone")).toBe(true);
    expect(exercise.querySelector(".group-count")).toBeNull();
    expect(exercise.querySelector(".group-cleared")).toBeNull();
    expect(suggestion("2")?.querySelector(".suggestion-from")?.textContent).toBe("Suggested by Claude");
    expect(suggestion("2")?.querySelector(".suggestion-reason")).toBeNull();
  });

  it("joins the tasks when added, and Undo sends it back for review", async () => {
    await renderApp(root, client);
    suggestion("1")!.querySelector<HTMLButtonElement>("[data-suggestion-accept]")!.click();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("suggestion_accept", {id: "1"});
    expect(titles("Writing")).toEqual(["Draft essay", "Reply to Lee"]);
    expect(suggestion("1")).toBeNull();
    expect(notice()).toBe("Added “Reply to Lee” to Writing. It’s one of your tasks now.");
    expect(root.querySelector(".suggestion-pill")?.textContent).toBe("1 suggested");
    root.querySelector<HTMLButtonElement>("[data-notice-undo]")!.click();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("suggestion_undo", {id: "1", logged: ["stamp"]});
    expect(titles("Writing")).toEqual(["Draft essay"]);
    expect(suggestion("1")).not.toBeNull();
    expect(notice()).toBe("“Reply to Lee” is waiting for your review again.");
  });

  it("leaves the board when rejected, and Undo brings it back", async () => {
    await renderApp(root, client);
    suggestion("2")!.querySelector<HTMLButtonElement>("[data-suggestion-reject]")!.click();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("suggestion_reject", {id: "2"});
    expect(suggestion("2")).toBeNull();
    expect(data.tasks.some(entry => entry.description === "Stretch daily")).toBe(false);
    expect(notice()).toBe("Rejected “Stretch daily”. It stays off your list.");
    root.querySelector<HTMLButtonElement>("[data-notice-undo]")!.click();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("suggestion_undo", {id: "2"});
    expect(suggestion("2")).not.toBeNull();
  });

  it("can be adjusted before it is added", async () => {
    HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
    await renderApp(root, client);
    suggestion("1")!.querySelector<HTMLButtonElement>("[data-suggestion-adjust]")!.click();
    const dialog = root.querySelector<HTMLDialogElement>("dialog.editor-dialog")!;
    expect(dialog.querySelector("h2")?.textContent).toBe("Adjust, then add it");
    expect(dialog.querySelector(".dialog-suggestion")?.textContent).toBe("Suggested by Claude from Gmail: Lee asked for comments by Thursday.");
    expect(dialog.querySelector<HTMLInputElement>('[name="description"]')?.value).toBe("Reply to Lee");
    expect(dialog.querySelector<HTMLSelectElement>('[name="type"]')?.value).toBe("2");
    expect(dialog.querySelector('[type="submit"]')?.textContent).toBe("Add to my tasks");
    dialog.querySelector<HTMLInputElement>('[name="description"]')!.value = "Reply to Lee about the draft";
    dialog.querySelector<HTMLInputElement>('[name="priority"]')!.value = "9";
    dialog.querySelector<HTMLSelectElement>('[name="type"]')!.value = "1";
    dialog.querySelector("form")!.dispatchEvent(new Event("submit", {cancelable: true}));
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("suggestion_accept", {description: "Reply to Lee about the draft", priority: 9, type: "1", due: "", id: "1"});
    expect(titles("Coding")).toContain("Reply to Lee about the draft");
    expect(root.querySelector("dialog.editor-dialog")).toBeNull();
  });

  it("is only counted in a folded group, which opens to show it", async () => {
    window.localStorage.setItem("schedule-everything-task-board", "folded");
    await renderApp(root, client);
    const note = group("Writing").querySelector<HTMLButtonElement>(".suggestion-note")!;
    expect(note.textContent).toContain("1 suggested by Claude");
    expect(suggestion("1")).toBeNull();
    // A group with nothing but suggestions has no summary to fold into, so it shows them.
    expect(group("Exercise").classList.contains("is-folded")).toBe(false);
    expect(suggestion("2")).not.toBeNull();
    note.click();
    expect(group("Writing").classList.contains("is-folded")).toBe(false);
    expect(suggestion("1")).not.toBeNull();
  });

  it("is found by search like a task", async () => {
    await renderApp(root, client);
    const search = root.querySelector<HTMLInputElement>('[name="search"]')!;
    search.value = "stretch"; search.dispatchEvent(new Event("input"));
    expect(names()).toEqual(["Exercise"]);
    expect(suggestion("2")).not.toBeNull();
    expect(root.querySelector(".task-card")).toBeNull();
  });

  it("keeps groups with only suggestions in the board order", () => {
    expect(boardOrder(data)).toEqual(["3", "1", "2", "9", "4"]);
    expect(boardGroups(data, null).map(g => g.id)).toEqual(["3", "1", "2", "9", "4"]);
    expect(boardGroups(data, ["3", "1"]).map(g => g.id)).toEqual(["3", "1", "2", "9", "4"]);
  });
});
