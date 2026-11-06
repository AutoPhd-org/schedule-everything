// Browser workspace shell: application state, page layouts, and event binding.
// J mode shows the full structured overview; P mode keeps only the tasks-by-type board.
import { defaultGalleryFilter, filterHistory, galleryGroups, galleryView, lightbox, peekCard, weeklyView, type GalleryFilter, type HistoryLayout, type HistoryStatus, type HistorySort } from "./activity";
import { foldSwitch, groupTasks, masonry, taskBoard, type BoardView, type Composer } from "./board";
import { conferenceSwatch, deadlineSwatch, repeatBadge, repeatOptions, yearCalendar, type CalendarState, type ConferenceView } from "./calendar";
import { ring } from "./charts";
import { bindTilt, burst, reducedMotion } from "./effects";
import { bindPeek, hidePeek } from "./peek";
import { applyTheme, setThemePreference, themePreference, type ThemePreference } from "./theme";
import { bindTooltips } from "./tooltip";
import { bindTree, scrollTreeTo, treeView } from "./tree";
import type { BridgeClient, ConferencePayload, DeadlineItem, HistoryItem, Snapshot, TaskItem, WeeklyOverview, Workspace } from "./types";
import { addDays, button, e, empty, escapeHtml, field, formatDate, icons, isoDay, isSwatch, setTypeColors, sw, swatchPicker, typeSwatch } from "./ui";

export { escapeHtml, groupTasks };

type Page = "today" | "schedule" | "routines" | "activity" | "settings";
type Mode = "j" | "p";
type ConfigFile = {file: string; content: string; revision: string; activeId: number};
type ActivityState = {view: "gallery" | "week"; layout: HistoryLayout; items: HistoryItem[] | null; summary: WeeklyOverview | null; week: WeeklyOverview | null; filter: GalleryFilter; limit: number; treeLimit: number; loading: boolean};
/** The task board: expanded or folded, groups toggled away from that view, the inline new-task form, the task
 *  to greet after it was added, and whether to put the cursor back in the form after the next draw. Types are
 *  managed here too: the open type menu, the type being renamed or deleted, the new-type form, and the group
 *  to greet after its type was added or renamed. */
type BoardState = {
  view: BoardView; flipped: Set<string>; composer: Composer | null; added: string | null; focus: boolean;
  menu: string | null; renaming: string | null; deleting: {id: string; moveTo: string} | null; newType: {draft: string} | null; greet: string | null;
};
type AgentTab = "claude" | "codex" | "antigravity";
/** A way to take back what a notice reports. It belongs to that notice: once the notice is replaced or put
 *  away, or anything else is done, the chance to undo is gone. */
type Undo = {notice: string; run: () => Promise<void>};
type State = {page: Page; snapshot: Snapshot | null; workspace: Workspace | null; agentTab: AgentTab; search: string; busy: boolean; notice: string; undo: Undo | null; config: ConfigFile | null; activity: ActivityState; calendar: CalendarState; conferences: ConferenceView; board: BoardState};

const CONFERENCE_PREFS = "schedule-everything-conferences";
/** Which AI conferences to show is a per-browser preference. */
function loadConferencePrefs(): Pick<ConferenceView, "enabled" | "selected"> {
  try {
    const saved = JSON.parse(window.localStorage.getItem(CONFERENCE_PREFS) ?? "null");
    if (saved && typeof saved === "object") return {enabled: saved.enabled === true, selected: Array.isArray(saved.selected) ? saved.selected.filter((id: unknown) => typeof id === "string") : null};
  } catch { /* Fall back to defaults when storage is unavailable. */ }
  return {enabled: false, selected: null};
}
function saveConferencePrefs(view: ConferenceView): void {
  try { window.localStorage.setItem(CONFERENCE_PREFS, JSON.stringify({enabled: view.enabled, selected: view.selected})); } catch { /* Keep working without storage. */ }
}
const HISTORY_LAYOUT = "schedule-everything-history-layout";
/** Tree or gallery for the activity history is also a per-browser preference; the tree is the default. */
function loadHistoryLayout(): HistoryLayout {
  try { return window.localStorage.getItem(HISTORY_LAYOUT) === "gallery" ? "gallery" : "tree"; } catch { return "tree"; }
}
function saveHistoryLayout(layout: HistoryLayout): void {
  try { window.localStorage.setItem(HISTORY_LAYOUT, layout); } catch { /* Keep working without storage. */ }
}
const BOARD_VIEW = "schedule-everything-task-board";
/** Whether the task board is expanded or folded is a per-browser preference too; expanded is the default. */
function loadBoardView(): BoardView {
  try { return window.localStorage.getItem(BOARD_VIEW) === "folded" ? "folded" : "expanded"; } catch { return "expanded"; }
}
function saveBoardView(view: BoardView): void {
  try { window.localStorage.setItem(BOARD_VIEW, view); } catch { /* Keep working without storage. */ }
}

const PAGES: [Page, string][] = [["today", "Overview"], ["schedule", "Schedule"], ["routines", "Routines & deadlines"], ["activity", "Activity"], ["settings", "Settings"]];
const GALLERY_PAGE = 60;
const TREE_PAGE = 300;
const REPOSITORY = "AutoPhd-org/schedule-everything";
const AGENT_SETUP: Record<AgentTab, {name: string; steps: string[]; note: string}> = {
  claude: {name: "Claude Code", steps: [`claude plugin marketplace add ${REPOSITORY}`, "claude plugin install schedule-everything@schedule-everything"], note: "Inside a Claude Code session you can run the same steps as /plugin marketplace add and /plugin install."},
  codex: {name: "Codex", steps: [`codex plugin marketplace add ${REPOSITORY}`, "codex plugin add schedule-everything@schedule-everything"], note: "The plugin also appears in the ChatGPT desktop app once the marketplace is added."},
  antigravity: {name: "Antigravity", steps: [`git clone https://github.com/${REPOSITORY}.git`, "agy plugin install ./schedule-everything/plugins/schedule-everything"], note: "Antigravity installs the plugin from the cloned folder."}
};
const AGENT_PROMPTS = ["Build my weekly schedule", "Plan my focus blocks for today", "Move my workouts to the evenings"];
const typeOptions = (snapshot: Snapshot, selected = "") => Object.entries(snapshot.taskTypes).map(([id, name]) => `<option value="${e(id)}" ${id === selected ? "selected" : ""}>${e(name)}</option>`).join("");
const isOverview = (value: unknown): value is WeeklyOverview => !!value && Array.isArray((value as WeeklyOverview).days) && Array.isArray((value as WeeklyOverview).trend);
const undoKey = () => /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent) ? "⌘Z" : "Ctrl+Z";

/** Run a DOM update as a view transition, so boxes that change size or place morph instead of jumping.
 *  Browsers without view transitions, and people who prefer reduced motion, get the update at once. */
function morph(update: () => void): void {
  if (typeof document.startViewTransition !== "function" || reducedMotion()) { update(); return; }
  const html = document.documentElement;
  html.dataset.vt = "board";
  try {
    const transition = document.startViewTransition(update);
    // A page that is not being painted (a background tab) never reaches the update; apply it anyway.
    const fallback = window.setTimeout(() => transition.skipTransition(), 400);
    const updated = () => window.clearTimeout(fallback);
    const done = () => { delete html.dataset.vt; };
    transition.updateCallbackDone.then(updated, updated);
    transition.ready.catch(() => {});
    transition.finished.then(done, done);
  } catch {
    delete html.dataset.vt;
    update();
  }
}

type RootWithAbort = HTMLElement & {scheduleAbort?: AbortController};

export async function renderApp(root: HTMLElement, client: BridgeClient): Promise<void> {
  (root as RootWithAbort).scheduleAbort?.abort();
  const controller = new AbortController();
  (root as RootWithAbort).scheduleAbort = controller;
  const {signal} = controller;
  let appearance = themePreference();
  applyTheme(appearance);
  const state: State = {
    page: "today", snapshot: null, workspace: null, agentTab: "claude", search: "", busy: false, notice: "", undo: null, config: null,
    activity: {view: "gallery", layout: loadHistoryLayout(), items: null, summary: null, week: null, filter: defaultGalleryFilter(), limit: GALLERY_PAGE, treeLimit: TREE_PAGE, loading: false},
    calendar: {year: new Date().getFullYear(), day: null, filter: null, flash: null},
    conferences: {...loadConferencePrefs(), menuOpen: false, data: null, loading: false},
    board: {view: loadBoardView(), flipped: new Set(), composer: null, added: null, focus: false, menu: null, renaming: null, deleting: null, newType: null, greet: null}
  };
  bindTooltips(signal);
  let drawn: {page: Page; mode: Mode} | null = null;
  root.innerHTML = `<div class="loading"><span class="loading-blob">${icons.sprout}</span><p>Opening your workspace…</p></div>`;
  const request = async <T>(command: string, payload: Record<string, unknown> = {}): Promise<T> => client.send<T>(command, payload);
  const modeOf = (): Mode => state.workspace?.mode === "p" ? "p" : "j";

  /** Fetch and redraw. `morph` animates the change, so groups glide into place when a type comes or goes;
   *  `then` runs right after the redraw. */
  async function reload(options: {morph?: boolean; then?: () => void} = {}) {
    state.snapshot = await request<Snapshot>("status_snapshot");
    state.workspace = await request<Workspace>("workspace_info");
    setTypeColors(state.snapshot.taskTypeColors);
    if (!drawn) state.calendar.year = Number(state.snapshot.today.date.slice(0, 4)) || state.calendar.year;
    if (state.page === "activity") await loadActivity();
    const update = () => { draw(); options.then?.(); };
    if (options.morph) morph(update); else update();
  }
  async function loadActivity() {
    const [history, summary] = await Promise.all([
      request<{activities?: HistoryItem[]}>("task_history", {all: true}),
      request<WeeklyOverview>("weekly_overview", {})
    ]);
    const activity = state.activity;
    activity.items = Array.isArray(history?.activities) ? history.activities : [];
    activity.summary = isOverview(summary) ? summary : null;
    if (activity.week?.isCurrentWeek || (activity.view === "week" && !activity.week)) activity.week = activity.summary;
  }
  async function refreshActivity() {
    if (state.activity.loading) return;
    const firstLoad = !state.activity.items;
    state.activity.loading = true;
    try { await loadActivity(); }
    catch (error) { state.notice = error instanceof Error ? error.message : String(error); }
    finally { state.activity.loading = false; }
    if (state.page === "activity" && root.isConnected) draw(firstLoad);
  }
  async function action(work: () => Promise<void>) {
    if (state.busy) return;
    state.busy = true;
    state.notice = "";
    state.undo = null;
    status();
    // Only buttons disabled here are re-enabled, so intentionally disabled controls stay disabled.
    root.querySelectorAll<HTMLButtonElement>("button:not(:disabled)").forEach(btn => { btn.disabled = true; btn.dataset.busy = ""; });
    root.querySelectorAll<HTMLInputElement>("[data-habit]").forEach(input => input.disabled = true);
    try { await work(); }
    catch (error) { state.notice = error instanceof Error ? error.message : String(error); }
    finally { state.busy = false; root.querySelectorAll<HTMLButtonElement>("button[data-busy]").forEach(btn => { btn.disabled = false; delete btn.dataset.busy; }); root.querySelectorAll<HTMLInputElement>("[data-habit]").forEach(input => input.disabled = false); status(); }
  }
  function status() {
    const region = root.querySelector<HTMLElement>("#notice");
    if (!region) return;
    const text = state.busy ? "Working on it…" : state.notice;
    region.querySelector(".notice-text")!.textContent = text;
    region.querySelector<HTMLButtonElement>("[data-notice-undo]")!.hidden = !undoable();
    region.classList.remove("leaving");
    region.classList.toggle("visible", !!text);
    region.classList.toggle("busy", state.busy);
  }
  /** Let the notice slide away; a newer notice that arrives meanwhile replaces it. */
  function dismissNotice() {
    const region = root.querySelector<HTMLElement>("#notice");
    if (state.busy || !state.notice || !region) return;
    state.notice = "";
    state.undo = null;
    if (reducedMotion()) { status(); return; }
    region.style.setProperty("--h", `${region.offsetHeight}px`);
    region.classList.add("leaving");
    const finish = () => { if (region.classList.contains("leaving") && !state.notice && !state.busy) status(); };
    region.addEventListener("animationend", event => { if (event.target === region) finish(); });
    window.setTimeout(finish, 520);
  }
  /** Show a notice with an Undo button beside it. */
  function offerUndo(notice: string, run: () => Promise<void>) {
    state.notice = notice;
    state.undo = {notice, run};
  }
  const undoable = () => !state.busy && !!state.undo && state.undo.notice === state.notice;
  function undo() {
    if (!undoable()) return;
    const {run} = state.undo!;
    void action(run);
  }
  function navigate(page: Page) {
    state.page = page;
    Object.assign(state.board, {composer: null, menu: null, renaming: null, deleting: null, newType: null});
    state.calendar.day = null;
    if (page === "activity") state.activity.view = "gallery";
    draw();
    scrollTop();
    if (page === "activity") void refreshActivity();
  }

  function draw(animate = false) {
    const s = state.snapshot!;
    const mode = modeOf();
    hidePeek();
    document.documentElement.dataset.mode = mode;
    animate ||= !drawn || drawn.page !== state.page || drawn.mode !== mode;
    drawn = {page: state.page, mode};
    const title = PAGES.find(([id]) => id === state.page)![1];
    const page = state.page === "today" ? (mode === "p" ? overviewP(s) : overviewJ(s)) : state.page === "schedule" ? schedulePage(s) : state.page === "routines" ? routines(s) : state.page === "activity" ? activity(s) : settings(s);
    root.innerHTML = `<div class="workspace mode-${mode}"><aside class="sidebar"><a class="brand" href="#" aria-label="Schedule Everything overview"><span class="brand-mark">${icons.sprout}</span><span>schedule<span class="brand-sub">everything</span></span></a><div class="workspace-label">YOUR WORKSPACE</div>
      <nav aria-label="Main navigation">${PAGES.map(([id, label]) => `<button type="button" data-page="${id}" title="${e(label)}" aria-current="${state.page === id ? "page" : "false"}" class="nav-item ${state.page === id ? "active" : ""}"><span class="nav-icon">${icons[id]}</span><span class="nav-label">${label}</span>${id === "today" ? `<span class="nav-count">${s.tasks.length}</span>` : ""}</button>`).join("")}</nav>
      <div class="sidebar-bottom"><div class="mode-note mode-${mode}">${mode === "p" ? icons.feather : icons.target}<div><strong>${mode === "p" ? "P mode" : "J mode"}</strong><span>${mode === "p" ? "Flexible, task-first" : "Structured day"}</span></div></div><span class="local-dot"></span> Saved on your computer<p>One day, one thing at a time.</p></div></aside>
      <main class="main"><header class="topbar"><span class="breadcrumb">Workspace ${icons.arrow} <strong>${title}</strong></span>${modeSwitch(mode)}<span class="date">${icons.today}${e(formatDate(s.today.date, {weekday: "short", month: "short", day: "numeric"}))}</span>${themeControls()}${button("refresh", "Refresh", icons.refresh, "button small ghost")}</header>
      <div class="content page-${state.page} ${animate ? "enter" : ""}"><div id="notice" class="notice"><span class="notice-text" role="status" aria-live="polite"></span><button type="button" class="notice-undo" data-notice-undo hidden title="Undo (${undoKey()})">${icons.undo}<span>Undo</span></button><button type="button" class="notice-close" data-notice-close aria-label="Dismiss notification" title="Dismiss (Esc)">${icons.close}</button></div>${s.config.error ?`<div class="config-banner"><h3>Let’s get your schedule ready.</h3><p>${e(s.config.error)}</p><button class="button primary" type="button" data-page="schedule">Build a schedule with your AI agent</button><button class="button ghost" type="button" data-page="settings">Edit configuration</button></div>` : ""}${page}</div></main></div>`;
    bindShell(s);
    status();
    state.board.added = null;
    state.board.greet = null;
    if (state.board.focus) {
      state.board.focus = false;
      root.querySelector<HTMLInputElement>('form.composer [name="composerText"]')?.focus();
    }
  }

  function bindShell(s: Snapshot) {
    root.querySelectorAll<HTMLButtonElement>("[data-theme-choice]").forEach(btn => btn.onclick = () => {
      appearance = btn.dataset.themeChoice as ThemePreference;
      setThemePreference(appearance);
      root.querySelectorAll<HTMLButtonElement>("[data-theme-choice]").forEach(choice => choice.setAttribute("aria-pressed", String(choice.dataset.themeChoice === appearance)));
    });
    root.querySelectorAll<HTMLButtonElement>("[data-mode-choice]").forEach(btn => btn.onclick = () => {
      const mode = btn.dataset.modeChoice as Mode;
      if (mode === modeOf()) return;
      const toggle = btn.closest(".mode-switch");
      toggle?.classList.toggle("is-p", mode === "p");
      void action(async () => {
        try { await request("service_action", {action: "mode", mode}); }
        catch (error) { toggle?.classList.toggle("is-p", mode !== "p"); throw error; }
        await reload();
        state.notice = mode === "p" ? "P mode: a flexible, task-first workspace." : "J mode: your structured day is back.";
      });
    });
    root.querySelectorAll<HTMLButtonElement>("[data-page]").forEach(btn => btn.onclick = () => navigate(btn.dataset.page as Page));
    root.querySelector<HTMLAnchorElement>(".brand")!.onclick = event => { event.preventDefault(); navigate("today"); };
    root.querySelectorAll<HTMLButtonElement>("[data-action]").forEach(btn => btn.onclick = () => void action(() => runAction(btn.dataset.action!)));
    root.querySelector<HTMLButtonElement>("[data-notice-close]")!.onclick = dismissNotice;
    root.querySelector<HTMLButtonElement>("[data-notice-undo]")!.onclick = undo;
    const board = root.querySelector<HTMLElement>("#task-board");
    if (board) bindBoard(board, s);
    root.querySelectorAll<HTMLButtonElement>("[data-board-view]").forEach(btn => btn.onclick = () => setBoardView(btn.dataset.boardView as BoardView, s));
    root.querySelectorAll<HTMLButtonElement>("[data-deadline]").forEach(btn => btn.onclick = () => {
      const deadline = s.deadlines[Number(btn.dataset.deadline)];
      if (btn.dataset.operation === "edit") deadlineDialog(deadline);
      else void removeDeadline(deadline);
    });
    root.querySelectorAll<HTMLInputElement>("[data-habit]").forEach(check => check.onchange = () => {
      if (check.checked) burst(check.closest(".habit-row") ?? check, 14);
      void action(async () => { await request("habit_mark", {habitIds: [...root.querySelectorAll<HTMLInputElement>("[data-habit]:checked")].map(el => el.dataset.habit)}); await reload(); });
    });
    root.querySelector<HTMLInputElement>('[name="search"]')?.addEventListener("input", event => {
      state.search = (event.target as HTMLInputElement).value;
      redrawBoard(s);
    });
    root.querySelector<HTMLSelectElement>('[name="configFile"]')?.addEventListener("change", event => void action(async () => {
      if (state.config && root.querySelector<HTMLTextAreaElement>('[name="configContent"]')?.value !== state.config.content && !window.confirm("Discard unsaved edits?")) { (event.target as HTMLSelectElement).value = state.config.file; return; }
      state.config = await request<ConfigFile>("config_read", {file: (event.target as HTMLSelectElement).value});
      draw();
    }));
    root.querySelectorAll<HTMLButtonElement>("[data-agent-tab]").forEach(btn => btn.onclick = () => { state.agentTab = btn.dataset.agentTab as AgentTab; draw(); root.querySelector<HTMLElement>(`#agent-tab-${state.agentTab}`)?.focus(); });
    root.querySelectorAll<HTMLButtonElement>("[data-copy]").forEach(btn => btn.onclick = () => void copyText(btn.dataset.copy!));
    if (state.page === "activity") bindActivity(s);
    if (state.page === "routines") bindCalendar(s);
  }

  async function runAction(id: string) {
    if (id === "refresh") await reload();
    if (id === "add-task") taskDialog();
    if (id === "add-deadline") deadlineDialog();
    if (id === "sync-clear") { if (!window.confirm("Clear today’s plan? Your weekly routine stays the same.")) return; await request("sync_clear"); await reload(); state.notice = "Today’s plan was cleared."; }
    if (id === "open-config") { state.config = await request<ConfigFile>("config_read", {file: "settings"}); draw(); }
    if (id === "config-save") {
      const content = root.querySelector<HTMLTextAreaElement>('[name="configContent"]')!.value;
      const result = await request<ConfigFile & {message: string}>("config_save", {...state.config, content}); state.config = result; await reload(); state.notice = result.message;
    }
    if (id === "shutdown") { if (!window.confirm("Stop the local browser service? You can relaunch it whenever you need.")) return; await request("server_shutdown"); root.dispatchEvent(new Event("workspace-closed")); root.innerHTML = '<div class="loading"><h1>See you next time.</h1><p>Your local data is saved. Relaunch the browser workspace to return.</p></div>'; }
    if (id === "mode") { await request("service_action", {action: "mode", mode: modeOf() === "p" ? "j" : "p"}); await reload(); }
    if (["update", "stop", "switch"].includes(id)) {
      if (id === "stop" && !window.confirm("Stop reminder notifications?")) return;
      const result = await request<{message: string}>("service_action", {action: id, configId: Number(root.querySelector<HTMLSelectElement>('[name="configId"]')?.value)}); state.config = null; await reload(); state.notice = result.message;
    }
    if (id === "schedule-pdf") {
      const result = await request<{url: string; filename: string}>("export_pdf", {kind: "schedule"});
      const blob = await (await fetch(result.url)).blob(); const url = URL.createObjectURL(blob); const link = document.createElement("a"); link.href = url; link.download = result.filename; link.click(); setTimeout(() => URL.revokeObjectURL(url), 60000); state.notice = "PDF downloaded.";
    }
    if (id === "open-week" && state.activity.summary) { state.activity.view = "week"; state.activity.week = state.activity.summary; draw(true); scrollTop(); }
    if (id === "close-week") { state.activity.view = "gallery"; draw(true); }
    if (id === "clear-day") { state.activity.filter.day = null; redrawGallery(); }
  }
  async function copyText(text: string) {
    try { await navigator.clipboard.writeText(text); state.notice = `Copied: ${text}`; }
    catch { state.notice = "Could not copy. Select the text and copy it instead."; }
    status();
  }
  const scrollTop = () => { if (document.scrollingElement) document.scrollingElement.scrollTop = 0; };

  function bindTasks(scope: ParentNode, s: Snapshot) {
    scope.querySelectorAll<HTMLButtonElement>("[data-task]").forEach(btn => btn.onclick = () => {
      const task = s.tasks[Number(btn.dataset.task)];
      if (btn.dataset.operation === "edit") { taskDialog(task); return; }
      void action(async () => {
        const kind = btn.dataset.operation!;
        if (kind !== "deleted" && !window.confirm(`${kind === "cancelled" ? "Cancel" : "Drop"} “${task.description}”?`)) return;
        const card = btn.closest(".task-card");
        if (kind === "deleted") { burst(btn); card?.classList.add("completing"); }
        let result: {undo?: Record<string, unknown>} | undefined;
        try { result = await request<{undo?: Record<string, unknown>}>("task_delete", {description: task.description, action: kind}); }
        catch (error) { card?.classList.remove("completing"); throw error; }
        await reload();
        const notice = kind === "deleted" ? `Completed “${task.description}”. Nice work.` : `Removed “${task.description}”.`;
        const token = result?.undo;
        if (token) offerUndo(notice, () => restoreTask(task.description, token));
        else state.notice = notice;
      });
    });
  }
  /** Put a completed, cancelled, or dropped task back as it was, and greet it on the board. */
  async function restoreTask(description: string, token: Record<string, unknown>) {
    await request("task_restore", token);
    state.board.added = description;
    await reload();
    state.notice = `“${description}” is back on your list.`;
  }
  // ----- Task board: folding and inline add -----------------------------------

  const boardHtml = (s: Snapshot, opening: string | null = null) => {
    const board = state.board;
    return taskBoard({snapshot: s, query: state.search, isFolded: id => (board.view === "folded") !== board.flipped.has(id), composer: board.composer, added: board.added, opening, menu: board.menu, renaming: board.renaming, deleting: board.deleting, newType: board.newType, greet: board.greet});
  };
  /** Every group folded: the summaries line up in plain rows instead of the masonry. */
  const uniformBoard = () => state.board.view === "folded" && !state.board.flipped.size && !state.search;
  /** Redraw only the board, leaving the page around it as it is. `reveal` animates the contents of every
   *  group, or of one group by ID, as they appear after folding or expanding. */
  function redrawBoard(s: Snapshot, options: {opening?: string; reveal?: true | string; fresh?: boolean} = {}) {
    const board = root.querySelector<HTMLElement>("#task-board");
    if (!board) return;
    root.querySelector(".content")?.classList.remove("enter");
    board.classList.toggle("is-uniform", uniformBoard());
    board.innerHTML = boardHtml(s, options.opening ?? null);
    if (options.reveal) board.querySelectorAll<HTMLElement>(".task-group").forEach(group => group.classList.toggle("reveal", options.reveal === true || group.dataset.group === options.reveal));
    bindBoard(board, s, options.fresh);
  }
  function bindBoard(scope: HTMLElement, s: Snapshot, fresh = false) {
    masonry(scope, fresh);
    bindTasks(scope, s);
    scope.querySelectorAll<HTMLButtonElement>("[data-group-toggle]").forEach(btn => btn.onclick = () => {
      const id = btn.dataset.groupToggle!;
      if (!state.board.flipped.delete(id)) state.board.flipped.add(id);
      morph(() => {
        redrawBoard(s, {reveal: id});
        [...root.querySelectorAll<HTMLElement>("[data-group-toggle]")].find(el => el.dataset.groupToggle === id)?.focus({preventScroll: true});
      });
    });
    scope.querySelectorAll<HTMLButtonElement>("[data-compose]").forEach(btn => btn.onclick = () => {
      const type = btn.dataset.compose!;
      const previous = state.board.composer;
      state.board.composer = {type, priority: previous?.type === type ? previous.priority : 5, draft: previous?.type === type ? previous.draft : ""};
      redrawBoard(s, {opening: type});
      scope.querySelector<HTMLInputElement>('form.composer [name="composerText"]')?.focus();
    });
    scope.querySelectorAll<HTMLFormElement>("form.composer").forEach(form => bindComposer(form, s));
    bindTypes(scope, s);
  }

  // ----- Task board: managing types ------------------------------------------

  const findIn = (scope: ParentNode, selector: string, attribute: string, value: string) =>
    [...scope.querySelectorAll<HTMLElement>(selector)].find(el => el.getAttribute(attribute) === value);
  const typeName = (s: Snapshot, id: string) => s.taskTypes[id] ?? groupTasks(s).find(group => group.id === id)?.name ?? id;
  /** Close the type menu, the rename field, the delete question, and the new-type form, then redraw the board. */
  function settleTypes(s: Snapshot, change: Partial<BoardState> = {}) {
    Object.assign(state.board, {menu: null, renaming: null, deleting: null, ...change});
    redrawBoard(s);
  }
  function bindTypes(scope: HTMLElement, s: Snapshot) {
    const badge = scope.querySelector<HTMLElement>(".group-badge.is-open");
    if (badge) {
      badge.addEventListener("focusout", event => { if (!badge.contains(event.relatedTarget as Node | null)) closeTypeMenu(); });
      badge.addEventListener("keydown", event => {
        if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
        const items = [...badge.querySelectorAll<HTMLButtonElement>("[role='menuitem']:not(:disabled)")];
        if (!items.length) return;
        event.preventDefault();
        const at = items.indexOf(document.activeElement as HTMLButtonElement);
        items[(at + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length].focus();
      });
    }
    scope.querySelectorAll<HTMLButtonElement>("[data-type-menu]").forEach(btn => btn.onclick = () => {
      const id = btn.dataset.typeMenu!;
      const opening = state.board.menu !== id;
      settleTypes(s, {menu: opening ? id : null});
      if (opening) scope.querySelector<HTMLElement>(".type-menu [role='menuitem']:not(:disabled)")?.focus();
      else findIn(scope, "[data-type-menu]", "data-type-menu", id)?.focus();
    });
    const rename = (id: string) => {
      settleTypes(s, {renaming: id});
      const input = scope.querySelector<HTMLInputElement>("[data-type-name-input]");
      input?.focus();
      input?.select();
    };
    scope.querySelectorAll<HTMLButtonElement>("[data-type-rename]").forEach(btn => btn.onclick = () => rename(btn.dataset.typeRename!));
    scope.querySelectorAll<HTMLElement>("[data-type-title]").forEach(title => title.ondblclick = () => rename(title.dataset.typeTitle!));
    const nameInput = scope.querySelector<HTMLInputElement>("[data-type-name-input]");
    if (nameInput) bindRename(nameInput, s);

    scope.querySelectorAll<HTMLButtonElement>("[data-type-delete]").forEach(btn => btn.onclick = () => {
      const id = btn.dataset.typeDelete!;
      settleTypes(s, {deleting: {id, moveTo: ""}});
      const panel = findIn(scope, "[data-group]", "data-group", id);
      (panel?.querySelector<HTMLElement>("[data-type-move]") ?? panel?.querySelector<HTMLElement>("[data-type-delete-confirm]"))?.focus();
    });
    const panel = scope.querySelector<HTMLElement>(".type-delete");
    if (panel && state.board.deleting) {
      const {id} = state.board.deleting;
      const cancel = () => {
        settleTypes(s);
        findIn(scope, "[data-type-menu]", "data-type-menu", id)?.focus({preventScroll: true});
      };
      const select = panel.querySelector<HTMLSelectElement>("[data-type-move]");
      if (select) select.onchange = () => { if (state.board.deleting) state.board.deleting.moveTo = select.value; };
      panel.addEventListener("keydown", event => { if (event.key === "Escape") { event.preventDefault(); cancel(); } });
      panel.querySelector<HTMLButtonElement>("[data-type-delete-cancel]")!.onclick = cancel;
      panel.querySelector<HTMLButtonElement>("[data-type-delete-confirm]")!.onclick = () => void action(async () => {
        const name = typeName(s, id);
        const moveTo = select?.value ?? "";
        const result = await request<{movedTasks: number; movedDeadlines: number}>("task_type_delete", moveTo ? {id, moveTo} : {id});
        state.board.deleting = null;
        state.board.flipped.delete(id);
        await reload({morph: true, then: () => root.querySelector<HTMLElement>("[data-type-new]")?.focus({preventScroll: true})});
        const moved = [result.movedTasks ? `${result.movedTasks} ${result.movedTasks === 1 ? "task" : "tasks"}` : "", result.movedDeadlines ? `${result.movedDeadlines} ${result.movedDeadlines === 1 ? "deadline" : "deadlines"}` : ""].filter(Boolean).join(" and ");
        state.notice = `Deleted the type “${name}”.${moved ? ` ${moved} moved to “${moveTo ? typeName(s, moveTo) : "no type"}”.` : ""}`;
      });
    }

    scope.querySelectorAll<HTMLButtonElement>("[data-type-new]").forEach(btn => btn.onclick = () => {
      settleTypes(s, {newType: {draft: ""}});
      scope.querySelector<HTMLInputElement>('[name="newTypeName"]')?.focus();
    });
    const form = scope.querySelector<HTMLFormElement>(".new-type-form");
    if (form) {
      const input = form.querySelector<HTMLInputElement>('[name="newTypeName"]')!;
      const close = () => {
        settleTypes(s, {newType: null});
        scope.querySelector<HTMLElement>("[data-type-new]")?.focus({preventScroll: true});
      };
      input.addEventListener("input", () => { if (state.board.newType) state.board.newType.draft = input.value; });
      form.addEventListener("keydown", event => { if (event.key === "Escape") { event.preventDefault(); close(); } });
      form.querySelector<HTMLButtonElement>("[data-type-new-cancel]")!.onclick = close;
      form.addEventListener("submit", event => {
        event.preventDefault();
        if (state.busy) return;
        const name = input.value.trim();
        if (!name) {
          form.classList.remove("shake");
          void form.offsetWidth;
          form.classList.add("shake");
          input.focus();
          return;
        }
        void action(async () => {
          const created = await request<{id: string; name: string}>("task_type_save", {name});
          state.board.newType = null;
          state.board.greet = created.id;
          // The new group is ready for its first task.
          await reload({morph: true, then: () => findIn(root, "[data-compose]", "data-compose", created.id)?.focus()});
          state.notice = `Added the type “${created.name}”.`;
        });
      });
    }
  }
  function bindRename(input: HTMLInputElement, s: Snapshot) {
    const id = input.dataset.typeNameInput!;
    const original = typeName(s, id);
    let done = false;
    const finish = (save: boolean) => {
      if (done) return;
      done = true;
      const name = input.value.trim();
      const back = () => findIn(root, "[data-type-menu]", "data-type-menu", id)?.focus({preventScroll: true});
      if (!save || !name || name === original) { settleTypes(s); back(); return; }
      void action(async () => {
        try { await request("task_type_save", {id, name}); }
        catch (error) { done = false; throw error; }
        state.board.renaming = null;
        state.board.greet = id;
        await reload({then: back});
        state.notice = `Renamed “${original}” to “${name}”.`;
      });
    };
    input.addEventListener("keydown", event => {
      if (event.key === "Enter") { event.preventDefault(); finish(true); }
      if (event.key === "Escape") { event.preventDefault(); finish(false); }
    });
    input.addEventListener("blur", () => { if (input.isConnected) finish(true); });
  }
  /** Close the type menu in place, without redrawing the board. */
  function closeTypeMenu(focusButton = false) {
    const id = state.board.menu;
    if (!id) return;
    state.board.menu = null;
    root.querySelectorAll(".task-group.menu-open").forEach(group => group.classList.remove("menu-open"));
    root.querySelectorAll<HTMLElement>(".group-badge.is-open").forEach(badge => {
      badge.classList.remove("is-open");
      badge.querySelector(".type-menu")?.remove();
      badge.querySelector("[data-type-menu]")?.setAttribute("aria-expanded", "false");
    });
    if (focusButton) findIn(root, "[data-type-menu]", "data-type-menu", id)?.focus();
  }
  function setBoardView(view: BoardView, s: Snapshot) {
    if (view === state.board.view && !state.board.flipped.size) return;
    state.board.view = view;
    state.board.flipped.clear();
    saveBoardView(view);
    morph(() => {
      const toggle = root.querySelector<HTMLElement>(".fold-switch");
      toggle?.classList.toggle("is-folded", view === "folded");
      toggle?.querySelectorAll<HTMLButtonElement>("[data-board-view]").forEach(btn => btn.setAttribute("aria-pressed", String(btn.dataset.boardView === view)));
      redrawBoard(s, {reveal: true, fresh: true});
    });
  }
  function closeComposer(s: Snapshot) {
    const type = state.board.composer?.type;
    const slot = root.querySelector<HTMLElement>("form.composer")?.closest<HTMLElement>(".compose-slot");
    let closed = false;
    const finish = () => {
      if (closed) return;
      closed = true;
      state.board.composer = null;
      redrawBoard(s);
      [...root.querySelectorAll<HTMLElement>("[data-compose]")].find(el => el.dataset.compose === type)?.focus({preventScroll: true});
    };
    if (!slot || reducedMotion()) { finish(); return; }
    slot.classList.remove("opening");
    slot.classList.add("closing");
    slot.addEventListener("animationend", event => { if (event.target === slot) finish(); });
    window.setTimeout(finish, 320);
  }
  function bindComposer(form: HTMLFormElement, s: Snapshot) {
    const type = form.dataset.composer!;
    const input = form.querySelector<HTMLInputElement>('[name="composerText"]')!;
    const range = form.querySelector<HTMLInputElement>('[name="composerPriority"]')!;
    const shown = form.querySelector<HTMLElement>(".composer-importance b")!;
    const mine = () => state.board.composer?.type === type ? state.board.composer : null;
    input.addEventListener("input", () => { const composer = mine(); if (composer) composer.draft = input.value; });
    range.addEventListener("input", () => {
      const priority = Number(range.value);
      shown.textContent = String(priority);
      range.style.setProperty("--p", ((priority - 1) / 9).toFixed(3));
      const composer = mine();
      if (composer) composer.priority = priority;
    });
    form.addEventListener("keydown", event => { if (event.key === "Escape") { event.preventDefault(); closeComposer(s); } });
    form.querySelector<HTMLButtonElement>("[data-compose-close]")!.onclick = () => closeComposer(s);
    form.addEventListener("submit", event => {
      event.preventDefault();
      if (state.busy) return;
      const description = input.value.trim();
      if (!description) {
        form.classList.remove("shake");
        void form.offsetWidth;
        form.classList.add("shake");
        input.focus();
        return;
      }
      const priority = Number(range.value) || 5;
      const name = s.taskTypes[type] ?? groupTasks(s).find(group => group.id === type)?.name ?? "this group";
      const existing = s.tasks.find(task => task.description === description);
      const where = existing && existing.type !== type ? ` under ${s.taskTypes[existing.type] ?? existing.typeName}` : "";
      if (existing && !window.confirm(`“${description}” is already on your list${where}. Replace it with this one?`)) { input.focus(); return; }
      // Clear the field right away so the next task can be typed while this one saves.
      input.value = "";
      state.board.composer = {type, priority, draft: ""};
      void action(async () => {
        try { await request("task_add", {description, priority, type, postpone: 0}); }
        catch (error) {
          const composer = mine();
          if (composer && !composer.draft) { composer.draft = description; if (input.isConnected && !input.value) input.value = description; }
          throw error;
        }
        state.board.added = description;
        state.board.focus = true;
        await reload();
        state.notice = `Added “${description}” to ${name}.`;
      });
    });
  }

  async function removeDeadline(deadline: DeadlineItem) {
    await action(async () => { if (!window.confirm(`Remove “${deadline.event}”?`)) return; await request("deadline_delete", {event: deadline.event}); await reload(); });
  }
  function themeControls() {
    return `<div class="theme-controls" role="group" aria-label="Color theme">${(["light", "dark", "system"] as ThemePreference[]).map(mode => `<button type="button" data-theme-choice="${mode}" aria-label="${mode === "system" ? "Use system theme" : `Use ${mode} theme`}" aria-pressed="${appearance === mode}" title="${mode === "system" ? "Follow system" : `${mode === "light" ? "Light" : "Dark"} theme`}">${icons[mode]}</button>`).join("")}</div>`;
  }
  function modeSwitch(mode: Mode) {
    return `<div class="mode-switch ${mode === "p" ? "is-p" : ""}" role="group" aria-label="Planning mode"><span class="mode-thumb" aria-hidden="true"></span><button type="button" data-mode-choice="j" aria-pressed="${mode === "j"}" title="J mode · structured day with schedule, deadlines, and timeline">${icons.target}<span>J</span></button><button type="button" data-mode-choice="p" aria-pressed="${mode === "p"}" title="P mode · flexible and task-first">${icons.feather}<span>P</span></button></div>`;
  }

  // ----- Overview ------------------------------------------------------------

  function overviewJ(s: Snapshot) {
    const complete = s.habits.filter(h => h.completed).length;
    const overdue = s.deadlines.filter(d => d.daysLeft !== null && d.daysLeft < 0).length;
    const dueToday = s.deadlines.filter(d => d.daysLeft === 0).length;
    const highImportance = s.tasks.filter(t => t.priority >= 8 && (!t.alarmFrom || t.alarmFrom <= s.today.date)).length;
    const metrics = [
      {id: "overdue", label: "Overdue", value: overdue, detail: "Past the due date", tone: overdue ? "danger" : "neutral", image: icons.flag},
      {id: "today", label: "Due today", value: dueToday, detail: "Due by end of day", tone: dueToday ? "warning" : "neutral", image: icons.schedule},
      {id: "priority", label: "High importance", value: highImportance, detail: "Ready now", tone: highImportance ? "priority" : "neutral", image: icons.zap},
      {id: "habits", label: "Habits complete", value: `${complete}/${s.habits.length}`, detail: s.habits.length ? "Your daily routine" : "No habits configured", tone: complete ? "success" : "neutral", image: icons.activity}
    ];
    return `<div class="page-heading"><div><p class="eyebrow">${icons.target} TODAY’S OVERVIEW · J MODE</p><h1>Your day at a glance<span class="accent-dot">.</span></h1><p class="muted">${s.tasks.length} open tasks · ${s.deadlines.length} deadlines · ${complete}/${s.habits.length} habits complete</p></div>${button("add-task", "New task", icons.plus, "button primary")}</div>
      <section class="status-grid" aria-label="Today at a glance">${metrics.map((m, i) => `<article class="status-card status-${m.tone}" data-metric="${m.id}" style="--i:${i}${m.id === "habits" ? `; --p:${s.habits.length ? complete / s.habits.length : 0}` : ""}"><span class="orb" aria-hidden="true"></span><div class="status-label">${m.image}<span>${m.label}</span></div><strong class="status-value">${m.value}</strong><p>${m.detail}</p></article>`).join("")}</section>
      <section class="overview-grid"><article class="focus-card"><div class="aurora" aria-hidden="true"><i></i><i></i><i></i></div><div class="focus-top"><span class="focus-label"><span class="live-dot"></span> RIGHT NOW</span><span class="pill">${s.schedule.hasSyncedOverlay ? "Your plan" : "Weekly routine"}</span></div><h2>${e(s.schedule.current ?? (s.schedule.isSkipped ? "Schedule paused today" : "No active block"))}</h2><div class="next-block"><span>${icons.clock} UP NEXT</span><strong>${e(s.schedule.next ?? "No upcoming blocks")}</strong>${s.schedule.timeToNext ? `<span class="next-time">In ${e(s.schedule.timeToNext)}</span>` : ""}</div><button class="focus-link" type="button" data-page="schedule">View today’s schedule ${icons.arrow}</button><svg class="focus-wave" viewBox="0 0 400 60" preserveAspectRatio="none" aria-hidden="true"><path d="M0 34 C 70 4, 130 60, 210 30 S 340 6, 400 28 L400 60 L0 60Z"/></svg></article>
      <article class="card deadline-radar"><div class="section-heading compact"><h2>Next deadlines <span class="count">${s.deadlines.length}</span></h2>${button("add-deadline", "Add", icons.plus, "text-button")}</div>${deadlineList(s, 3)}<button class="text-button view-all" type="button" data-page="routines">Open the year calendar ${icons.arrow}</button></article></section>
      ${taskSection(s)}
      <article class="card timeline-card"><div class="section-heading compact"><h3>${icons.schedule} Today’s timeline</h3><button class="text-button" type="button" data-page="schedule">Full schedule ${icons.arrow}</button></div>${timeline(s)}</article>`;
  }
  const searchPill = () => `<label class="search-pill">${icons.search}<input class="search" name="search" aria-label="Search tasks" placeholder="Find a task…" value="${e(state.search)}"></label>`;
  function overviewP(s: Snapshot) {
    return `<div class="page-heading p-heading"><div><p class="eyebrow">${icons.feather} P MODE · GO WITH THE FLOW</p><h1>Tasks by type<span class="accent-dot">.</span></h1><p class="muted">${s.tasks.length} open tasks. No clock to chase. Pick whatever pulls you in.</p></div><div class="heading-actions">${searchPill()}${foldSwitch(state.board.view)}${button("add-task", "New task", icons.plus, "button primary")}</div></div>
      <section id="task-board" class="task-board flow-board ${uniformBoard() ? "is-uniform" : ""}">${boardHtml(s)}</section>`;
  }
  function taskSection(s: Snapshot) {
    return `<div class="section-heading board-heading"><div><h2>Tasks by type <span class="count">${s.tasks.length}</span></h2><p class="muted">Groups with more important work come first. Inside a group, tasks keep their order by status and importance.</p></div><div class="board-tools">${searchPill()}${foldSwitch(state.board.view)}</div></div><section id="task-board" class="task-board ${uniformBoard() ? "is-uniform" : ""}">${boardHtml(s)}</section>`;
  }
  function timeline(s: Snapshot) {
    const events = s.schedule.events;
    if (!events.length) return empty("No blocks scheduled for today. Build your routine in Schedule.");
    const now = new Date();
    const minutes = now.getHours() * 60 + now.getMinutes();
    let current = -1;
    if (s.today.date === isoDay(now)) events.forEach((event, i) => { const [hour, minute] = event.time.split(":").map(Number); if (hour * 60 + minute <= minutes) current = i; });
    return `<ol class="timeline">${events.map((event, i) => `<li class="${i < current ? "is-past" : ""} ${i === current ? "is-now" : ""}" style="--i:${i}"><time>${e(event.time)}</time><span class="timeline-dot ${event.syncable ? "work" : ""}"></span><span>${e(event.label)}</span>${i === current ? '<span class="pill now-pill">Now</span>' : event.syncable ? '<span class="pill light">Focus</span>' : ""}</li>`).join("")}</ol>`;
  }
  function deadlineList(s: Snapshot, limit?: number) {
    const deadlines = [...s.deadlines].sort((a, b) => (a.daysLeft ?? Infinity) - (b.daysLeft ?? Infinity));
    return deadlines.length ? `<ul class="deadline-list">${(limit ? deadlines.slice(0, limit) : deadlines).map(d => {
      const index = s.deadlines.indexOf(d);
      const label = d.daysLeft === null ? "Check date" : d.daysLeft === 0 ? "Due today" : d.daysLeft < 0 ? `${-d.daysLeft}d overdue` : d.daysLeft === 1 ? "Tomorrow" : `${d.daysLeft}d left`;
      return `<li class="deadline-${e(d.status)} ${sw(deadlineSwatch(d, s.taskTypes))}"><span class="ddl-dot" aria-hidden="true"></span><div class="deadline-copy"><strong>${e(d.event)}</strong><div class="deadline-meta"><time>${e(d.deadline)}</time>${repeatBadge(d.repeat)}<span class="type-badge ${d.type ? sw(typeSwatch(s.taskTypes, d.type)) : "untyped"}">${e(d.typeName ?? d.type ?? "Uncategorized")}</span></div></div><span class="deadline-status ${e(d.status)}">${label}</span><button class="text-button" data-deadline="${index}" data-operation="edit" aria-label="Edit ${e(d.event)}" type="button">Edit</button><button class="icon-button danger" data-deadline="${index}" data-operation="remove" aria-label="Remove ${e(d.event)}" type="button">${icons.close}</button></li>`;
    }).join("")}</ul>` : empty("No deadlines to track.");
  }

  // ----- Schedule, routines, settings -------------------------------------------

  function schedulePage(s: Snapshot) {
    const agent = AGENT_SETUP[state.agentTab];
    const planCard = s.schedule.hasSyncedOverlay ? `<article class="card plan-card"><p class="eyebrow">${icons.spark} TODAY’S PLAN</p><h2>Your focus blocks have a purpose.</h2><p class="muted">Your AI agent planned today. Each focus block in the timeline shows its task, and reminders announce it. Your weekly routine is unchanged.</p><div class="button-row">${button("sync-clear", "Clear today’s plan", icons.close, "button ghost danger")}</div></article>` : "";
    return `<div class="page-heading"><div><p class="eyebrow">A RHYTHM THAT WORKS FOR YOU</p><h1>Make a little space<span class="accent-dot">.</span></h1><p class="muted">${e(s.today.parity)} week · ${e(s.today.weekday)} · schedule ${s.config.activeId}</p></div>${button("schedule-pdf", "Download schedule", icons.schedule)}</div><section class="split-grid"><article class="card timeline-card"><div class="section-heading compact"><h2>Today’s timeline</h2><span class="pill light">${s.schedule.events.length} blocks</span></div>${timeline(s)}</article><div>${planCard}
      <article class="card agent-card"><p class="eyebrow">${icons.spark} PLAN WITH YOUR AI AGENT</p><h2>Let your coding agent build and plan your schedule.</h2><p class="muted">Install the Schedule Everything plugin in Claude Code, Codex, or Antigravity. It interviews you, writes your weekly routine, and fills today’s focus blocks through the local <code>rmd</code> CLI. Your agent sends what it reads to its own model provider.</p>
        <div class="agent-tabs" role="tablist" aria-label="Choose your agent">${(Object.keys(AGENT_SETUP) as AgentTab[]).map(id => `<button type="button" role="tab" id="agent-tab-${id}" class="agent-tab" data-agent-tab="${id}" aria-selected="${state.agentTab === id}" aria-controls="agent-panel">${e(AGENT_SETUP[id].name)}</button>`).join("")}</div>
        <div class="agent-panel" role="tabpanel" id="agent-panel" aria-labelledby="agent-tab-${state.agentTab}"><ol class="agent-steps">${agent.steps.map((command, i) => `<li><span class="step-num" aria-hidden="true">${i + 1}</span><code>${e(command)}</code><button type="button" class="icon-button" data-copy="${e(command)}" aria-label="Copy command ${i + 1}">${icons.copy}</button></li>`).join("")}</ol><p class="muted">${e(agent.note)}</p></div>
        <p class="eyebrow try-label">THEN ASK YOUR AGENT</p><div class="prompt-chips">${AGENT_PROMPTS.map(prompt => `<button type="button" class="chip prompt-chip" data-copy="${e(prompt)}" title="Copy prompt">${icons.spark}${e(prompt)}</button>`).join("")}</div></article></div></section>`;
  }
  function routines(s: Snapshot) {
    const done = s.habits.filter(h => h.completed).length;
    return `<div class="page-heading"><div><p class="eyebrow">SMALL THINGS, STEADY PROGRESS</p><h1>Routines & deadlines<span class="accent-dot">.</span></h1><p class="muted">Plant deadlines on your year, color them, and keep your daily habits close.</p></div>${button("add-deadline", "New deadline", icons.plus, "button primary")}</div>
      ${yearCalendar({deadlines: s.deadlines, taskTypes: s.taskTypes, today: s.today.date, state: state.calendar, conferences: state.conferences})}
      <section class="split-grid"><article class="card habits-card"><div class="section-heading"><h2>Today’s habits</h2><span class="pill light">${done}/${s.habits.length} complete</span></div>${s.habits.length ? `<div class="habit-hero">${ring({value: done / s.habits.length, size: 96, label: `${done} of ${s.habits.length} habits complete`, center: `${done}/${s.habits.length}`})}<p class="muted">${done === s.habits.length ? "Every habit done today. Lovely." : `${s.habits.length - done} to go. Small and steady.`}</p></div>${s.habits.map((h, i) => `<label class="habit-row ${h.completed ? "done" : ""}" style="--i:${i}"><input type="checkbox" data-habit="${e(h.id)}" ${h.completed ? "checked" : ""}><span class="habit-check" aria-hidden="true">${icons.check}</span><span class="habit-text">${e(h.description)}</span></label>`).join("")}` : empty("Add habits in Settings to build your daily rhythm.")}<p class="muted">Edit your habit definitions in Settings → Habits.</p></article><article class="card"><h2>All deadlines</h2>${deadlineList(s)}</article></section>`;
  }
  function activity(s: Snapshot) {
    const a = state.activity;
    if (a.view === "week" && a.week) return `<div class="page-heading"><div><p class="eyebrow">${icons.spark} WEEKLY OVERVIEW</p><h1>Your week in color<span class="accent-dot">.</span></h1><p class="muted">Compared with last week and the weeks before it.</p></div></div>${weeklyView(a.week)}`;
    const intro = a.layout === "tree" ? "Everything you finished, cancelled, or let go, growing along one branch of time. Hover over any event to see its story." : "A gallery of everything you finished, cancelled, or let go. Hover over any piece to see its story.";
    return `<div class="page-heading"><div><p class="eyebrow">LOOK HOW FAR YOU’VE COME</p><h1>Little wins add up<span class="accent-dot">.</span></h1><p class="muted">${intro}</p></div></div>${galleryView({items: a.items, overview: a.summary, filter: a.filter, layout: a.layout, region: a.items ? historyRegion(s) : "", today: s.today.date, taskTypes: s.taskTypes})}`;
  }
  function settings(s: Snapshot) {
    const files = {settings: "General settings", odd: "Odd week schedule", even: "Even week schedule", habits: "Habits", deadlines: "Deadlines", profile: "Profile"};
    return `<div class="page-heading"><div><p class="eyebrow">MAKE IT YOURS</p><h1>Your workspace, your rules<span class="accent-dot">.</span></h1><p class="muted">All changes are saved to your computer.</p></div></div><section class="split-grid"><article class="card"><h2>Reminder preferences</h2><p class="muted">${modeOf() === "p" ? "P mode · selected event alarms paused · overview shows tasks only" : "J mode · all reminders allowed · full structured overview"}</p>${button("mode", modeOf() === "p" ? "Switch to J mode" : "Switch to P mode", icons.settings)}<hr><label>Active schedule<select name="configId">${state.workspace?.configIds?.map(id => `<option value="${id}" ${id === s.config.activeId ? "selected" : ""}>Schedule ${id}</option>`).join("") ?? ""}</select></label><div class="button-row">${button("switch", "Activate")}${button("update", "Reload reminders", icons.refresh)}${button("stop", "Stop reminders", "", "button ghost danger")}</div><p class="muted path">${e(s.config.rootDir)}</p><hr><h3>Task & deadline types</h3><p class="muted">Types are managed on the task board: add one with the New type tile at the end of the board, and rename or delete one from the menu in its group's header. Deadlines share the same types.</p><div class="button-row"><button type="button" class="button ghost" data-page="today">${icons.tasks}<span>Open the task board</span></button></div><hr>${button("shutdown", "Close browser service", "", "button ghost")}</article><article class="card"><h2>Configuration editor</h2><p class="muted">Edit your schedules, habits, settings, or profile here. Syntax is checked before saving.</p>${state.config ? `<label>File<select name="configFile">${Object.entries(files).map(([id, label]) => `<option value="${id}" ${id === state.config!.file ? "selected" : ""}>${label}</option>`).join("")}</select></label><label>Contents<textarea class="code-editor" name="configContent" rows="22" spellcheck="false">${e(state.config.content)}</textarea></label>${button("config-save", "Save changes", icons.check, "button primary")}` : button("open-config", "Open editor", icons.settings)}<p class="muted">Saved edits reload the installed reminder service when available.</p></article></section>`;
  }

  // ----- Activity bindings ---------------------------------------------------

  /** History in display order: the tree always runs newest first, the gallery follows the chosen sort. */
  function visibleHistory(): HistoryItem[] {
    const a = state.activity;
    return filterHistory(a.items ?? [], a.layout === "tree" ? {...a.filter, sort: "newest"} : a.filter);
  }
  function historyRegion(s: Snapshot): string {
    const a = state.activity;
    const items = a.items ?? [];
    if (a.layout === "gallery") return galleryGroups(items, a.filter, a.limit, s.today.date, s.taskTypes);
    const visible = visibleHistory();
    if (!visible.length) return empty(items.length ? "Nothing matches these filters. Try another outcome or type." : "Your first small win will appear here.");
    const position = new Map(items.map((item, i) => [item, i]));
    return treeView({items: visible.slice(0, a.treeLimit), total: visible.length, indexOf: item => position.get(item) ?? -1, taskTypes: s.taskTypes, today: s.today.date, step: TREE_PAGE});
  }
  function redrawRegion(s: Snapshot, keepScroll = false) {
    const region = root.querySelector<HTMLElement>("#history");
    if (!region) return;
    const top = root.querySelector<HTMLElement>("[data-tree-viewport]")?.scrollTop ?? 0;
    hidePeek();
    region.innerHTML = historyRegion(s);
    bindRegion(s);
    const viewport = root.querySelector<HTMLElement>("[data-tree-viewport]");
    if (viewport && keepScroll) viewport.scrollTop = top;
  }
  function redrawGallery() {
    draw();
    root.querySelector("#history")?.classList.add("reflow");
  }
  function bindActivity(s: Snapshot) {
    const a = state.activity;
    const resetPaging = () => { a.limit = GALLERY_PAGE; a.treeLimit = TREE_PAGE; };
    root.querySelectorAll<HTMLButtonElement>("[data-filter-status]").forEach(btn => btn.onclick = () => { a.filter.status = btn.dataset.filterStatus as HistoryStatus; resetPaging(); redrawGallery(); });
    root.querySelectorAll<HTMLButtonElement>("[data-filter-type]").forEach(btn => btn.onclick = () => { a.filter.type = a.filter.type === btn.dataset.filterType ? null : btn.dataset.filterType!; resetPaging(); redrawGallery(); });
    root.querySelectorAll<HTMLButtonElement>("[data-layout]").forEach(btn => btn.onclick = () => {
      const layout = btn.dataset.layout as HistoryLayout;
      if (layout === a.layout) return;
      a.layout = layout;
      saveHistoryLayout(layout);
      redrawGallery();
      root.querySelector<HTMLElement>(`[data-layout="${layout}"]`)?.focus();
    });
    // In the tree a day is a place to travel to; in the gallery it filters the cards.
    root.querySelectorAll<HTMLButtonElement>("[data-heat-day]").forEach(btn => btn.onclick = () => {
      if (a.layout === "tree" && !a.filter.day) { scrollTreeTo(root, btn.dataset.heatDay!); return; }
      a.filter.day = a.filter.day === btn.dataset.heatDay ? null : btn.dataset.heatDay!;
      a.limit = GALLERY_PAGE;
      redrawGallery();
    });
    root.querySelector<HTMLSelectElement>('[name="gallerySort"]')?.addEventListener("change", event => { a.filter.sort = (event.target as HTMLSelectElement).value as HistorySort; redrawGallery(); });
    root.querySelector<HTMLInputElement>('[name="gallerySearch"]')?.addEventListener("input", event => {
      a.filter.query = (event.target as HTMLInputElement).value;
      resetPaging();
      redrawRegion(s);
    });
    root.querySelectorAll<HTMLButtonElement>("[data-week]").forEach(btn => btn.onclick = () => void action(async () => {
      if (btn.dataset.week === "today" || !a.week) a.week = a.summary;
      else {
        const result = await request<WeeklyOverview>("weekly_overview", {date: addDays(a.week.weekStart, btn.dataset.week === "prev" ? -7 : 7)});
        if (isOverview(result)) a.week = result;
      }
      draw(true);
    }));
    bindRegion(s);
    const rhythm = root.querySelector<HTMLElement>(".rhythm");
    if (rhythm) rhythm.scrollLeft = rhythm.scrollWidth;
  }
  function bindRegion(s: Snapshot) {
    const a = state.activity;
    root.querySelectorAll<HTMLElement>("[data-history]").forEach(card => {
      card.onclick = () => openLightbox(Number(card.dataset.history), s);
      if (card.tagName !== "BUTTON") card.onkeydown = event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); openLightbox(Number(card.dataset.history), s); } };
    });
    // Hovering (or focusing) an event shows its details beside it; clicking still opens the full story.
    const items = a.items ?? [];
    bindPeek(root.querySelector<HTMLElement>("#history"), el => {
      const item = items[Number(el.dataset.history)];
      return item ? peekCard(item, items, s.taskTypes) : null;
    });
    const more = root.querySelector<HTMLButtonElement>("[data-gallery-more]");
    if (more) more.onclick = () => { a.limit += GALLERY_PAGE; redrawRegion(s); };
    const grow = root.querySelector<HTMLButtonElement>("[data-tree-more]");
    if (grow) grow.onclick = () => { a.treeLimit += TREE_PAGE; redrawRegion(s, true); };
    if (a.layout === "gallery") { bindTilt(root, ".gallery-card"); return; }
    // Days on the rhythm grid that are in the tree's view light up as you scroll.
    const cells = [...root.querySelectorAll<HTMLElement>(".heat-cell[data-heat-day]")];
    bindTree(root, range => cells.forEach(cell => cell.classList.toggle("in-view", !!range && cell.dataset.heatDay! >= range.from && cell.dataset.heatDay! <= range.to)));
  }
  function openLightbox(index: number, s: Snapshot) {
    const items = state.activity.items ?? [];
    const visible = visibleHistory();
    let position = visible.indexOf(items[index]);
    if (position < 0) return;
    root.querySelector("dialog.lightbox")?.remove();
    const el = document.createElement("dialog");
    el.className = "lightbox";
    el.setAttribute("aria-label", "Task details");
    const close = () => { el.close(); el.remove(); root.querySelector<HTMLElement>(`[data-history="${items.indexOf(visible[position])}"]`)?.focus(); };
    const render = (direction = "") => {
      el.innerHTML = `<button type="button" class="icon-button lb-close" aria-label="Close">${icons.close}</button><div class="lb-inner ${direction}">${lightbox(visible[position], items, {index: position, total: visible.length}, s.taskTypes)}</div>`;
      el.querySelector<HTMLButtonElement>(".lb-close")!.onclick = close;
      el.querySelectorAll<HTMLButtonElement>("[data-lightbox]").forEach(btn => btn.onclick = () => step(btn.dataset.lightbox === "next" ? 1 : -1));
    };
    const step = (by: number) => {
      const next = position + by;
      if (next < 0 || next >= visible.length) return;
      position = next;
      render(by > 0 ? "from-right" : "from-left");
    };
    el.addEventListener("keydown", event => {
      if (event.key === "ArrowRight") step(1);
      if (event.key === "ArrowLeft") step(-1);
      if (event.key === "Escape") { event.preventDefault(); close(); }
    });
    el.addEventListener("click", event => { if (event.target === el) close(); });
    root.append(el);
    el.showModal();
    render();
    el.querySelector<HTMLButtonElement>(".lb-close")?.focus();
  }

  // ----- Calendar bindings ---------------------------------------------------

  function drawCalendar(focusInput = false) {
    const card = root.querySelector("#calendar");
    const s = state.snapshot;
    if (!card || !s) return;
    card.outerHTML = yearCalendar({deadlines: s.deadlines, taskTypes: s.taskTypes, today: s.today.date, state: state.calendar, conferences: state.conferences});
    bindCalendar(s);
    if (focusInput) root.querySelector<HTMLInputElement>('[name="popEvent"]')?.focus();
  }
  function openDay(day: string) {
    state.calendar.day = day;
    state.conferences.menuOpen = false;
    state.calendar.year = Number(day.slice(0, 4));
    drawCalendar(true);
    if (!root.querySelector('[name="popEvent"]')) root.querySelector<HTMLElement>(".day-popover button")?.focus();
  }
  function positionPopover() {
    const pop = root.querySelector<HTMLElement>(".day-popover");
    const card = root.querySelector<HTMLElement>("#calendar");
    const cell = root.querySelector<HTMLElement>(`.day[data-day="${state.calendar.day}"]`);
    if (!pop || !card || !cell) return;
    const box = card.getBoundingClientRect();
    const rect = cell.getBoundingClientRect();
    const width = pop.offsetWidth || 340;
    const height = pop.offsetHeight || 280;
    const left = Math.max(12, Math.min(rect.left - box.left + rect.width / 2 - width / 2, box.width - width - 12));
    let top = rect.bottom - box.top + 10;
    if (top + height > box.height - 8 && rect.top - box.top - height - 10 > 0) { top = rect.top - box.top - height - 10; pop.classList.add("above"); }
    pop.style.left = `${left}px`;
    pop.style.top = `${top}px`;
    pop.style.setProperty("--arrow", `${rect.left - box.left + rect.width / 2 - left}px`);
  }
  async function moveDeadline(index: number, day: string) {
    const s = state.snapshot!;
    const deadline = s.deadlines[index];
    if (!deadline || deadline.deadline === day) return;
    await action(async () => {
      await request("deadline_update", {originalEvent: deadline.event, event: deadline.event, date: day});
      state.calendar.day = null;
      state.calendar.flash = day;
      await reload();
      state.notice = `Moved “${deadline.event}” to ${formatDate(day, {weekday: "short", month: "short", day: "numeric"})}.`;
    });
  }
  /** Fetch conference dates for the viewed year; the server serves them from a weekly cache. */
  async function loadConferences(refresh = false) {
    const view = state.conferences;
    if (!view.enabled || view.loading) return;
    const year = state.calendar.year;
    view.loading = true;
    drawCalendar();
    // Record the year that was asked for, so a mismatched reply can never trigger another fetch.
    try { view.data = {...await request<ConferencePayload>("conference_calendar", refresh ? {year, refresh: true} : {year}), year}; }
    catch (error) { view.data = {year, source: "https://github.com/huggingface/ai-deadlines", fetchedAt: null, error: error instanceof Error ? error.message : String(error), conferences: view.data?.conferences ?? [], events: []}; }
    finally { view.loading = false; }
    if (state.page === "routines" && root.isConnected) drawCalendar();
  }
  function bindConferences() {
    const view = state.conferences;
    // A redraw replaces the clicked control, so put keyboard focus back on its replacement.
    const redraw = (focus: string) => { drawCalendar(); root.querySelector<HTMLElement>(focus)?.focus(); };
    const update = (focus: string) => { saveConferencePrefs(view); redraw(focus); };
    const toggle = root.querySelector<HTMLButtonElement>("[data-conf-toggle]");
    if (toggle) toggle.onclick = () => { view.enabled = !view.enabled; view.menuOpen = view.enabled && !view.data; update("[data-conf-toggle]"); };
    const menu = root.querySelector<HTMLButtonElement>("[data-conf-menu]");
    if (menu) menu.onclick = () => { view.menuOpen = !view.menuOpen; redraw("[data-conf-menu]"); };
    const ids = view.data?.conferences.map(conference => conference.id) ?? [];
    root.querySelectorAll<HTMLInputElement>("[data-conf-pick]").forEach(box => box.onchange = () => {
      const chosen = new Set(view.selected ?? ids);
      if (box.checked) chosen.add(box.dataset.confPick!); else chosen.delete(box.dataset.confPick!);
      view.selected = ids.filter(id => chosen.has(id));
      if (view.selected.length === ids.length) view.selected = null;
      update(`[data-conf-pick="${box.dataset.confPick}"]`);
    });
    const all = root.querySelector<HTMLButtonElement>("[data-conf-all]");
    if (all) all.onclick = () => { view.selected = null; update("[data-conf-all]"); };
    const none = root.querySelector<HTMLButtonElement>("[data-conf-none]");
    if (none) none.onclick = () => { view.selected = []; update("[data-conf-none]"); };
    const refresh = root.querySelector<HTMLButtonElement>("[data-conf-refresh]");
    if (refresh) refresh.onclick = () => void loadConferences(true);
    root.querySelectorAll<HTMLButtonElement>("[data-conf-day]").forEach(item => item.onclick = () => openDay(item.dataset.confDay!));
    root.querySelectorAll<HTMLButtonElement>("[data-conf-add]").forEach(btn => btn.onclick = () => {
      const event = view.data?.events.find(item => item.id === btn.dataset.confAdd);
      if (!event) return;
      const name = `${event.name} ${event.edition} ${event.label.toLowerCase()}`;
      void action(async () => {
        await request("deadline_add", {event: name, date: event.date, type: "", color: conferenceSwatch(event), repeat: ""});
        state.calendar.flash = event.date;
        await reload();
        state.notice = `Added “${name}” to your deadlines.`;
      });
    });
    if (view.enabled && !view.loading && view.data?.year !== state.calendar.year) void loadConferences();
  }
  function bindCalendar(s: Snapshot) {
    const today = s.today.date;
    bindConferences();
    if (state.calendar.flash) window.setTimeout(() => { state.calendar.flash = null; }, 1600);
    root.querySelectorAll<HTMLButtonElement>("[data-cal]").forEach(btn => btn.onclick = () => {
      const target = btn.dataset.cal;
      state.calendar.year = target === "today" ? Number(today.slice(0, 4)) : state.calendar.year + (target === "next" ? 1 : -1);
      state.calendar.day = null;
      drawCalendar();
      if (target === "today") root.querySelector<HTMLElement>(".day.is-today")?.focus();
    });
    root.querySelectorAll<HTMLButtonElement>("[data-cal-filter]").forEach(btn => btn.onclick = () => {
      const color = btn.dataset.calFilter;
      state.calendar.filter = isSwatch(color) && state.calendar.filter !== color ? color : null;
      drawCalendar();
    });
    root.querySelectorAll<HTMLButtonElement>(".day[data-day]").forEach(cell => {
      const day = cell.dataset.day!;
      cell.onclick = () => {
        if (state.calendar.day !== day) { openDay(day); return; }
        state.calendar.day = null;
        drawCalendar();
      };
      if (day < today) return;
      cell.addEventListener("dragover", event => { event.preventDefault(); cell.classList.add("drop-hover"); });
      cell.addEventListener("dragleave", () => cell.classList.remove("drop-hover"));
      cell.addEventListener("drop", event => {
        event.preventDefault();
        cell.classList.remove("drop-hover");
        const index = Number(event.dataTransfer?.getData("text/plain"));
        if (Number.isInteger(index)) void moveDeadline(index, day);
      });
    });
    root.querySelectorAll<HTMLElement>("[data-drag-index]").forEach(el => {
      el.addEventListener("dragstart", event => {
        event.dataTransfer?.setData("text/plain", el.dataset.dragIndex!);
        if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
        root.classList.add("is-dragging");
        el.classList.add("dragging");
      });
      el.addEventListener("dragend", () => { root.classList.remove("is-dragging"); el.classList.remove("dragging"); root.querySelectorAll(".drop-hover").forEach(cell => cell.classList.remove("drop-hover")); });
    });
    root.querySelectorAll<HTMLButtonElement>("[data-chip-day]").forEach(chip => chip.onclick = () => openDay(chip.dataset.chipDay!));
    const pop = root.querySelector<HTMLElement>(".day-popover");
    if (!pop) return;
    const day = pop.dataset.popoverDay!;
    positionPopover();
    pop.querySelector<HTMLButtonElement>("[data-pop-close]")!.onclick = () => { state.calendar.day = null; drawCalendar(); root.querySelector<HTMLElement>(`.day[data-day="${day}"]`)?.focus(); };
    pop.querySelectorAll<HTMLButtonElement>("[data-pop-color]").forEach(btn => btn.onclick = () => {
      const deadline = s.deadlines[Number(btn.dataset.popIndex)];
      const color = btn.dataset.popColor!;
      void action(async () => { await request("deadline_update", {originalEvent: deadline.event, event: deadline.event, date: deadline.deadline, color: deadline.color === color ? "" : color}); await reload(); });
    });
    pop.querySelectorAll<HTMLButtonElement>("[data-pop-edit]").forEach(btn => btn.onclick = () => deadlineDialog(s.deadlines[Number(btn.dataset.popEdit)]));
    pop.querySelectorAll<HTMLButtonElement>("[data-pop-remove]").forEach(btn => btn.onclick = () => void removeDeadline(s.deadlines[Number(btn.dataset.popRemove)]));
    pop.querySelector<HTMLFormElement>(".pop-form")?.addEventListener("submit", event => {
      event.preventDefault();
      const data = new FormData(event.target as HTMLFormElement);
      const name = String(data.get("popEvent") ?? "").trim();
      if (!name) return;
      const existing = s.deadlines.find(d => d.event === name);
      if (existing && existing.deadline !== day && !window.confirm(`“${name}” is already due ${existing.deadline}. Move it to this day?`)) return;
      void action(async () => {
        await request("deadline_add", {event: name, date: day, type: data.get("popType") ?? "", color: data.get("popColor") ?? "", repeat: data.get("popRepeat") ?? ""});
        state.calendar.day = null;
        state.calendar.flash = day;
        await reload();
        state.notice = `Added “${name}” on ${formatDate(day, {weekday: "short", month: "short", day: "numeric"})}.`;
      });
    });
  }

  // ----- Dialogs ---------------------------------------------------------------

  function dialog(title: string, body: string, submit: (data: FormData) => Promise<void>) {
    root.querySelector("dialog.editor-dialog")?.remove();
    const el = document.createElement("dialog"); el.className = "editor-dialog"; el.innerHTML = `<form><div class="section-heading"><h2>${title}</h2><button class="icon-button" type="button" aria-label="Close">${icons.close}</button></div>${body}<div class="button-row"><button class="button primary" type="submit">Save</button><button class="button ghost" type="button" data-close>Cancel</button></div><p class="dialog-error" role="alert"></p></form>`;
    root.append(el); el.showModal();
    el.querySelectorAll<HTMLButtonElement>('button[type="button"]').forEach(btn => btn.onclick = () => { el.close(); el.remove(); });
    el.querySelector("form")!.onsubmit = event => { event.preventDefault(); const btn = el.querySelector<HTMLButtonElement>('[type="submit"]')!; btn.disabled = true; void submit(new FormData(event.target as HTMLFormElement)).then(() => { el.close(); el.remove(); }).catch(error => { el.querySelector(".dialog-error")!.textContent = error instanceof Error ? error.message : String(error); btn.disabled = false; }); };
  }
  function taskDialog(task?: TaskItem) {
    const days = task?.alarmFrom ? Math.max(0, Math.round((Date.parse(task.alarmFrom) - Date.parse(state.snapshot!.today.date)) / 86400000)) : 0;
    dialog(task ? "Edit task" : "A new thing to work on", `${field("description", "Task", "text", task?.description ?? "", 'required maxlength="500"')}${field("priority", "Importance · 1 to 10", "number", String(task?.priority ?? 5), 'required min="1" max="10"')}<label>Task type<select name="type">${typeOptions(state.snapshot!, task?.type)}</select></label>${field("postpone", "Start reminders in (days)", "number", String(days), 'required min="0" max="3650"')}`, async data => { await request(task ? "task_update" : "task_add", {description: data.get("description"), priority: Number(data.get("priority")), type: data.get("type"), postpone: Number(data.get("postpone")), originalDescription: task?.description}); await reload(); });
  }
  function deadlineDialog(deadline?: DeadlineItem) {
    const selected = deadline?.type ?? "";
    const color = deadline?.color;
    const unknown = selected && !(selected in state.snapshot!.taskTypes) ? `<option value="${e(selected)}" selected>${e(deadline?.typeName ?? selected)} (unavailable)</option>` : "";
    dialog(deadline ? "Edit deadline" : "New deadline", `${field("event", "Event", "text", deadline?.event ?? "", "required")}${field("date", "Due date", "date", deadline?.deadline ?? state.snapshot!.today.date, "required")}<label>Deadline type<select name="type"><option value="">Uncategorized</option>${typeOptions(state.snapshot!, selected)}${unknown}</select></label><label>Repeat<select name="repeat">${repeatOptions(deadline?.repeat)}</select></label>${swatchPicker("color", isSwatch(color) ? color : "", "Match type color")}`, async data => {
      await request(deadline ? "deadline_update" : "deadline_add", {event: data.get("event"), date: data.get("date"), type: data.get("type"), color: data.get("color") ?? "", repeat: data.get("repeat") ?? "", originalEvent: deadline?.event});
      state.calendar.day = null;
      await reload();
    });
  }

  root.addEventListener("workspace-refresh", () => { if (state.busy || document.hidden || root.querySelector("dialog[open]") || state.calendar.day || state.conferences.menuOpen || state.board.menu || state.board.renaming || state.board.deleting || state.board.newType || ["settings", "schedule", "activity"].includes(state.page) || ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName ?? "")) return; void reload().catch(() => { state.notice = "Could not refresh. Check the local server connection."; status(); }); }, {signal});
  root.addEventListener("workspace-activate", () => { state.notice = "Your existing workspace is ready."; status(); }, {signal});
  root.addEventListener("pointerdown", event => {
    if (state.board.menu && event.target instanceof Element && !event.target.closest(".group-badge")) closeTypeMenu();
  }, {signal, capture: true});
  root.addEventListener("click", event => {
    if (state.conferences.menuOpen && event.target instanceof Element && event.target.isConnected && !event.target.closest(".conf-control")) {
      state.conferences.menuOpen = false;
      drawCalendar();
      return;
    }
    if (!state.calendar.day || !(event.target instanceof Element) || !event.target.isConnected) return;
    if (event.target.closest(".day-popover, .day[data-day], .ddl-chip, dialog, [data-cal]")) return;
    state.calendar.day = null;
    drawCalendar();
  }, {signal});
  // Listen on the document: redraws replace the focused control, and Safari never focuses clicked buttons,
  // so Escape often reaches <body> rather than the workspace root.
  document.addEventListener("keydown", event => {
    if (!root.isConnected || event.defaultPrevented) return;
    // ⌘Z / Ctrl+Z takes back what the notice reports, unless a field would undo its own typing instead.
    if (event.key.toLowerCase() === "z" && (event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey && undoable()) {
      const target = document.activeElement;
      const typing = target instanceof HTMLElement && (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) || target.isContentEditable);
      if (!typing && !root.querySelector("dialog[open]")) { event.preventDefault(); undo(); return; }
    }
    if (event.key === "Escape" && state.board.menu) {
      closeTypeMenu(true);
      return;
    }
    if (event.key === "Escape" && state.conferences.menuOpen) {
      state.conferences.menuOpen = false;
      drawCalendar();
      root.querySelector<HTMLElement>("[data-conf-menu]")?.focus();
      return;
    }
    if (event.key !== "Escape" || root.querySelector("dialog[open]")) return;
    if (state.calendar.day) {
      const day = state.calendar.day;
      state.calendar.day = null;
      drawCalendar();
      root.querySelector<HTMLElement>(`.day[data-day="${day}"]`)?.focus();
      return;
    }
    // Nothing else is open, so Escape puts the notice away.
    dismissNotice();
  }, {signal});
  window.addEventListener("resize", () => positionPopover(), {signal});
  try { await reload(); } catch (error) { root.innerHTML = `<div class="loading"><h1>Could not open your workspace</h1><p>${e(error instanceof Error ? error.message : String(error))}</p><button class="button" id="retry">Try again</button></div>`; root.querySelector("#retry")?.addEventListener("click", () => void renderApp(root, client)); }
}
