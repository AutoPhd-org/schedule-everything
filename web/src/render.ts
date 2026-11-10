// Browser workspace shell: application state, page layouts, and event binding.
// J mode shows the full structured overview; P mode keeps only the tasks-by-type board. In both, timers for
// today's timed tasks and for deadlines in their last 24 hours run above the board, and the daily overview
// sums up the day. The theme follows the workload (workload.ts) unless that is turned off in Settings.
import { defaultGalleryFilter, filterHistory, galleryGroups, galleryView, lightbox, peekCard, weeklyView, type GalleryFilter, type HistoryLayout, type HistoryStatus, type HistorySort } from "./activity";
import { boardGroups, boardOrder, boardTaskCard, foldSwitch, groupTasks, layoutSwitch, masonry, setBoardHooks, spreadOut, taskBoard, typeShelf, type BoardView, type Composer, type SubtaskComposer } from "./board";
import { conferenceSwatch, deadlineSwatch, repeatBadge, repeatOptions, yearCalendar, type CalendarState, type ConferenceView } from "./calendar";
import { ring } from "./charts";
import { dayPanel } from "./daily";
import { bindGlossary, glossaryPage, showInGlossary, updateGlossaryValues, type GlossaryContext } from "./glossary";
import { bindTilt, burst, farewell, reducedMotion, type Farewell } from "./effects";
import { bindPeek, hidePeek } from "./peek";
import { bindCategoryPanel, bindPluginControls, bindPluginPage, bindPluginTimers, bindTaskExtras, categoryBar, categoryPanel, describeSuggestion, groupIcon, pluginById, pluginNav, pluginOfType, pluginPage, pluginPageId, pluginsCard, pluginTimers, subtasksFor, taskExtra, tickPluginClocks, type PluginHost } from "./plugins";
import { loadPrefs, savePrefs, type BoardLayout, type Prefs } from "./prefs";
import { applyTheme, setThemePreference, themePreference, type ThemePreference } from "./theme";
import { clockText, dialDash, progressOf, pruneClosed, runway, timerStrip, todayProgress, urgency } from "./timers";
import { bindTooltips } from "./tooltip";
import { bindTree, scrollTreeTo, treeView } from "./tree";
import type { BridgeClient, ConferencePayload, DailyOverview, DeadlineItem, HistoryItem, Snapshot, Subtask, SuggestionItem, TaskItem, WeeklyOverview, Workspace } from "./types";
import { addDays, button, e, empty, escapeHtml, field, formatDate, icons, isoDay, isSwatch, setTypeColors, sw, swatchPicker, typeSwatch } from "./ui";
import { LANGUAGES, language, msg, setLanguage, t, tn } from "./i18n";
import { DEFAULT_THRESHOLDS, describeLoad, levelInfo, levelRanges, loadLevels, loadNumber, loadThresholds, MAX_THRESHOLD, roundLoad, setLoadThresholds, thresholdProblem, WORKLOAD_DEFINITION, workload, type LoadThresholds, type Workload } from "./workload";

export { escapeHtml, groupTasks };

/** A plugin's page is `plugin:<id>` (see plugins.ts). */
type Page = "today" | "schedule" | "routines" | "activity" | "glossary" | "settings" | `plugin:${string}`;
type Mode = "j" | "p";
type ConfigFile = {file: string; content: string; revision: string; activeId: number};
type ActivityState = {view: "gallery" | "week"; layout: HistoryLayout; items: HistoryItem[] | null; summary: WeeklyOverview | null; week: WeeklyOverview | null; filter: GalleryFilter; limit: number; treeLimit: number; loading: boolean};
/** The task board: expanded or folded, groups toggled away from that view, the inline new-task form, the task
 *  to greet after it was added, and whether to put the cursor back in the form after the next draw. Types are
 *  managed here too: the open type menu, the type being renamed or deleted, the new-type form, and the group
 *  to greet after its type was added or renamed. `order` holds the groups in the places they keep while the
 *  page stays open (null places them afresh at the next draw, and `fresh` lays them out anew), and
 *  `subtaskComposer` is the open new-subtask field. */
type BoardState = {
  view: BoardView; flipped: Set<string>; composer: Composer | null; added: string | null; focus: boolean;
  menu: string | null; renaming: string | null; deleting: {id: string; moveTo: string} | null; newType: {draft: string} | null; greet: string | null;
  order: string[] | null; fresh: boolean; subtaskComposer: SubtaskComposer | null;
};
type AgentTab = "claude" | "codex" | "antigravity";
/** A way to take back what a notice reports. It belongs to that notice: once the notice is replaced or put
 *  away, or anything else is done, the chance to undo is gone. */
type Undo = {notice: string; run: () => Promise<void>};
/** Deadline timers the user closed, by deadline name and date (see timerKey). */
type State = {page: Page; glossary: string | null; snapshot: Snapshot | null; workspace: Workspace | null; agentTab: AgentTab; search: string; busy: boolean; notice: string; undo: Undo | null; config: ConfigFile | null; activity: ActivityState; calendar: CalendarState; conferences: ConferenceView; board: BoardState; closedTimers: Set<string>; prefs: Prefs; daily: DailyOverview | null};

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
const CLOSED_TIMERS = "schedule-everything-closed-timers";
/** Closing a deadline's timer only hides the timer, in this browser; the deadline itself is left alone. */
function loadClosedTimers(): Set<string> {
  try {
    const saved = JSON.parse(window.localStorage.getItem(CLOSED_TIMERS) ?? "[]");
    if (Array.isArray(saved)) return new Set(saved.filter((key: unknown): key is string => typeof key === "string"));
  } catch { /* Fall back to showing every timer when storage is unavailable. */ }
  return new Set();
}
function saveClosedTimers(closed: Set<string>, today: string): void {
  try { window.localStorage.setItem(CLOSED_TIMERS, JSON.stringify(pruneClosed(closed, today))); } catch { /* Keep working without storage. */ }
}

/** Each page with its name, marked for translation and translated where it is drawn. */
const PAGES: [Exclude<Page, `plugin:${string}`>, string][] = [["today", msg("Overview")], ["schedule", msg("Schedule")], ["routines", msg("Routines & deadlines")], ["activity", msg("Activity")], ["glossary", msg("Glossary")], ["settings", msg("Settings")]];
const GALLERY_PAGE = 60;
const TREE_PAGE = 300;
/** How long a notice stays before it goes away on its own, and how long it lingers once the pointer or focus
 *  that held it leaves. */
const NOTICE_MS = 10_000;
const NOTICE_LINGER_MS = 2_000;
const REPOSITORY = "AutoPhd-org/schedule-everything";
const AGENT_SETUP: Record<AgentTab, {name: string; steps: string[]; note: string}> = {
  claude: {name: "Claude Code", steps: [`claude plugin marketplace add ${REPOSITORY}`, "claude plugin install schedule-everything@schedule-everything"], note: msg("Inside a Claude Code session you can run the same steps as /plugin marketplace add and /plugin install.")},
  codex: {name: "Codex", steps: [`codex plugin marketplace add ${REPOSITORY}`, "codex plugin add schedule-everything@schedule-everything"], note: msg("The plugin also appears in the ChatGPT desktop app once the marketplace is added.")},
  antigravity: {name: "Antigravity", steps: [`git clone https://github.com/${REPOSITORY}.git`, "agy plugin install ./schedule-everything/plugins/schedule-everything"], note: msg("Antigravity installs the plugin from the cloned folder.")}
};
/** Example requests to copy into an AI agent, in the interface language (translated where they are drawn). */
const AGENT_PROMPTS = [msg("Build my weekly schedule"), msg("Plan my focus blocks for today"), msg("Move my workouts to the evenings"), msg("Suggest tasks from my email and calendar")];
const typeOptions = (snapshot: Snapshot, selected = "") => Object.entries(snapshot.taskTypes).map(([id, name]) => `<option value="${e(id)}" ${id === selected ? "selected" : ""}>${e(name)}</option>`).join("");
const isOverview = (value: unknown): value is WeeklyOverview => !!value && Array.isArray((value as WeeklyOverview).days) && Array.isArray((value as WeeklyOverview).trend);
const isDaily = (value: unknown): value is DailyOverview => !!value && Array.isArray((value as DailyOverview).hours) && typeof (value as DailyOverview).date === "string";
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
    page: "today", glossary: null, snapshot: null, workspace: null, agentTab: "claude", search: "", busy: false, notice: "", undo: null, config: null,
    activity: {view: "gallery", layout: loadHistoryLayout(), items: null, summary: null, week: null, filter: defaultGalleryFilter(), limit: GALLERY_PAGE, treeLimit: TREE_PAGE, loading: false},
    calendar: {year: new Date().getFullYear(), day: null, filter: null, flash: null},
    conferences: {...loadConferencePrefs(), menuOpen: false, data: null, loading: false},
    board: {view: loadBoardView(), flipped: new Set(), composer: null, added: null, focus: false, menu: null, renaming: null, deleting: null, newType: null, greet: null, order: null, fresh: true, subtaskComposer: null},
    closedTimers: loadClosedTimers(),
    prefs: loadPrefs(),
    daily: null
  };
  setLoadThresholds(state.prefs.loadThresholds);
  bindTooltips(signal);
  /** What plugin modules may use of the shell. */
  const host: PluginHost = {
    root,
    snapshot: () => state.snapshot!,
    history: () => state.activity.items,
    request: (command, payload = {}) => request(command, payload),
    action: work => action(work),
    reload: () => reload(),
    redraw: () => { if (state.snapshot) draw(); },
    notify: text => { state.notice = text; status(); },
    offerUndo: (text, run) => { offerUndo(text, run); status(); },
    navigate: page => navigate(page as Page),
    bindTasks: scope => { const s = state.snapshot!; bindTasks(scope, s); bindProgress(scope, s); bindSubtasks(scope, s); bindTaskExtras(scope as HTMLElement, host); },
    taskCard: (task, index, position) => boardTaskCard(task, index, position, Date.now(), state.board.subtaskComposer?.task === task.description ? state.board.subtaskComposer : null)
  };
  setBoardHooks({
    groupIcon: type => state.snapshot ? groupIcon(state.snapshot, type) : null,
    taskExtra: (task, index) => state.snapshot ? taskExtra(state.snapshot, task, index) : "",
    suggestionExtra: item => {
      const line = state.snapshot ? describeSuggestion(state.snapshot, item.type, item.plugin) : "";
      return line ? `<p class="suggestion-plugin">${icons.zap}<span>${e(line)}</span></p>` : "";
    },
    pluginType: type => pluginOfType(state.snapshot, type)?.name ?? null
  });
  let drawn: {page: Page; mode: Mode} | null = null;
  /** Progress and subtasks set on this page that the server has not confirmed yet, by task. */
  const progressPending = new Map<string, number>();
  const subtasksPending = new Map<string, Subtask[]>();
  root.innerHTML = `<div class="loading"><span class="loading-blob">${icons.sprout}</span><p>${t("Opening your workspace…")}</p></div>`;
  const request = async <T>(command: string, payload: Record<string, unknown> = {}): Promise<T> => client.send<T>(command, payload);
  const modeOf = (): Mode => state.workspace?.mode === "p" ? "p" : "j";

  /** Fetch and redraw. `morph` animates the change, so groups glide into place when a type comes or goes;
   *  `then` runs right after the redraw. */
  async function reload(options: {morph?: boolean; then?: () => void} = {}) {
    state.snapshot = await request<Snapshot>("status_snapshot");
    // Progress and subtasks still on their way to the server win over what the server had a moment ago.
    for (const [description, progress] of progressPending) {
      const task = state.snapshot.tasks.find(item => item.description === description);
      if (task) task.progress = progress;
    }
    for (const [description, steps] of subtasksPending) {
      const task = state.snapshot.tasks.find(item => item.description === description);
      if (task) { task.subtasks = steps.map(step => ({...step})); task.progress = progressOf(task); }
    }
    [state.workspace] = await Promise.all([request<Workspace>("workspace_info"), state.page === "today" ? loadDaily() : null]);
    // The language is a setting the CLI can change too, so every reload picks it up.
    if (state.workspace?.language) setLanguage(state.workspace.language);
    setTypeColors(state.snapshot.taskTypeColors);
    if (!drawn) state.calendar.year = Number(state.snapshot.today.date.slice(0, 4)) || state.calendar.year;
    if (state.page === "activity") await loadActivity();
    const update = () => { draw(); options.then?.(); };
    if (options.morph) morph(update); else update();
  }
  /** Today's numbers for the daily overview; when they cannot be loaded, the last ones stay. */
  async function loadDaily() {
    try {
      const overview = await request<DailyOverview>("daily_overview", {});
      if (isDaily(overview)) state.daily = overview;
    } catch { /* The panel keeps what it had. */ }
  }
  async function refreshDaily() {
    await loadDaily();
    if (state.page === "today" && root.isConnected) redrawDaily();
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
    const text = state.busy ? t("Working on it…") : state.notice;
    region.querySelector(".notice-text")!.textContent = text;
    region.querySelector<HTMLButtonElement>("[data-notice-undo]")!.hidden = !undoable();
    region.classList.remove("leaving");
    region.classList.toggle("visible", !!text);
    region.classList.toggle("busy", state.busy);
    timeNotice();
  }
  /** A notice goes away on its own NOTICE_MS after it appears (and its chance to undo with it). Redraws that show
   *  the same notice keep its clock; "Working on it…" waits for the work to end. While the pointer rests on the
   *  notice or focus is inside it, it stays, and leaves shortly after. */
  let noticeTimer = 0;
  let timedNotice = "";
  function timeNotice(delay = NOTICE_MS) {
    if (state.busy || !state.notice) { window.clearTimeout(noticeTimer); timedNotice = ""; return; }
    if (timedNotice === state.notice && delay === NOTICE_MS) return;
    window.clearTimeout(noticeTimer);
    timedNotice = state.notice;
    noticeTimer = window.setTimeout(() => {
      if (signal.aborted || state.notice !== timedNotice) return;
      const region = root.querySelector<HTMLElement>("#notice");
      if (region && (region.matches(":hover") || region.contains(document.activeElement))) { timeNotice(NOTICE_LINGER_MS); return; }
      dismissNotice();
    }, delay);
  }
  signal.addEventListener("abort", () => window.clearTimeout(noticeTimer));
  /** Let the notice slide away; a newer notice that arrives meanwhile replaces it. */
  function dismissNotice() {
    const region = root.querySelector<HTMLElement>("#notice");
    if (state.busy || !state.notice || !region) return;
    state.notice = "";
    state.undo = null;
    timeNotice();
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
  /** Open a page. `week` opens Activity on the weekly overview. */
  function navigate(page: Page, options: {week?: boolean} = {}) {
    state.page = page;
    Object.assign(state.board, {composer: null, menu: null, renaming: null, deleting: null, newType: null, subtaskComposer: null});
    // Opening a page, even the one already open, places the task board afresh.
    freshBoard();
    state.calendar.day = null;
    if (page === "activity") {
      state.activity.view = options.week ? "week" : "gallery";
      if (options.week) state.activity.week = state.activity.summary;
    }
    draw(options.week);
    scrollTop();
    if (page === "activity") void refreshActivity();
    if (page === "today") void refreshDaily();
    if (page === "glossary") void refreshGlossary();
  }

  // ----- Glossary ------------------------------------------------------------------------------------------

  const glossaryContext = (s: Snapshot): GlossaryContext => ({snapshot: s, daily: state.daily, week: state.activity.summary, weighted: state.prefs.weighted, now: Date.now()});
  /** Open the glossary at a metric or a section (see glossary.ts). */
  function openGlossary(target: string | null) {
    navigate("glossary");
    if (target) showInGlossary(root, target, reducedMotion());
  }
  /** The glossary shows your own numbers beside the metrics: load today's and this week's, then fill them in. */
  async function refreshGlossary() {
    try {
      const [daily, week] = await Promise.all([request<DailyOverview>("daily_overview", {}), request<WeeklyOverview>("weekly_overview", {})]);
      if (isDaily(daily)) state.daily = daily;
      if (isOverview(week)) state.activity.summary = week;
    } catch { /* The glossary keeps the numbers it had. */ }
    if (state.page === "glossary" && state.snapshot && root.isConnected) updateGlossaryValues(root, glossaryContext(state.snapshot));
  }
  /** Follow a glossary link to where a metric shows. */
  function jumpFromGlossary(target: string) {
    if (target === "week") navigate("activity", {week: true});
    else if (target === "day") {
      navigate("today");
      if (state.prefs.dayFolded) { state.prefs.dayFolded = false; savePrefs(state.prefs); redrawDaily(); }
      root.querySelector<HTMLElement>("#day-panel")?.scrollIntoView?.({behavior: reducedMotion() ? "auto" : "smooth", block: "start"});
    }
    else if (target === "board") {
      navigate("today");
      root.querySelector<HTMLElement>("#board-area")?.scrollIntoView?.({behavior: reducedMotion() ? "auto" : "smooth", block: "start"});
    }
    else if (target === "activity" || target === "settings" || target.startsWith("plugin:")) navigate(target as Page);
  }
  /** Forget where the board's groups were, so the next draw orders and places them anew. Only opening a page,
   *  switching mode or layout, and Refresh do this: changes made on the board leave every group in its place. */
  function freshBoard() {
    state.board.order = null;
    state.board.fresh = true;
  }
  /** Take the board's order when the page opens, and add groups that got their first task since at the end. */
  function syncBoardOrder(s: Snapshot) {
    if (!state.board.order) { state.board.order = boardOrder(s, state.prefs.layout); return; }
    const listed = new Set(state.board.order);
    for (const group of boardGroups(s, state.board.order)) if (!listed.has(group.id)) state.board.order.push(group.id);
  }

  function draw(animate = false) {
    const s = state.snapshot!;
    const mode = modeOf();
    hidePeek();
    document.documentElement.dataset.mode = mode;
    const samePage = !!drawn && drawn.page === state.page && drawn.mode === mode;
    animate ||= !samePage;
    if (!samePage) freshBoard();
    drawn = {page: state.page, mode};
    syncBoardOrder(s);
    applyLoad(s);
    // Redrawing the same page keeps it where it was scrolled, even if the board is briefly shorter meanwhile.
    const scroller = document.scrollingElement;
    const scrolled = samePage && scroller ? scroller.scrollTop : null;
    // A plugin page whose plugin was removed falls back to the overview.
    const plugin = pluginPageId(state.page) === null ? null : pluginById(s, pluginPageId(state.page)!);
    if (pluginPageId(state.page) !== null && !plugin) { state.page = "today"; drawn = {page: state.page, mode}; }
    const title = plugin ? e(plugin.name) : t(PAGES.find(([id]) => id === state.page)![1]);
    const page = plugin ? pluginPage(plugin, host) : state.page === "today" ? (mode === "p" ? overviewP(s) : overviewJ(s)) : state.page === "schedule" ? schedulePage(s) : state.page === "routines" ? routines(s) : state.page === "activity" ? activity(s) : state.page === "glossary" ? glossaryPage(glossaryContext(s)) : settings(s);
    root.innerHTML = `<div class="workspace mode-${mode}"><aside class="sidebar"><a class="brand" href="#" aria-label="${t("Schedule Everything overview")}"><span class="brand-mark">${icons.sprout}</span><span>schedule<span class="brand-sub">everything</span></span></a><div class="workspace-label">${t("YOUR WORKSPACE")}</div>
      <nav aria-label="${t("Main navigation")}">${PAGES.map(([id, label]) => `<button type="button" data-page="${id}" title="${e(t(label))}" aria-current="${state.page === id ? "page" : "false"}" class="nav-item ${state.page === id ? "active" : ""}"><span class="nav-icon">${icons[id]}</span><span class="nav-label">${t(label)}</span>${id === "today" ? `<span class="nav-count">${s.tasks.length}</span>` : ""}</button>`).join("")}${pluginNav(s, state.page)}</nav>
      <div class="sidebar-bottom"><div class="mode-note mode-${mode}">${mode === "p" ? icons.feather : icons.target}<div><strong>${mode === "p" ? t("P mode") : t("J mode")}</strong><span>${mode === "p" ? t("Flexible, task-first") : t("Structured day")}</span></div></div><span class="local-dot"></span> ${t("Saved on your computer")}<p>${t("One day, one thing at a time.")}</p></div></aside>
      <main class="main"><header class="topbar"><span class="breadcrumb">${t("Workspace")} ${icons.arrow} <strong>${title}</strong></span>${modeSwitch(mode)}${loadChip(s)}<span class="date">${icons.today}${e(formatDate(s.today.date, {weekday: "short", month: "short", day: "numeric"}))}</span>${themeControls()}${button("refresh", t("Refresh"), icons.refresh, "button small ghost")}</header>
      <div class="content page-${plugin ? "plugin" : state.page} ${animate ? "enter" : ""}"><div id="notice" class="notice"><span class="notice-text" role="status" aria-live="polite"></span><button type="button" class="notice-undo" data-notice-undo hidden title="${t("Undo ({key})", {key: undoKey()})}">${icons.undo}<span>${t("Undo")}</span></button><button type="button" class="notice-close" data-notice-close aria-label="${t("Dismiss notification")}" title="${t("Dismiss (Esc)")}">${icons.close}</button></div>${s.config.error ?`<div class="config-banner"><h3>${t("Let’s get your schedule ready.")}</h3><p>${e(s.config.error)}</p><button class="button primary" type="button" data-page="schedule">${t("Build a schedule with your AI agent")}</button><button class="button ghost" type="button" data-page="settings">${t("Edit configuration")}</button></div>` : ""}${page}</div></main></div>`;
    bindShell(s);
    state.board.fresh = false;
    if (scrolled !== null && scroller && Math.abs(scroller.scrollTop - scrolled) > 1) scroller.scrollTop = scrolled;
    status();
    state.board.added = null;
    state.board.greet = null;
    if (state.board.focus) {
      state.board.focus = false;
      root.querySelector<HTMLInputElement>('form.composer [name="composerText"]')?.focus({preventScroll: true});
    }
  }

  // ----- Workload and the daily overview ------------------------------------------

  /** The theme follows the workload, unless that is turned off in Settings: the workload's level picks the
   *  palette, and today's progress bar (today's timed work, weighed as the bar weighs it) eases it part of the
   *  way back toward calm as it fills (--day-progress, see styles.css). */
  function applyLoad(s: Snapshot) {
    const html = document.documentElement;
    if (!state.prefs.workloadTheme) {
      delete html.dataset.load;
      html.style.removeProperty("--day-progress");
      return;
    }
    html.dataset.load = workload(s.tasks, s.today.date).level;
    const today = todayProgress(s, Date.now(), state.prefs.weighted);
    html.style.setProperty("--day-progress", (today.parts.length ? today.value : 0).toFixed(3));
  }
  /** The workload in the header, on every page; it opens the daily overview. */
  function loadChip(s: Snapshot) {
    const load = workload(s.tasks, s.today.date);
    const info = levelInfo(load.level);
    return `<button type="button" class="load-chip load-${load.level}" data-load-chip data-tip="${e(`${describeLoad(load)} ${t(WORKLOAD_DEFINITION)}`)}" aria-label="${e(`${describeLoad(load)} ${t("Open the daily overview.")}`)}">${info.icon}<span class="load-name">${e(info.label)}</span><b>${load.points.toFixed(1)}</b></button>`;
  }
  function bindLoadChip() {
    const chip = root.querySelector<HTMLButtonElement>("[data-load-chip]");
    if (!chip) return;
    chip.onclick = () => {
      if (state.page !== "today") navigate("today");
      if (state.prefs.dayFolded) { state.prefs.dayFolded = false; savePrefs(state.prefs); redrawDaily(); }
      root.querySelector<HTMLElement>("#day-panel")?.scrollIntoView?.({behavior: reducedMotion() ? "auto" : "smooth", block: "start"});
    };
  }
  /** After open work changes on this page: the theme and the header follow. */
  function updateLoad(s: Snapshot) {
    applyLoad(s);
    const chip = root.querySelector("[data-load-chip]");
    if (!chip) return;
    chip.outerHTML = loadChip(s);
    bindLoadChip();
  }
  const dayOptions = (s: Snapshot) => ({overview: state.daily, snapshot: s, now: Date.now(), weighted: state.prefs.weighted, folded: state.prefs.dayFolded});
  function redrawDaily() {
    const panel = root.querySelector("#day-panel");
    const s = state.snapshot;
    if (!panel || !s) return;
    panel.outerHTML = dayPanel(dayOptions(s));
    bindDaily();
  }
  function bindDaily() {
    const fold = root.querySelector<HTMLButtonElement>("[data-day-fold]");
    if (fold) fold.onclick = () => {
      state.prefs.dayFolded = !state.prefs.dayFolded;
      savePrefs(state.prefs);
      redrawDaily();
      root.querySelector<HTMLElement>("[data-day-fold]")?.focus({preventScroll: true});
    };
    root.querySelectorAll<HTMLButtonElement>("#day-panel [data-page]").forEach(btn => btn.onclick = () => navigate(btn.dataset.page as Page));
    root.querySelectorAll<HTMLButtonElement>("#day-panel [data-glossary]").forEach(btn => btn.onclick = () => openGlossary(btn.dataset.glossary || null));
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
        state.notice = mode === "p" ? t("P mode: a flexible, task-first workspace.") : t("J mode: your structured day is back.");
      });
    });
    root.querySelectorAll<HTMLButtonElement>("[data-language]").forEach(btn => btn.onclick = () => {
      const choice = btn.dataset.language!;
      if (choice === language()) return;
      void action(async () => {
        await request("language_set", {language: choice});
        await reload();
        state.notice = t("The workspace, the rmd command line, and reminder popups now use {name}.", {name: LANGUAGES.find(entry => entry.id === language())!.name});
      });
    });
    root.querySelectorAll<HTMLButtonElement>("[data-page]").forEach(btn => btn.onclick = () => navigate(btn.dataset.page as Page));
    root.querySelector<HTMLAnchorElement>(".brand")!.onclick = event => { event.preventDefault(); navigate("today"); };
    root.querySelectorAll<HTMLButtonElement>("[data-action]").forEach(btn => btn.onclick = () => void action(() => runAction(btn.dataset.action!)));
    root.querySelector<HTMLButtonElement>("[data-notice-close]")!.onclick = dismissNotice;
    root.querySelector<HTMLButtonElement>("[data-notice-undo]")!.onclick = undo;
    const area = root.querySelector<HTMLElement>("#board-area");
    if (area) bindBoard(area, s);
    const timers = root.querySelector<HTMLElement>("#timer-strip");
    if (timers) bindTimers(timers, s);
    root.querySelectorAll<HTMLButtonElement>("[data-board-view]").forEach(btn => btn.onclick = () => setBoardView(btn.dataset.boardView as BoardView, s));
    bindDisplay(root, s);
    bindLoadChip();
    bindDaily();
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
      if (state.config && root.querySelector<HTMLTextAreaElement>('[name="configContent"]')?.value !== state.config.content && !window.confirm(t("Discard unsaved edits?"))) { (event.target as HTMLSelectElement).value = state.config.file; return; }
      state.config = await request<ConfigFile>("config_read", {file: (event.target as HTMLSelectElement).value});
      draw();
    }));
    root.querySelectorAll<HTMLButtonElement>("[data-agent-tab]").forEach(btn => btn.onclick = () => { state.agentTab = btn.dataset.agentTab as AgentTab; draw(); root.querySelector<HTMLElement>(`#agent-tab-${state.agentTab}`)?.focus(); });
    root.querySelectorAll<HTMLButtonElement>("[data-copy]").forEach(btn => btn.onclick = () => void copyText(btn.dataset.copy!));
    root.querySelectorAll<HTMLButtonElement>("[data-glossary]").forEach(btn => btn.onclick = () => openGlossary(btn.dataset.glossary || null));
    if (state.page === "activity") bindActivity(s);
    if (state.page === "routines") bindCalendar(s);
    if (state.page === "glossary") bindGlossary(root, {jump: jumpFromGlossary, reducedMotion: reducedMotion()});
    const plugin = pluginPageId(state.page) === null ? null : pluginById(s, pluginPageId(state.page)!);
    if (plugin) bindPluginPage(root, plugin, host);
    bindPluginControls(root, host);
  }

  async function runAction(id: string) {
    if (id === "refresh") { freshBoard(); await reload(); }
    if (id === "add-task") taskDialog();
    if (id === "add-deadline") deadlineDialog();
    if (id === "sync-clear") { if (!window.confirm(t("Clear today’s plan? Your weekly routine stays the same."))) return; await request("sync_clear"); await reload(); state.notice = t("Today’s plan was cleared."); }
    if (id === "open-config") { state.config = await request<ConfigFile>("config_read", {file: "settings"}); draw(); }
    if (id === "config-save") {
      const content = root.querySelector<HTMLTextAreaElement>('[name="configContent"]')!.value;
      const result = await request<ConfigFile & {message: string}>("config_save", {...state.config, content}); state.config = result; await reload(); state.notice = result.message;
    }
    if (id === "shutdown") { if (!window.confirm(t("Stop the local browser service? You can relaunch it whenever you need."))) return; await request("server_shutdown"); root.dispatchEvent(new Event("workspace-closed")); root.innerHTML = `<div class="loading"><h1>${t("See you next time.")}</h1><p>${t("Your local data is saved. Relaunch the browser workspace to return.")}</p></div>`; }
    if (id === "mode") { await request("service_action", {action: "mode", mode: modeOf() === "p" ? "j" : "p"}); await reload(); }
    if (["update", "stop", "switch"].includes(id)) {
      if (id === "stop" && !window.confirm(t("Stop reminder notifications?"))) return;
      const result = await request<{message: string}>("service_action", {action: id, configId: Number(root.querySelector<HTMLSelectElement>('[name="configId"]')?.value)}); state.config = null; await reload(); state.notice = result.message;
    }
    if (id === "schedule-pdf") {
      const result = await request<{url: string; filename: string}>("export_pdf", {kind: "schedule"});
      const blob = await (await fetch(result.url)).blob(); const url = URL.createObjectURL(blob); const link = document.createElement("a"); link.href = url; link.download = result.filename; link.click(); setTimeout(() => URL.revokeObjectURL(url), 60000); state.notice = t("PDF downloaded.");
    }
    if (id === "open-week" && state.activity.summary) { state.activity.view = "week"; state.activity.week = state.activity.summary; draw(true); scrollTop(); }
    if (id === "close-week") { state.activity.view = "gallery"; draw(true); }
    if (id === "clear-day") { state.activity.filter.day = null; redrawGallery(); }
  }
  async function copyText(text: string) {
    try { await navigator.clipboard.writeText(text); state.notice = t("Copied: {text}", {text}); }
    catch { state.notice = t("Could not copy. Select the text and copy it instead."); }
    status();
  }
  const scrollTop = () => { if (document.scrollingElement) document.scrollingElement.scrollTop = 0; };

  function bindTasks(scope: ParentNode, s: Snapshot) {
    scope.querySelectorAll<HTMLButtonElement>("button[data-task]").forEach(btn => btn.onclick = () => {
      const task = s.tasks[Number(btn.dataset.task)];
      if (btn.dataset.operation === "edit") { taskDialog(task); return; }
      if (btn.dataset.operation === "timer") { taskDialog(task, {timed: true}); return; }
      if (btn.dataset.operation === "untime") {
        void action(async () => {
          await flushTask(task.description);
          await request("task_update", {originalDescription: task.description, description: task.description, priority: task.priority, type: task.type, due: ""});
          await reload();
          state.notice = t("“{task}” is a normal task again.", {task: task.description});
        });
        return;
      }
      void action(async () => {
        const kind = btn.dataset.operation as Farewell;
        if (kind !== "deleted" && !window.confirm(kind === "cancelled" ? t("Cancel “{task}”?", {task: task.description}) : t("Drop “{task}”?", {task: task.description}))) return;
        // The task leaves the page while the server is told, its card and its timer together, and the board
        // is redrawn once the space they took has closed.
        const exit = farewell(copiesOf(btn.dataset.task!, btn), kind, btn);
        let result: {undo?: Record<string, unknown>} | undefined;
        try {
          await flushTask(task.description);
          result = await request<{undo?: Record<string, unknown>}>("task_delete", {description: task.description, action: kind});
        }
        catch (error) { exit.revert(); throw error; }
        await exit.done;
        // The last task of a type leaves its group cleared, so the group glides to its new size.
        const emptied = !s.tasks.some(other => other !== task && other.type === task.type);
        await reload({morph: emptied, then: () => tickCounts(task.type)});
        const notice = kind === "deleted" ? t("Completed “{task}”. Nice work.", {task: task.description}) : t("Removed “{task}”.", {task: task.description});
        const token = result?.undo;
        if (token) offerUndo(notice, () => restoreTask(task.description, token));
        else state.notice = notice;
      });
    });
  }
  /** Every copy of a task on the page, by its index: its card on the board and its timer, the one holding
   *  `pressed` first. */
  function copiesOf(index: string, pressed: Element): HTMLElement[] {
    const copies = new Set<HTMLElement>();
    root.querySelectorAll(`[data-task="${index}"], input[data-progress="${index}"]`).forEach(el => {
      const item = el.closest<HTMLElement>(".task-card, .timer");
      if (item) copies.add(item);
    });
    return [...copies].sort((a, b) => Number(b.contains(pressed)) - Number(a.contains(pressed)));
  }
  /** After a task leaves, the counts it was part of tick down: its group's and the one in the sidebar. */
  function tickCounts(type: string) {
    const group = [...root.querySelectorAll<HTMLElement>(".task-group")].find(el => el.dataset.group === type);
    for (const count of [group?.querySelector(".group-count"), root.querySelector(".nav-count")]) count?.classList.add("tick");
  }
  /** Put a completed, cancelled, or dropped task back as it was, and greet it on the board. */
  async function restoreTask(description: string, token: Record<string, unknown>) {
    await request("task_restore", token);
    state.board.added = description;
    await reload();
    state.notice = t("“{task}” is back on your list.", {task: description});
  }
  // ----- Progress, subtasks, and timers ---------------------------------------

  /** Saves of progress and subtasks wait a moment for clicks and key presses to settle, then go out one after
   *  another, so a later one never lands before an earlier one. */
  const waiting = new Map<string, {timer: number; run: () => void}>();
  let saving: Promise<unknown> = Promise.resolve();
  function queue<T>(work: () => Promise<T>): Promise<T> {
    const next = saving.then(work, work);
    saving = next.catch(() => {});
    return next;
  }
  function later(key: string, run: () => void) {
    window.clearTimeout(waiting.get(key)?.timer);
    waiting.set(key, {timer: window.setTimeout(() => { waiting.delete(key); run(); }, 250), run});
  }
  /** Send what is still waiting for a task, and wait until it is saved, before the task itself changes. */
  async function flushTask(description: string) {
    for (const key of [`progress\n${description}`, `subtasks\n${description}`]) {
      const entry = waiting.get(key);
      if (!entry) continue;
      window.clearTimeout(entry.timer);
      waiting.delete(key);
      entry.run();
    }
    await saving;
  }
  const failed = (error: unknown) => {
    state.notice = error instanceof Error ? error.message : String(error);
    status();
    void reload().catch(() => {});
  };
  /** Show a progress value on every copy of a task's slider, on its card and in its timer. */
  function showProgress(index: number, value: number) {
    root.querySelectorAll<HTMLInputElement>(`input[data-progress="${index}"]`).forEach(input => {
      if (input.value !== String(value)) input.value = String(value);
      input.style.setProperty("--p", (value / 100).toFixed(2));
      input.setAttribute("aria-valuetext", `${value}%`);
      const label = input.closest(".progress-control");
      label?.classList.toggle("is-full", value === 100);
      const shown = label?.querySelector("b");
      if (shown) shown.textContent = `${value}%`;
    });
  }
  function bindProgress(scope: ParentNode, s: Snapshot) {
    scope.querySelectorAll<HTMLInputElement>("input[data-progress]").forEach(input => {
      const index = Number(input.dataset.progress);
      const task = s.tasks[index];
      if (!task) return;
      input.addEventListener("input", () => showProgress(index, Number(input.value)));
      input.addEventListener("change", () => setProgress(s, index, Number(input.value), input));
    });
  }
  /** After a task's progress changes on this page: redraw what sums it up (the timers and today's bar, the
   *  board's groups, the workload and its theme, the daily overview), then put keyboard focus back. */
  function refreshProgress(s: Snapshot, focus?: string) {
    redrawTimers(s);
    redrawBoard(s);
    updateLoad(s);
    redrawDaily();
    if (focus) root.querySelector<HTMLElement>(focus)?.focus({preventScroll: true});
  }
  /** Save a task's progress, keeping the slider in hand. */
  function setProgress(s: Snapshot, index: number, value: number, input: HTMLInputElement) {
    const task = s.tasks[index];
    if (!task || task.subtasks?.length || (task.progress ?? 0) === value) return;
    task.progress = value;
    const where = input.closest("#timer-strip") ? "#timer-strip" : "#board-area";
    refreshProgress(s, document.activeElement === input ? `${where} input[data-progress="${index}"]` : undefined);
    const {description} = task;
    progressPending.set(description, value);
    later(`progress\n${description}`, () => {
      const settle = () => { if (progressPending.get(description) === value) progressPending.delete(description); };
      queue(() => request("task_progress", {description, progress: value})).then(settle, (error: unknown) => { settle(); failed(error); });
    });
  }
  /** Change a task's subtasks here at once (its progress follows), and save them a moment later. */
  function changeSubtasks(s: Snapshot, index: number, change: (steps: Subtask[]) => Subtask[], focus?: string) {
    const task = s.tasks[index];
    if (!task) return;
    const before = progressOf(task);
    task.subtasks = change((task.subtasks ?? []).map(step => ({...step})));
    task.progress = progressOf(task);
    refreshProgress(s, focus);
    if (before < 100 && task.subtasks.length && task.progress === 100) {
      const card = [...root.querySelectorAll<HTMLElement>(`#board-area .task-card`)].find(el => el.querySelector(`[data-task="${index}"]`));
      if (card) burst(card.querySelector(".complete-button") ?? card, 26);
    }
    const {description} = task;
    const steps = task.subtasks.map(step => ({...step}));
    subtasksPending.set(description, steps);
    later(`subtasks\n${description}`, () => {
      const settle = () => { if (subtasksPending.get(description) === steps) subtasksPending.delete(description); };
      queue(() => request("task_subtasks", {description, subtasks: steps})).then(settle, (error: unknown) => { settle(); failed(error); });
    });
  }
  /** Checking subtasks off (on a card or a timer), removing them (Undo brings one back), and adding them. */
  function bindSubtasks(scope: ParentNode, s: Snapshot) {
    scope.querySelectorAll<HTMLInputElement>("input[data-subtask]").forEach(box => box.onchange = () => {
      const index = Number(box.dataset.subtask);
      const step = Number(box.dataset.step);
      const done = box.checked;
      const inTimers = !!box.closest("#timer-strip");
      if (done) burst(box.closest(".subtask, .timer-next") ?? box, 10);
      // In a timer the next subtask takes the checked one's place, so focus moves on to it.
      changeSubtasks(s, index, steps => steps.map((item, i) => i === step ? {...item, done} : item),
        inTimers ? `#timer-strip input[data-subtask="${index}"]` : `#board-area input[data-subtask="${index}"][data-step="${step}"]`);
    });
    scope.querySelectorAll<HTMLButtonElement>("[data-subtask-remove]").forEach(btn => btn.onclick = () => {
      const index = Number(btn.dataset.subtaskRemove);
      const step = Number(btn.dataset.step);
      const task = s.tasks[index];
      const removed = task?.subtasks?.[step];
      if (!task || !removed) return;
      const left = task.subtasks!.length - 1;
      changeSubtasks(s, index, steps => steps.filter((_, i) => i !== step),
        left ? `#board-area [data-subtask-remove="${index}"][data-step="${Math.min(step, left - 1)}"]` : `#board-area [data-subtask-open="${index}"]`);
      offerUndo(t("Removed the subtask “{subtask}”.", {subtask: removed.text}), async () => {
        const current = state.snapshot!;
        const at = current.tasks.findIndex(item => item.description === task.description);
        if (at < 0) throw new Error(t("“{task}” is no longer on your list.", {task: task.description}));
        changeSubtasks(current, at, steps => [...steps.slice(0, step), removed, ...steps.slice(step)]);
        state.notice = t("“{subtask}” is back.", {subtask: removed.text});
      });
      status();
    });
    scope.querySelectorAll<HTMLButtonElement>("[data-subtask-open]").forEach(btn => btn.onclick = () => {
      const task = s.tasks[Number(btn.dataset.subtaskOpen)];
      if (!task) return;
      const previous = state.board.subtaskComposer;
      state.board.subtaskComposer = {task: task.description, draft: previous?.task === task.description ? previous.draft : ""};
      redrawBoard(s);
      root.querySelector<HTMLInputElement>('#board-area form.subtask-form [name="subtaskText"]')?.focus();
    });
    scope.querySelectorAll<HTMLFormElement>("form.subtask-form").forEach(form => bindSubtaskForm(form, s));
  }
  function bindSubtaskForm(form: HTMLFormElement, s: Snapshot) {
    const index = Number(form.dataset.subtaskForm);
    const task = s.tasks[index];
    const input = form.querySelector<HTMLInputElement>('[name="subtaskText"]')!;
    if (!task) return;
    const mine = () => state.board.subtaskComposer?.task === task.description ? state.board.subtaskComposer : null;
    const close = () => {
      state.board.subtaskComposer = null;
      redrawBoard(s);
      root.querySelector<HTMLElement>(`#board-area [data-subtask-open="${index}"]`)?.focus({preventScroll: true});
    };
    input.addEventListener("input", () => { const writing = mine(); if (writing) writing.draft = input.value; });
    form.addEventListener("keydown", event => { if (event.key === "Escape") { event.preventDefault(); close(); } });
    form.querySelector<HTMLButtonElement>("[data-subtask-close]")!.onclick = close;
    form.addEventListener("submit", event => {
      event.preventDefault();
      const text = input.value.trim();
      if (!text || (task.subtasks?.length ?? 0) >= 50) {
        if (text) { state.notice = t("A task can have at most {max} subtasks.", {max: 50}); status(); }
        form.classList.remove("shake");
        void form.offsetWidth;
        form.classList.add("shake");
        input.focus();
        return;
      }
      const writing = mine();
      if (writing) writing.draft = "";
      // The field stays open for the next one.
      changeSubtasks(s, index, steps => [...steps, {text, done: false}], '#board-area form.subtask-form [name="subtaskText"]');
    });
  }
  const timerOptions = (s: Snapshot) => ({snapshot: s, now: Date.now(), closed: state.closedTimers, weighted: state.prefs.weighted, extras: pluginTimers(s, Date.now())});
  const timerArea = (s: Snapshot) => `<div id="timer-strip" class="timer-strip">${timerStrip(timerOptions(s))}</div>`;
  /** Redraw only the timers, keeping the row where it was scrolled. */
  function redrawTimers(s: Snapshot) {
    const area = root.querySelector<HTMLElement>("#timer-strip");
    if (!area) return;
    const scrolled = area.querySelector<HTMLElement>(".timer-row")?.scrollLeft ?? 0;
    area.innerHTML = timerStrip(timerOptions(s));
    const row = area.querySelector<HTMLElement>(".timer-row");
    if (row) row.scrollLeft = scrolled;
    bindTimers(area, s);
  }
  function bindTimers(scope: HTMLElement, s: Snapshot) {
    bindProgress(scope, s);
    bindSubtasks(scope, s);
    bindTasks(scope, s);
    bindPluginTimers(scope, host);
    scope.querySelectorAll<HTMLButtonElement>("[data-timer-close]").forEach(btn => btn.onclick = () => {
      const key = btn.dataset.timerClose!;
      const name = btn.closest(".timer")?.querySelector("h4")?.textContent ?? t("this deadline");
      const today = s.today.date;
      const close = () => {
        state.closedTimers.add(key);
        saveClosedTimers(state.closedTimers, today);
        redrawTimers(s);
        offerUndo(t("Closed the timer for “{deadline}”. The deadline itself is unchanged.", {deadline: name}), async () => {
          state.closedTimers.delete(key);
          saveClosedTimers(state.closedTimers, today);
          redrawTimers(state.snapshot ?? s);
          state.notice = t("The timer for “{deadline}” is back.", {deadline: name});
        });
        status();
      };
      const card = btn.closest<HTMLElement>(".timer");
      if (!card || reducedMotion()) { close(); return; }
      card.classList.add("is-leaving");
      let done = false;
      const finish = () => { if (!done) { done = true; close(); } };
      card.addEventListener("animationend", finish, {once: true});
      window.setTimeout(finish, 360);
    });
  }
  /** Advance every running clock and dial once a second. A deadline that ends, a task that turns overdue, a
   *  timer whose urgency changes, or a new day redraws the timers, since what they show changes. */
  let tickDay = "";
  function tick() {
    tickPluginClocks(root, Date.now());
    const s = state.snapshot;
    if (!s || state.page !== "today" || !root.querySelector("#timer-strip")) return;
    const now = Date.now();
    const day = new Date(now).toDateString();
    let changed = tickDay !== "" && tickDay !== day;
    tickDay = day;
    root.querySelectorAll<HTMLElement>("[data-at]").forEach(item => {
      const at = Number(item.dataset.at);
      const kind = item.dataset.kind === "deadline" ? "deadline" : "task";
      if (kind === "deadline" && at <= now) changed = true;
      if (kind === "task" && at <= now && !item.classList.contains("is-overdue")) {
        item.classList.add("is-overdue");
        if (item.closest("#timer-strip")) changed = true;
      }
      if (item.dataset.urgency && item.dataset.urgency !== urgency(kind, at, now, item.hasAttribute("data-ready"))) changed = true;
      item.querySelector("[data-dial]")?.setAttribute("stroke-dasharray", dialDash(runway(kind, at, now)));
      item.querySelectorAll<HTMLElement>("[data-clock]").forEach(el => {
        const text = clockText(el.dataset.clock, at, now);
        if (el.textContent !== text) el.textContent = text;
      });
    });
    if (changed) redrawTimers(s);
  }
  const ticker = window.setInterval(() => { if (root.isConnected) tick(); else window.clearInterval(ticker); }, 1000);
  signal.addEventListener("abort", () => window.clearInterval(ticker));

  // ----- Task board: folding and inline add -----------------------------------

  const boardOptions = (s: Snapshot, opening: string | null = null) => {
    const board = state.board;
    return {snapshot: s, query: state.search, isFolded: (id: string) => (board.view === "folded") !== board.flipped.has(id), composer: board.composer, added: board.added, opening, menu: board.menu, renaming: board.renaming, deleting: board.deleting, newType: board.newType, greet: board.greet, now: Date.now(), order: board.order, layout: state.prefs.layout, subtaskComposer: board.subtaskComposer};
  };
  const spread = () => state.prefs.layout === "spread";
  /** The board, and below it the shelf of types without tasks, in one area that redraws and binds together. */
  const boardArea = (s: Snapshot, flow: boolean) => `<div id="board-area" class="board-area">${spread() ? `<p class="spread-legend">${icons.orbit}<span>${t("Spread by workload: the heaviest group sits in the middle, and the lighter a group, the farther out it sits.")}</span></p>` : ""}<section id="task-board" class="task-board ${flow ? "flow-board" : ""} ${spread() ? "spread-board" : ""} ${uniformBoard() ? "is-uniform" : ""}">${taskBoard(boardOptions(s))}</section><div id="task-shelf">${typeShelf(boardOptions(s))}</div></div>`;
  /** Every group folded: the summaries line up in plain rows instead of the masonry (compact layout only). */
  const uniformBoard = () => !spread() && state.board.view === "folded" && !state.board.flipped.size && !state.search;
  /** Redraw only the board, leaving the page around it as it is. `reveal` animates the contents of every
   *  group, or of one group by ID, as they appear after folding or expanding. */
  function redrawBoard(s: Snapshot, options: {opening?: string; reveal?: true | string; fresh?: boolean} = {}) {
    const area = root.querySelector<HTMLElement>("#board-area");
    const board = area?.querySelector<HTMLElement>("#task-board");
    const shelf = area?.querySelector<HTMLElement>("#task-shelf");
    // A plugin page lists the plugin's tasks instead of the board: it is drawn again as a whole.
    if (!area || !board || !shelf) { if (pluginPageId(state.page) !== null && state.snapshot) draw(); return; }
    syncBoardOrder(s);
    root.querySelector(".content")?.classList.remove("enter");
    board.classList.toggle("is-uniform", uniformBoard());
    board.innerHTML = taskBoard(boardOptions(s, options.opening ?? null));
    shelf.innerHTML = typeShelf(boardOptions(s, options.opening ?? null));
    if (options.reveal) board.querySelectorAll<HTMLElement>(".task-group").forEach(group => group.classList.toggle("reveal", options.reveal === true || group.dataset.group === options.reveal));
    bindBoard(area, s, options.fresh);
  }
  /** Bind the board area: the board's groups and the shelf's types. */
  function bindBoard(scope: HTMLElement, s: Snapshot, fresh = false) {
    const board = scope.querySelector<HTMLElement>("#task-board");
    if (board) (spread() ? spreadOut : masonry)(board, fresh || state.board.fresh);
    bindTasks(scope, s);
    bindProgress(scope, s);
    bindSubtasks(scope, s);
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
    bindSuggestions(scope, s);
    bindTaskExtras(scope, host);
  }

  // ----- Task board: suggestions from AI agents ----------------------------------

  const suggestionsOf = (s: Snapshot) => s.suggestions ?? [];
  function bindSuggestions(scope: HTMLElement, s: Snapshot) {
    const find = (id: string | undefined) => suggestionsOf(s).find(item => item.id === id);
    scope.querySelectorAll<HTMLButtonElement>("[data-suggestion-accept]").forEach(btn => btn.onclick = () => {
      const item = find(btn.dataset.suggestionAccept);
      if (!item) return;
      burst(btn, 16);
      void action(() => acceptSuggestion(item));
    });
    scope.querySelectorAll<HTMLButtonElement>("[data-suggestion-adjust]").forEach(btn => btn.onclick = () => {
      const item = find(btn.dataset.suggestionAdjust);
      if (item) taskDialog(undefined, {suggestion: item});
    });
    scope.querySelectorAll<HTMLButtonElement>("[data-suggestion-reject]").forEach(btn => btn.onclick = () => {
      const item = find(btn.dataset.suggestionReject);
      if (item) rejectSuggestion(item, btn);
    });
  }
  /** Add a suggestion to the user's tasks, as it is or with `adjust` (description, priority, type, due), and greet
   *  it on the board as one of their own. The notice can take it back. */
  async function acceptSuggestion(item: SuggestionItem, adjust: Record<string, unknown> = {}) {
    // Plugin data, such as a workout an agent planned, becomes the task's steps.
    const steps = state.snapshot ? subtasksFor(state.snapshot, String(adjust.type ?? item.type), item.plugin) : [];
    if (steps.length) adjust = {...adjust, subtasks: steps};
    const result = await request<{task: {description: string; type: string}; undo: Record<string, unknown>}>("suggestion_accept", {...adjust, id: item.id});
    const {description, type} = result.task;
    state.board.added = description;
    await reload({morph: true});
    offerUndo(t("Added “{task}” to {type}. It’s one of your tasks now.", {task: description, type: typeName(state.snapshot!, type)}), () => takeBack(item, result.undo));
    status();
  }
  function rejectSuggestion(item: SuggestionItem, pressed: HTMLElement) {
    void action(async () => {
      const exit = leave(pressed.closest<HTMLElement>(".suggestion"));
      let result: {undo: Record<string, unknown>};
      try { result = await request<{undo: Record<string, unknown>}>("suggestion_reject", {id: item.id}); }
      catch (error) { exit.revert(); throw error; }
      await exit.done;
      await reload({morph: true});
      offerUndo(t("Rejected “{task}”. It stays off your list.", {task: item.description}), () => takeBack(item, result.undo));
    });
  }
  /** Undo an accept or a reject: the suggestion waits for review again. */
  async function takeBack(item: SuggestionItem, token: Record<string, unknown>) {
    await request("suggestion_undo", token);
    await reload({morph: true});
    state.notice = t("“{task}” is waiting for your review again.", {task: item.description});
  }
  /** A rejected suggestion folds away while the server is told; `revert` brings it back if that fails. */
  function leave(card: HTMLElement | null): {done: Promise<void>; revert: () => void} {
    if (!card || reducedMotion()) return {done: Promise.resolve(), revert: () => {}};
    card.classList.add("is-leaving");
    const done = new Promise<void>(resolve => {
      card.addEventListener("animationend", () => resolve(), {once: true});
      window.setTimeout(resolve, 420);
    });
    return {done, revert: () => card.classList.remove("is-leaving")};
  }
  /** How many suggestions wait for review, beside the board's heading. */
  function suggestionPill(s: Snapshot) {
    const count = suggestionsOf(s).length;
    if (!count) return "";
    const agents = [...new Set(suggestionsOf(s).map(item => item.agent))].join(", ");
    return `<span class="suggestion-pill" data-tip="${e(t("Suggested by {agents}. They sit in their type’s group, dashed, and join your list only when you add them.", {agents}))}">${icons.spark}${t("{count} suggested", {count})}</span>`;
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
        const tasks = result.movedTasks ? tn("{count} task", "{count} tasks", result.movedTasks) : "";
        const deadlines = result.movedDeadlines ? tn("{count} deadline", "{count} deadlines", result.movedDeadlines) : "";
        const moved = tasks && deadlines ? t("{tasks} and {deadlines}", {tasks, deadlines}) : tasks || deadlines;
        const deleted = t("Deleted the type “{type}”.", {type: name});
        state.notice = moved ? `${deleted} ${t("{moved} moved to “{type}”.", {moved, type: moveTo ? typeName(s, moveTo) : t("no type")})}` : deleted;
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
          state.notice = t("Added the type “{type}”.", {type: created.name});
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
        state.notice = t("Renamed “{old}” to “{new}”.", {old: original, new: name});
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
    root.querySelectorAll(".menu-open").forEach(group => group.classList.remove("menu-open"));
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
  /** Compact or spread: the groups glide to their new places. */
  function setBoardLayout(layout: BoardLayout) {
    if (layout === state.prefs.layout) return;
    state.prefs.layout = layout;
    savePrefs(state.prefs);
    freshBoard();
    morph(() => {
      draw();
      root.querySelector<HTMLElement>(`[data-board-layout="${layout}"]`)?.focus({preventScroll: true});
    });
  }
  function setPref(key: "workloadTheme" | "weighted", on: boolean, s: Snapshot) {
    state.prefs[key] = on;
    savePrefs(state.prefs);
    // Weighing tasks by importance changes today's progress, which the theme follows too.
    applyLoad(s);
    state.notice = key === "workloadTheme"
      ? (on ? t("The theme follows your workload again.") : t("The theme stays the same whatever your workload."))
      : (on ? t("Today’s progress now weighs each task by its importance.") : t("Today’s progress now counts every task the same."));
    status();
  }
  /** The display settings' controls: the layout switches (on the board too), the toggles, and the numbers where
   *  the workload levels start. */
  function bindDisplay(scope: ParentNode, s: Snapshot) {
    scope.querySelectorAll<HTMLButtonElement>("[data-board-layout]").forEach(btn => btn.onclick = () => setBoardLayout(btn.dataset.boardLayout as BoardLayout));
    scope.querySelectorAll<HTMLInputElement>("[data-pref]").forEach(box => box.onchange = () => setPref(box.dataset.pref as "workloadTheme" | "weighted", box.checked, s));
    const fields = [...scope.querySelectorAll<HTMLInputElement>("[data-threshold]")];
    // Typing says at once what is wrong; a number takes effect once it is entered (Enter, the arrows, or leaving
    // the field), and only when all three can start the levels.
    fields.forEach(field => {
      field.oninput = () => readThresholds(fields);
      field.onchange = () => {
        const values = readThresholds(fields);
        if (values) setThresholds(values, s);
      };
    });
    const reset = scope.querySelector<HTMLButtonElement>("[data-thresholds-reset]");
    if (reset) reset.onclick = () => {
      setThresholds(DEFAULT_THRESHOLDS, s);
      // The button is disabled once the defaults are back, so focus moves to the first number.
      fields[0]?.focus({preventScroll: true});
    };
  }
  /** The numbers in the level fields, when they can start the levels; otherwise null, with what is wrong shown
   *  under the fields and each field at fault marked. */
  function readThresholds(fields: HTMLInputElement[]): LoadThresholds | null {
    const values = fields.map(field => field.value.trim() === "" ? NaN : roundLoad(Number(field.value)));
    const problem = thresholdProblem(values);
    const faulty = (value: number, i: number) => !Number.isFinite(value) || value <= 0 || value > MAX_THRESHOLD || (i > 0 && value <= values[i - 1]);
    fields.forEach((field, i) => field.setAttribute("aria-invalid", String(!!problem && faulty(values[i], i))));
    const message = fields[0]?.closest(".load-thresholds")?.querySelector<HTMLElement>(".threshold-problem");
    if (message) { message.textContent = problem ?? ""; message.hidden = !problem; }
    return problem ? null : [values[0], values[1], values[2]];
  }
  /** Start the workload levels at new numbers: the theme, the header, and the Settings card follow at once. */
  function setThresholds(values: LoadThresholds, s: Snapshot) {
    const changed = values.some((value, i) => value !== loadThresholds()[i]);
    if (changed) {
      setLoadThresholds(values);
      state.prefs.loadThresholds = loadThresholds();
      savePrefs(state.prefs);
      updateLoad(s);
    }
    refreshDisplayCard(s);
    if (!changed) return;
    const [steady, busy, heavy] = loadThresholds().map(loadNumber);
    const levels = {steady, busy, heavy};
    state.notice = atDefaults()
      ? t("Workload levels are back to the defaults: Steady from {steady}, Busy from {busy}, Heavy from {heavy}.", levels)
      : t("Workload levels updated: Steady from {steady}, Busy from {busy}, Heavy from {heavy}.", levels);
    status();
  }
  /** Bring the Settings card up to date in place, leaving its fields (and focus, and the rest of the page, such as
   *  an unsaved edit in the configuration editor) as they are. */
  function refreshDisplayCard(s: Snapshot) {
    const card = root.querySelector<HTMLElement>(".display-card");
    if (!card) return;
    const load = workload(s.tasks, s.today.date);
    const scale = card.querySelector(".load-scale");
    if (scale) scale.outerHTML = levelScale(load);
    const now = card.querySelector(".load-now");
    if (now) now.innerHTML = loadNow(load);
    card.querySelectorAll<HTMLInputElement>("[data-threshold]").forEach((field, i) => {
      field.value = loadNumber(loadThresholds()[i]);
      field.setAttribute("aria-invalid", "false");
    });
    const problem = card.querySelector<HTMLElement>(".threshold-problem");
    if (problem) { problem.textContent = ""; problem.hidden = true; }
    const reset = card.querySelector<HTMLButtonElement>("[data-thresholds-reset]");
    if (reset) reset.disabled = atDefaults();
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
      const name = s.taskTypes[type] ?? groupTasks(s).find(group => group.id === type)?.name ?? t("this group");
      const existing = s.tasks.find(task => task.description === description);
      const question = !existing ? "" : existing.type !== type
        ? t("“{task}” is already on your list under {type}. Replace it with this one?", {task: description, type: s.taskTypes[existing.type] ?? existing.typeName})
        : t("“{task}” is already on your list. Replace it with this one?", {task: description});
      if (existing && !window.confirm(question)) { input.focus(); return; }
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
        await reload({morph: !s.tasks.some(task => task.type === type)});
        state.notice = t("Added “{task}” to {type}.", {task: description, type: name});
      });
    });
  }

  async function removeDeadline(deadline: DeadlineItem) {
    await action(async () => { if (!window.confirm(t("Remove “{deadline}”?", {deadline: deadline.event}))) return; await request("deadline_delete", {event: deadline.event}); await reload(); });
  }
  function themeControls() {
    const labels: Record<ThemePreference, {action: string; name: string}> = {
      light: {action: t("Use light theme"), name: t("Light theme")},
      dark: {action: t("Use dark theme"), name: t("Dark theme")},
      system: {action: t("Use system theme"), name: t("Follow system")}
    };
    return `<div class="theme-controls" role="group" aria-label="${t("Color theme")}">${(["light", "dark", "system"] as ThemePreference[]).map(mode => `<button type="button" data-theme-choice="${mode}" aria-label="${labels[mode].action}" aria-pressed="${appearance === mode}" title="${labels[mode].name}">${icons[mode]}</button>`).join("")}</div>`;
  }
  function modeSwitch(mode: Mode) {
    return `<div class="mode-switch ${mode === "p" ? "is-p" : ""}" role="group" aria-label="${t("Planning mode")}"><span class="mode-thumb" aria-hidden="true"></span><button type="button" data-mode-choice="j" aria-pressed="${mode === "j"}" title="${t("J mode · structured day with schedule, deadlines, and timeline")}">${icons.target}<span>J</span></button><button type="button" data-mode-choice="p" aria-pressed="${mode === "p"}" title="${t("P mode · flexible and task-first")}">${icons.feather}<span>P</span></button></div>`;
  }

  // ----- Overview ------------------------------------------------------------

  function overviewJ(s: Snapshot) {
    const complete = s.habits.filter(h => h.completed).length;
    const overdue = s.deadlines.filter(d => d.daysLeft !== null && d.daysLeft < 0).length;
    const dueToday = s.deadlines.filter(d => d.daysLeft === 0).length;
    const highImportance = s.tasks.filter(t => t.priority >= 8 && (!t.alarmFrom || t.alarmFrom <= s.today.date)).length;
    const metrics = [
      {id: "overdue", label: t("Overdue"), value: overdue, detail: t("Past the due date"), tone: overdue ? "danger" : "neutral", image: icons.flag},
      {id: "today", label: t("Due today"), value: dueToday, detail: t("Due by end of day"), tone: dueToday ? "warning" : "neutral", image: icons.schedule},
      {id: "priority", label: t("High importance"), value: highImportance, detail: t("Ready now"), tone: highImportance ? "priority" : "neutral", image: icons.zap},
      {id: "habits", label: t("Habits complete"), value: `${complete}/${s.habits.length}`, detail: s.habits.length ? t("Your daily routine") : t("No habits configured"), tone: complete ? "success" : "neutral", image: icons.activity}
    ];
    return `<div class="page-heading"><div><p class="eyebrow">${icons.target} ${t("TODAY’S OVERVIEW · J MODE")}</p><h1>${t("Your day at a glance")}<span class="accent-dot">.</span></h1><p class="muted">${t("{tasks} open tasks · {deadlines} deadlines · {done}/{habits} habits complete", {tasks: s.tasks.length, deadlines: s.deadlines.length, done: complete, habits: s.habits.length})}</p></div>${button("add-task", t("New task"), icons.plus, "button primary")}</div>
      <section class="status-grid" aria-label="${t("Today at a glance")}">${metrics.map((m, i) => `<article class="status-card status-${m.tone}" data-metric="${m.id}" style="--i:${i}${m.id === "habits" ? `; --p:${s.habits.length ? complete / s.habits.length : 0}` : ""}"><span class="orb" aria-hidden="true"></span><div class="status-label">${m.image}<span>${m.label}</span></div><strong class="status-value">${m.value}</strong><p>${m.detail}</p></article>`).join("")}</section>
      <section class="overview-grid"><article class="focus-card"><div class="aurora" aria-hidden="true"><i></i><i></i><i></i></div><div class="focus-top"><span class="focus-label"><span class="live-dot"></span> ${t("RIGHT NOW")}</span><span class="pill">${s.schedule.hasSyncedOverlay ? t("Your plan") : t("Weekly routine")}</span></div><h2>${e(s.schedule.current ?? (s.schedule.isSkipped ? t("Schedule paused today") : t("No active block")))}</h2><div class="next-block"><span>${icons.clock} ${t("UP NEXT")}</span><strong>${e(s.schedule.next ?? t("No upcoming blocks"))}</strong>${s.schedule.timeToNext ? `<span class="next-time">${t("In {time}", {time: e(s.schedule.timeToNext)})}</span>` : ""}</div><button class="focus-link" type="button" data-page="schedule">${t("View today’s schedule")} ${icons.arrow}</button><svg class="focus-wave" viewBox="0 0 400 60" preserveAspectRatio="none" aria-hidden="true"><path d="M0 34 C 70 4, 130 60, 210 30 S 340 6, 400 28 L400 60 L0 60Z"/></svg></article>
      <article class="card deadline-radar"><div class="section-heading compact"><h2>${t("Next deadlines")} <span class="count">${s.deadlines.length}</span></h2>${button("add-deadline", t("Add"), icons.plus, "text-button")}</div>${deadlineList(s, 3)}<button class="text-button view-all" type="button" data-page="routines">${t("Open the year calendar")} ${icons.arrow}</button></article></section>
      ${dayPanel(dayOptions(s))}
      ${taskSection(s)}
      <article class="card timeline-card"><div class="section-heading compact"><h3>${icons.schedule} ${t("Today’s timeline")}</h3><button class="text-button" type="button" data-page="schedule">${t("Full schedule")} ${icons.arrow}</button></div>${timeline(s)}</article>`;
  }
  const searchPill = () => `<label class="search-pill">${icons.search}<input class="search" name="search" aria-label="${t("Search tasks")}" placeholder="${t("Find a task…")}" value="${e(state.search)}"></label>`;
  function overviewP(s: Snapshot) {
    return `<div class="page-heading p-heading"><div><p class="eyebrow">${icons.feather} ${t("P MODE · GO WITH THE FLOW")}</p><h1>${t("Tasks by type")}<span class="accent-dot">.</span></h1><p class="muted">${t("{tasks} open tasks. No clock to chase. Pick whatever pulls you in.", {tasks: s.tasks.length})}${suggestionPill(s)}</p></div><div class="heading-actions">${searchPill()}${foldSwitch(state.board.view)}${layoutSwitch(state.prefs.layout)}${button("add-task", t("New task"), icons.plus, "button primary")}</div></div>
      ${timerArea(s)}${boardArea(s, true)}${dayPanel(dayOptions(s))}`;
  }
  function taskSection(s: Snapshot) {
    return `<div class="section-heading board-heading"><div><h2>${t("Tasks by type")} <span class="count">${s.tasks.length}</span>${suggestionPill(s)}</h2><p class="muted">${spread() ? t("Groups sit by workload: the heaviest in the middle, lighter ones farther out.") : t("Groups with more important work come first.")} ${t("They keep their places while you work here, and are arranged anew when you come back.")}</p></div><div class="board-tools">${searchPill()}${foldSwitch(state.board.view)}${layoutSwitch(state.prefs.layout)}</div></div>${timerArea(s)}${boardArea(s, false)}`;
  }
  function timeline(s: Snapshot) {
    const events = s.schedule.events;
    if (!events.length) return empty(t("No blocks scheduled for today. Build your routine in Schedule."));
    const now = new Date();
    const minutes = now.getHours() * 60 + now.getMinutes();
    let current = -1;
    if (s.today.date === isoDay(now)) events.forEach((event, i) => { const [hour, minute] = event.time.split(":").map(Number); if (hour * 60 + minute <= minutes) current = i; });
    return `<ol class="timeline">${events.map((event, i) => `<li class="${i < current ? "is-past" : ""} ${i === current ? "is-now" : ""}" style="--i:${i}"><time>${e(event.time)}</time><span class="timeline-dot ${event.syncable ? "work" : ""}"></span><span>${e(event.label)}</span>${i === current ? `<span class="pill now-pill">${t("Now")}</span>` : event.syncable ? `<span class="pill light">${t("Focus")}</span>` : ""}</li>`).join("")}</ol>`;
  }
  function deadlineList(s: Snapshot, limit?: number) {
    const deadlines = [...s.deadlines].sort((a, b) => (a.daysLeft ?? Infinity) - (b.daysLeft ?? Infinity));
    return deadlines.length ? `<ul class="deadline-list">${(limit ? deadlines.slice(0, limit) : deadlines).map(d => {
      const index = s.deadlines.indexOf(d);
      const label = d.daysLeft === null ? t("Check date") : d.daysLeft === 0 ? t("Due today") : d.daysLeft < 0 ? t("{days}d overdue", {days: -d.daysLeft}) : d.daysLeft === 1 ? t("Tomorrow") : t("{days}d left", {days: d.daysLeft});
      return `<li class="deadline-${e(d.status)} ${sw(deadlineSwatch(d, s.taskTypes))}"><span class="ddl-dot" aria-hidden="true"></span><div class="deadline-copy"><strong>${e(d.event)}</strong><div class="deadline-meta"><time>${e(d.deadline)}</time>${repeatBadge(d.repeat)}<span class="type-badge ${d.type ? sw(typeSwatch(s.taskTypes, d.type)) : "untyped"}">${e(d.typeName ?? d.type ?? t("Uncategorized"))}</span></div></div><span class="deadline-status ${e(d.status)}">${label}</span><button class="text-button" data-deadline="${index}" data-operation="edit" aria-label="${t("Edit {deadline}", {deadline: e(d.event)})}" type="button">${t("Edit")}</button><button class="icon-button danger" data-deadline="${index}" data-operation="remove" aria-label="${t("Remove {deadline}", {deadline: e(d.event)})}" type="button">${icons.close}</button></li>`;
    }).join("")}</ul>` : empty(t("No deadlines to track."));
  }

  // ----- Schedule, routines, settings -------------------------------------------

  function schedulePage(s: Snapshot) {
    const agent = AGENT_SETUP[state.agentTab];
    const planCard = s.schedule.hasSyncedOverlay ? `<article class="card plan-card"><p class="eyebrow">${icons.spark} ${t("TODAY’S PLAN")}</p><h2>${t("Your focus blocks have a purpose.")}</h2><p class="muted">${t("Your AI agent planned today. Each focus block in the timeline shows its task, and reminders announce it. Your weekly routine is unchanged.")}</p><div class="button-row">${button("sync-clear", t("Clear today’s plan"), icons.close, "button ghost danger")}</div></article>` : "";
    // The server names the week and the day in English (odd/even, monday); show them in the interface language.
    const parity = s.today.parity === "odd" ? t("odd week") : s.today.parity === "even" ? t("even week") : `${e(s.today.parity)} week`;
    const weekday = formatDate(s.today.date, {weekday: "long"}).toLowerCase();
    return `<div class="page-heading"><div><p class="eyebrow">${t("A RHYTHM THAT WORKS FOR YOU")}</p><h1>${t("Make a little space")}<span class="accent-dot">.</span></h1><p class="muted">${parity} · ${e(weekday)} · ${t("schedule {id}", {id: s.config.activeId})}</p></div>${button("schedule-pdf", t("Download schedule"), icons.schedule)}</div><section class="split-grid"><article class="card timeline-card"><div class="section-heading compact"><h2>${t("Today’s timeline")}</h2><span class="pill light">${t("{count} blocks", {count: s.schedule.events.length})}</span></div>${timeline(s)}</article><div>${planCard}
      <article class="card agent-card"><p class="eyebrow">${icons.spark} ${t("PLAN WITH YOUR AI AGENT")}</p><h2>${t("Let your coding agent build and plan your schedule.")}</h2><p class="muted">${t("Install the Schedule Everything plugin in Claude Code, Codex, or Antigravity. It interviews you, writes your weekly routine, fills today’s focus blocks, and suggests tasks from your email and calendar for you to review on the board, through the local <code>rmd</code> CLI. Your agent sends what it reads to its own model provider.")}</p>
        <div class="agent-tabs" role="tablist" aria-label="${t("Choose your agent")}">${(Object.keys(AGENT_SETUP) as AgentTab[]).map(id => `<button type="button" role="tab" id="agent-tab-${id}" class="agent-tab" data-agent-tab="${id}" aria-selected="${state.agentTab === id}" aria-controls="agent-panel">${e(AGENT_SETUP[id].name)}</button>`).join("")}</div>
        <div class="agent-panel" role="tabpanel" id="agent-panel" aria-labelledby="agent-tab-${state.agentTab}"><ol class="agent-steps">${agent.steps.map((command, i) => `<li><span class="step-num" aria-hidden="true">${i + 1}</span><code>${e(command)}</code><button type="button" class="icon-button" data-copy="${e(command)}" aria-label="${t("Copy command {n}", {n: i + 1})}">${icons.copy}</button></li>`).join("")}</ol><p class="muted">${e(t(agent.note))}</p></div>
        <p class="eyebrow try-label">${t("THEN ASK YOUR AGENT")}</p><div class="prompt-chips">${AGENT_PROMPTS.map(key => t(key)).map(prompt => `<button type="button" class="chip prompt-chip" data-copy="${e(prompt)}" title="${t("Copy prompt")}">${icons.spark}${e(prompt)}</button>`).join("")}</div></article></div></section>`;
  }
  function routines(s: Snapshot) {
    const done = s.habits.filter(h => h.completed).length;
    return `<div class="page-heading"><div><p class="eyebrow">${t("SMALL THINGS, STEADY PROGRESS")}</p><h1>${t("Routines & deadlines")}<span class="accent-dot">.</span></h1><p class="muted">${t("Plant deadlines on your year, color them, and keep your daily habits close.")}</p></div>${button("add-deadline", t("New deadline"), icons.plus, "button primary")}</div>
      ${yearCalendar({deadlines: s.deadlines, taskTypes: s.taskTypes, today: s.today.date, state: state.calendar, conferences: state.conferences})}
      <section class="split-grid"><article class="card habits-card"><div class="section-heading"><h2>${t("Today’s habits")}</h2><span class="pill light">${t("{done}/{total} complete", {done, total: s.habits.length})}</span></div>${s.habits.length ? `<div class="habit-hero">${ring({value: done / s.habits.length, size: 96, label: t("{done} of {total} habits complete", {done, total: s.habits.length}), center: `${done}/${s.habits.length}`})}<p class="muted">${done === s.habits.length ? t("Every habit done today. Lovely.") : t("{count} to go. Small and steady.", {count: s.habits.length - done})}</p></div>${s.habits.map((h, i) => `<label class="habit-row ${h.completed ? "done" : ""}" style="--i:${i}"><input type="checkbox" data-habit="${e(h.id)}" ${h.completed ? "checked" : ""}><span class="habit-check" aria-hidden="true">${icons.check}</span><span class="habit-text">${e(h.description)}</span></label>`).join("")}` : empty(t("Add habits in Settings to build your daily rhythm."))}<p class="muted">${t("Edit your habit definitions in Settings → Habits.")}</p></article><article class="card"><h2>${t("All deadlines")}</h2>${deadlineList(s)}</article></section>`;
  }
  function activity(s: Snapshot) {
    const a = state.activity;
    if (a.view === "week" && a.week) return `<div class="page-heading"><div><p class="eyebrow">${icons.spark} ${t("WEEKLY OVERVIEW")}</p><h1>${t("Your week in color")}<span class="accent-dot">.</span></h1><p class="muted">${t("Compared with last week and the weeks before it.")}</p></div></div>${weeklyView(a.week)}`;
    const intro = a.layout === "tree" ? t("Everything you finished, cancelled, or let go, growing along one branch of time. Hover over any event to see its story.") : t("A gallery of everything you finished, cancelled, or let go. Hover over any piece to see its story.");
    const bar = a.items ? categoryBar(s, a.items, a.filter.type) : "";
    const panel = a.items && a.filter.type ? categoryPanel(s, a.items, a.filter.type, Date.now()) : "";
    return `<div class="page-heading"><div><p class="eyebrow">${t("LOOK HOW FAR YOU’VE COME")}</p><h1>${t("Little wins add up")}<span class="accent-dot">.</span></h1><p class="muted">${intro}</p></div></div>${galleryView({items: a.items, overview: a.summary, filter: a.filter, layout: a.layout, region: a.items ? historyRegion(s) : "", today: s.today.date, taskTypes: s.taskTypes, categoryBar: bar, categoryPanel: panel})}`;
  }
  /** Display choices for this browser: the board's layout, the workload theme (with what workload means, its
   *  levels, and where each level starts), and how today's progress counts tasks. */
  function displayCard(s: Snapshot) {
    const load = workload(s.tasks, s.today.date);
    const {prefs} = state;
    const toggle = (key: "workloadTheme" | "weighted", title: string, detail: string) => `<label class="setting-toggle"><input type="checkbox" data-pref="${key}" ${prefs[key] ? "checked" : ""}><span class="timed-switch" aria-hidden="true"></span><span class="setting-copy"><strong>${title}</strong><small>${detail}</small></span></label>`;
    return `<article class="card display-card"><h2>${icons.palette} ${t("Board, workload & progress")}</h2><p class="muted">${t("Saved in this browser.")}</p>
      <div class="setting-block"><div class="setting-copy"><strong>${t("Task board layout")}</strong><small>${t("Compact packs the groups in rows, those with more important work first. Spread places them by workload: the heaviest in the middle, lighter ones farther out.")}</small></div>${layoutSwitch(prefs.layout)}</div>
      ${toggle("workloadTheme", t("Theme follows your workload"), t("Colors and the background shift from cool and calm to warm as open work piles up, and ease partway back toward calm as today’s progress bar fills."))}
      ${levelScale(load)}
      ${thresholdFields()}
      <p class="muted load-now">${loadNow(load)}</p>
      ${toggle("weighted", t("Weigh today’s progress by importance"), t("An importance-9 task moves today’s progress three times as much as an importance-3 one. Off, every task counts the same."))}
    </article>`;
  }
  /** The five levels with their ranges, the current one marked. */
  function levelScale(load: Workload) {
    const ranges = levelRanges();
    return `<div class="load-scale" role="list" aria-label="${t("Workload levels")}">${loadLevels().map((level, i) => `<span role="listitem" class="load-step load-${level.id} ${level.id === load.level ? "is-now" : ""}">${level.icon}<b>${e(level.label)}</b><small>${ranges[i]}</small></span>`).join("")}</div>`;
  }
  const loadNow = (load: Workload) => `${e(t(WORKLOAD_DEFINITION))} ${t("Right now it is <b>{points}</b>: {level}.", {points: load.points.toFixed(1), level: e(levelInfo(load.level).label.toLowerCase())})}`;
  const atDefaults = () => loadThresholds().every((value, i) => value === DEFAULT_THRESHOLDS[i]);
  /** Where Steady, Busy, and Heavy start, one number each, and a way back to the defaults. */
  function thresholdFields() {
    const field = (i: number, id: "steady" | "busy" | "heavy") => {
      const info = levelInfo(id);
      return `<label class="threshold load-${id}"><span class="threshold-name">${info.icon}${t("{level} from", {level: e(info.label)})}</span><input type="number" inputmode="decimal" min="0" max="${MAX_THRESHOLD}" step="0.5" value="${loadNumber(loadThresholds()[i])}" data-threshold="${i}" aria-describedby="threshold-help" aria-invalid="false"></label>`;
    };
    return `<div class="load-thresholds" role="group" aria-labelledby="threshold-title">
        <div class="setting-copy"><strong id="threshold-title">${t("Where each level starts")}</strong><small id="threshold-help">${t("A workload at or above a number is in that level. Light is anything above 0 and below Steady.")}</small></div>
        <div class="threshold-fields">${field(0, "steady")}${field(1, "busy")}${field(2, "heavy")}</div>
        <div class="threshold-foot"><p class="threshold-problem" role="alert" hidden></p><button type="button" class="text-button" data-thresholds-reset ${atDefaults() ? "disabled" : ""}>${icons.undo}<span>${t("Reset to {values}", {values: DEFAULT_THRESHOLDS.map(loadNumber).join(" · ")})}</span></button></div>
      </div>`;
  }
  /** The interface language: one setting (`[settings] language`) for this workspace, the CLI, and reminder popups.
   *  Each name is written in its own language, so anyone can find theirs. */
  function languageCard() {
    return `<article class="card language-card"><h2>${icons.language} ${t("Interface language")}</h2><p class="muted">${t("Used by this workspace, the rmd command line, and reminder popups. Saved in settings.toml.")}</p>
      <div class="layout-switch language-switch" role="group" aria-label="${t("Interface language")}">${LANGUAGES.map(entry => `<button type="button" data-language="${entry.id}" lang="${entry.locale}" aria-pressed="${entry.id === language()}">${entry.name}</button>`).join("")}</div></article>`;
  }
  function settings(s: Snapshot) {
    const files = {settings: t("General settings"), odd: t("Odd week schedule"), even: t("Even week schedule"), habits: t("Habits"), deadlines: t("Deadlines"), profile: t("Profile")};
    return `<div class="page-heading"><div><p class="eyebrow">${t("MAKE IT YOURS")}</p><h1>${t("Your workspace, your rules")}<span class="accent-dot">.</span></h1><p class="muted">${t("All changes are saved to your computer.")}</p></div></div><section class="split-grid"><div>${languageCard()}${pluginsCard(s)}<article class="card"><h2>${t("Reminder preferences")}</h2><p class="muted">${modeOf() === "p" ? t("P mode · selected event alarms paused · overview shows tasks only") : t("J mode · all reminders allowed · full structured overview")}</p>${button("mode", modeOf() === "p" ? t("Switch to J mode") : t("Switch to P mode"), icons.settings)}<hr><label>${t("Active schedule")}<select name="configId">${state.workspace?.configIds?.map(id => `<option value="${id}" ${id === s.config.activeId ? "selected" : ""}>${t("Schedule {id}", {id})}</option>`).join("") ?? ""}</select></label><div class="button-row">${button("switch", t("Activate"))}${button("update", t("Reload reminders"), icons.refresh)}${button("stop", t("Stop reminders"), "", "button ghost danger")}</div><p class="muted path">${e(s.config.rootDir)}</p><hr><h3>${t("Task & deadline types")}</h3><p class="muted">${t("Types are managed on the task board: add one with New type on the shelf below the board, and rename or delete one from its ⋯ menu. Deadlines share the same types.")}</p><div class="button-row"><button type="button" class="button ghost" data-page="today">${icons.tasks}<span>${t("Open the task board")}</span></button></div><hr>${button("shutdown", t("Close browser service"), "", "button ghost")}</article>${displayCard(s)}</div><article class="card"><h2>${t("Configuration editor")}</h2><p class="muted">${t("Edit your schedules, habits, settings, or profile here. Syntax is checked before saving.")}</p>${state.config ? `<label>${t("File")}<select name="configFile">${Object.entries(files).map(([id, label]) => `<option value="${id}" ${id === state.config!.file ? "selected" : ""}>${label}</option>`).join("")}</select></label><label>${t("Contents")}<textarea class="code-editor" name="configContent" rows="22" spellcheck="false">${e(state.config.content)}</textarea></label>${button("config-save", t("Save changes"), icons.check, "button primary")}` : button("open-config", t("Open editor"), icons.settings)}<p class="muted">${t("Saved edits reload the installed reminder service when available.")}</p></article></section>`;
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
    if (!visible.length) return empty(items.length ? t("Nothing matches these filters. Try another outcome or type.") : t("Your first small win will appear here."));
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
    root.querySelectorAll<HTMLButtonElement>("[data-category]").forEach(btn => btn.onclick = () => {
      const category = btn.dataset.category || null;
      a.filter.type = a.filter.type === category ? null : category;
      a.filter.day = null;
      resetPaging();
      redrawGallery();
      root.querySelector<HTMLElement>(`[data-category="${btn.dataset.category}"]`)?.focus({preventScroll: true});
    });
    bindCategoryPanel(root, host, a.filter.type);
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
    el.setAttribute("aria-label", t("Task details"));
    const close = () => { el.close(); el.remove(); root.querySelector<HTMLElement>(`[data-history="${items.indexOf(visible[position])}"]`)?.focus(); };
    const render = (direction = "") => {
      el.innerHTML = `<button type="button" class="icon-button lb-close" aria-label="${t("Close")}">${icons.close}</button><div class="lb-inner ${direction}">${lightbox(visible[position], items, {index: position, total: visible.length}, s.taskTypes)}</div>`;
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
      state.notice = t("Moved “{deadline}” to {date}.", {deadline: deadline.event, date: formatDate(day, {weekday: "short", month: "short", day: "numeric"})});
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
        state.notice = t("Added “{deadline}” to your deadlines.", {deadline: name});
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
      if (existing && existing.deadline !== day && !window.confirm(t("“{deadline}” is already due {date}. Move it to this day?", {deadline: name, date: existing.deadline}))) return;
      void action(async () => {
        await request("deadline_add", {event: name, date: day, type: data.get("popType") ?? "", color: data.get("popColor") ?? "", repeat: data.get("popRepeat") ?? ""});
        state.calendar.day = null;
        state.calendar.flash = day;
        await reload();
        state.notice = t("Added “{deadline}” on {date}.", {deadline: name, date: formatDate(day, {weekday: "short", month: "short", day: "numeric"})});
      });
    });
  }

  // ----- Dialogs ---------------------------------------------------------------

  function dialog(title: string, body: string, submit: (data: FormData) => Promise<void>, submitLabel = t("Save")) {
    root.querySelector("dialog.editor-dialog")?.remove();
    const el = document.createElement("dialog"); el.className = "editor-dialog"; el.innerHTML = `<form><div class="section-heading"><h2>${title}</h2><button class="icon-button" type="button" aria-label="${t("Close")}">${icons.close}</button></div>${body}<div class="button-row"><button class="button primary" type="submit">${e(submitLabel)}</button><button class="button ghost" type="button" data-close>${t("Cancel")}</button></div><p class="dialog-error" role="alert"></p></form>`;
    root.append(el); el.showModal();
    el.querySelectorAll<HTMLButtonElement>('button[type="button"]').forEach(btn => btn.onclick = () => { el.close(); el.remove(); });
    el.querySelector("form")!.onsubmit = event => { event.preventDefault(); const btn = el.querySelector<HTMLButtonElement>('[type="submit"]')!; btn.disabled = true; void submit(new FormData(event.target as HTMLFormElement)).then(() => { el.close(); el.remove(); }).catch(error => { el.querySelector(".dialog-error")!.textContent = error instanceof Error ? error.message : String(error); btn.disabled = false; }); };
  }
  /** The task form. A timed task has a finish-by time: due by tonight, it gets a timer above the board.
   *  `timed` opens the form with the timer on and its time in focus. With a `suggestion`, the form starts from
   *  what an agent suggested, and saving adds it to the user's tasks as they adjusted it. */
  function taskDialog(task?: TaskItem, options: {timed?: boolean; suggestion?: SuggestionItem} = {}) {
    const offer = task ? undefined : options.suggestion;
    const draft = task ?? offer;
    const timed = options.timed ?? !!draft?.due;
    const timer = `<fieldset class="timed-field ${timed ? "is-on" : ""}"><legend class="sr-only">${t("Timer")}</legend><label class="timed-toggle"><input type="checkbox" name="timed" ${timed ? "checked" : ""}><span class="timed-switch" aria-hidden="true"></span><span class="timed-copy"><strong>${icons.clock}${t("Timed task")}</strong><small>${t("Set when it should be finished. Due by tonight, it gets a timer above the board.")}</small></span></label><label class="timed-due">${t("Finish by")}<input type="datetime-local" name="due" value="${e(draft?.due ?? defaultDue())}" ${timed ? "required" : "disabled"}></label></fieldset>`;
    const why = offer ? `<p class="dialog-suggestion">${icons.spark}<span>${suggestedBy(offer)}</span></p>` : "";
    dialog(task ? t("Edit task") : offer ? t("Adjust, then add it") : t("A new thing to work on"), `${why}${field("description", t("Task"), "text", draft?.description ?? "", 'required maxlength="500"')}${field("priority", t("Importance · 1 to 10"), "number", String(draft?.priority ?? 5), 'required min="1" max="10"')}<label>${t("Task type")}<select name="type">${typeOptions(state.snapshot!, draft?.type)}</select></label>${timer}`, async data => {
      const due = data.get("timed") ? String(data.get("due") ?? "") : "";
      if (offer) { await acceptSuggestion(offer, {description: data.get("description"), priority: Number(data.get("priority")), type: data.get("type"), due}); return; }
      if (task) await flushTask(task.description);
      await request(task ? "task_update" : "task_add", {description: data.get("description"), priority: Number(data.get("priority")), type: data.get("type"), due, originalDescription: task?.description});
      await reload();
      if (due && due !== task?.due) { state.notice = t("“{task}” is timed: finish by {time}.", {task: String(data.get("description")), time: formatDate(due, {weekday: "short", hour: "numeric", minute: "2-digit"})}); status(); }
    }, offer ? t("Add to my tasks") : t("Save"));
    const form = root.querySelector<HTMLElement>("dialog.editor-dialog .timed-field");
    const toggle = form?.querySelector<HTMLInputElement>('[name="timed"]');
    const input = form?.querySelector<HTMLInputElement>('[name="due"]');
    if (!form || !toggle || !input) return;
    toggle.onchange = () => {
      form.classList.toggle("is-on", toggle.checked);
      input.disabled = !toggle.checked;
      input.required = toggle.checked;
      if (toggle.checked) input.focus();
    };
    if (options.timed) input.focus();
  }
  /** Who suggested a task, from where, and why, as one sentence in the interface language. */
  function suggestedBy(offer: SuggestionItem): string {
    const vars = {agent: e(offer.agent), source: e(offer.source ?? ""), reason: e(offer.reason ?? "")};
    if (offer.source) return offer.reason ? t("Suggested by <b>{agent}</b> from {source}: {reason}", vars) : t("Suggested by <b>{agent}</b> from {source}.", vars);
    return offer.reason ? t("Suggested by <b>{agent}</b>: {reason}", vars) : t("Suggested by <b>{agent}</b>.", vars);
  }
  /** A finish time to start from: two hours ahead, on the hour, and no later than the end of today. */
  function defaultDue(): string {
    const now = new Date();
    const due = new Date(now);
    due.setHours(now.getHours() + 2, 0, 0, 0);
    if (isoDay(due) !== isoDay(now)) { due.setTime(now.getTime()); due.setHours(23, 59, 0, 0); }
    return `${isoDay(due)}T${String(due.getHours()).padStart(2, "0")}:${String(due.getMinutes()).padStart(2, "0")}`;
  }
  function deadlineDialog(deadline?: DeadlineItem) {
    const selected = deadline?.type ?? "";
    const color = deadline?.color;
    const unknown = selected && !(selected in state.snapshot!.taskTypes) ? `<option value="${e(selected)}" selected>${t("{type} (unavailable)", {type: e(deadline?.typeName ?? selected)})}</option>` : "";
    dialog(deadline ? t("Edit deadline") : t("New deadline"), `${field("event", t("Event"), "text", deadline?.event ?? "", "required")}${field("date", t("Due date"), "date", deadline?.deadline ?? state.snapshot!.today.date, "required")}<label>${t("Deadline type")}<select name="type"><option value="">${t("Uncategorized")}</option>${typeOptions(state.snapshot!, selected)}${unknown}</select></label><label>${t("Repeat")}<select name="repeat">${repeatOptions(deadline?.repeat)}</select></label>${swatchPicker("color", isSwatch(color) ? color : "", t("Match type color"))}`, async data => {
      await request(deadline ? "deadline_update" : "deadline_add", {event: data.get("event"), date: data.get("date"), type: data.get("type"), color: data.get("color") ?? "", repeat: data.get("repeat") ?? "", originalEvent: deadline?.event});
      state.calendar.day = null;
      await reload();
    });
  }

  root.addEventListener("workspace-refresh", () => { if (state.busy || document.hidden || root.querySelector("dialog[open]") || state.calendar.day || state.conferences.menuOpen || state.board.menu || state.board.renaming || state.board.deleting || state.board.newType || state.board.subtaskComposer || ["settings", "schedule", "activity", "glossary"].includes(state.page) || pluginPageId(state.page) !== null || ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName ?? "")) return; void reload().catch(() => { state.notice = t("Could not refresh. Check the local server connection."); status(); }); }, {signal});
  root.addEventListener("workspace-activate", () => { state.notice = t("Your existing workspace is ready."); status(); }, {signal});
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
  try { await reload(); } catch (error) { root.innerHTML = `<div class="loading"><h1>${t("Could not open your workspace")}</h1><p>${e(error instanceof Error ? error.message : String(error))}</p><button class="button" id="retry">${t("Try again")}</button></div>`; root.querySelector("#retry")?.addEventListener("click", () => void renderApp(root, client)); }
}
