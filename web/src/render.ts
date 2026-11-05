// Browser workspace shell: application state, page layouts, and event binding.
// J mode shows the full structured overview; P mode keeps only the tasks-by-type board.
import { defaultGalleryFilter, filterHistory, galleryGroups, galleryView, lightbox, weeklyView, type GalleryFilter, type HistoryStatus, type HistorySort } from "./activity";
import { deadlineSwatch, yearCalendar, type CalendarState } from "./calendar";
import { ring } from "./charts";
import { bindTilt, burst } from "./effects";
import { applyTheme, setThemePreference, themePreference, type ThemePreference } from "./theme";
import type { BridgeClient, DeadlineItem, HistoryItem, Snapshot, SyncProposal, TaskItem, WeeklyOverview, Workspace } from "./types";
import { addDays, button, e, empty, escapeHtml, field, formatDate, hash, icons, isoDay, isSwatch, sw, swatchPicker, typeSwatch } from "./ui";

export { escapeHtml };

type Page = "today" | "schedule" | "routines" | "activity" | "settings";
type Mode = "j" | "p";
type ConfigFile = {file: string; content: string; revision: string; activeId: number};
type SetupTurn = {sessionId: string; phase: string; conversation: string; question_to_user: string | null; schedule_summary: string | null; profile_markdown: string | null; bundle: Record<string, string> | null};
type ActivityState = {view: "gallery" | "week"; items: HistoryItem[] | null; summary: WeeklyOverview | null; week: WeeklyOverview | null; filter: GalleryFilter; limit: number; loading: boolean};
type State = {page: Page; snapshot: Snapshot | null; workspace: Workspace | null; sync: SyncProposal | null; feedback: string[]; setup: SetupTurn | null; messages: {role: string; text: string}[]; search: string; busy: boolean; notice: string; config: ConfigFile | null; activity: ActivityState; calendar: CalendarState};

const PAGES: [Page, string][] = [["today", "Overview"], ["schedule", "Schedule"], ["routines", "Routines & deadlines"], ["activity", "Activity"], ["settings", "Settings"]];
const GALLERY_PAGE = 60;
const typeOptions = (snapshot: Snapshot, selected = "") => Object.entries(snapshot.taskTypes).map(([id, name]) => `<option value="${e(id)}" ${id === selected ? "selected" : ""}>${e(name)}</option>`).join("");
const isOverview = (value: unknown): value is WeeklyOverview => !!value && Array.isArray((value as WeeklyOverview).days) && Array.isArray((value as WeeklyOverview).trend);

export function groupTasks(snapshot: Snapshot): {id: string; name: string; tasks: TaskItem[]}[] {
  const groups = new Map<string, {id: string; name: string; tasks: TaskItem[]}>();
  for (const task of snapshot.tasks) {
    if (!groups.has(task.type)) groups.set(task.type, {id: task.type, name: snapshot.taskTypes[task.type] ?? task.typeName ?? "Other", tasks: []});
    groups.get(task.type)!.tasks.push(task);
  }
  return [...groups.values()].sort((a, b) => a.id.localeCompare(b.id, undefined, {numeric: true}));
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
    page: "today", snapshot: null, workspace: null, sync: null, feedback: [], setup: null, messages: [], search: "", busy: false, notice: "", config: null,
    activity: {view: "gallery", items: null, summary: null, week: null, filter: defaultGalleryFilter(), limit: GALLERY_PAGE, loading: false},
    calendar: {year: new Date().getFullYear(), day: null, filter: null, flash: null}
  };
  let drawn: {page: Page; mode: Mode} | null = null;
  root.innerHTML = `<div class="loading"><span class="loading-blob">${icons.sprout}</span><p>Opening your workspace…</p></div>`;
  const request = async <T>(command: string, payload: Record<string, unknown> = {}): Promise<T> => client.send<T>(command, payload);
  const modeOf = (): Mode => state.workspace?.mode === "p" ? "p" : "j";

  async function reload() {
    state.snapshot = await request<Snapshot>("status_snapshot");
    state.workspace = await request<Workspace>("workspace_info");
    if (!drawn) state.calendar.year = Number(state.snapshot.today.date.slice(0, 4)) || state.calendar.year;
    if (state.page === "activity") await loadActivity();
    draw();
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
    if (region) { region.textContent = state.busy ? "Working on it…" : state.notice; region.classList.toggle("visible", state.busy || !!state.notice); }
  }
  function navigate(page: Page) {
    state.page = page;
    state.calendar.day = null;
    if (page === "activity") state.activity.view = "gallery";
    draw();
    scrollTop();
    if (page === "activity") void refreshActivity();
  }

  function draw(animate = false) {
    const s = state.snapshot!;
    const mode = modeOf();
    document.documentElement.dataset.mode = mode;
    animate ||= !drawn || drawn.page !== state.page || drawn.mode !== mode;
    drawn = {page: state.page, mode};
    const title = PAGES.find(([id]) => id === state.page)![1];
    const page = state.page === "today" ? (mode === "p" ? overviewP(s) : overviewJ(s)) : state.page === "schedule" ? schedulePage(s) : state.page === "routines" ? routines(s) : state.page === "activity" ? activity(s) : settings(s);
    root.innerHTML = `<div class="workspace mode-${mode}"><aside class="sidebar"><a class="brand" href="#" aria-label="Schedule Everything overview"><span class="brand-mark">${icons.sprout}</span><span>schedule<span class="brand-sub">everything</span></span></a><div class="workspace-label">YOUR WORKSPACE</div>
      <nav aria-label="Main navigation">${PAGES.map(([id, label]) => `<button type="button" data-page="${id}" title="${e(label)}" aria-current="${state.page === id ? "page" : "false"}" class="nav-item ${state.page === id ? "active" : ""}"><span class="nav-icon">${icons[id]}</span><span class="nav-label">${label}</span>${id === "today" ? `<span class="nav-count">${s.tasks.length}</span>` : ""}</button>`).join("")}</nav>
      <div class="sidebar-bottom"><div class="mode-note mode-${mode}">${mode === "p" ? icons.feather : icons.target}<div><strong>${mode === "p" ? "P mode" : "J mode"}</strong><span>${mode === "p" ? "Flexible, task-first" : "Structured day"}</span></div></div><span class="local-dot"></span> Saved on your computer<p>One day, one thing at a time.</p></div></aside>
      <main class="main"><header class="topbar"><span class="breadcrumb">Workspace ${icons.arrow} <strong>${title}</strong></span>${modeSwitch(mode)}<span class="date">${icons.today}${e(formatDate(s.today.date, {weekday: "short", month: "short", day: "numeric"}))}</span>${themeControls()}${button("refresh", "Refresh", icons.refresh, "button small ghost")}</header>
      <div class="content page-${state.page} ${animate ? "enter" : ""}"><div id="notice" role="status" aria-live="polite" class="notice"></div>${s.config.error ? `<div class="config-banner"><h3>Let’s get your schedule ready.</h3><p>${e(s.config.error)}</p><button class="button primary" type="button" data-page="schedule">Build a schedule</button><button class="button ghost" type="button" data-page="settings">Edit configuration</button></div>` : ""}${page}</div></main></div>`;
    bindShell(s);
    status();
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
    bindTasks(root, s);
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
      const board = root.querySelector<HTMLElement>("#task-board")!;
      board.innerHTML = taskBoard(s);
      bindTasks(board, s);
    });
    root.querySelector<HTMLSelectElement>('[name="configFile"]')?.addEventListener("change", event => void action(async () => {
      if (state.config && root.querySelector<HTMLTextAreaElement>('[name="configContent"]')?.value !== state.config.content && !window.confirm("Discard unsaved edits?")) { (event.target as HTMLSelectElement).value = state.config.file; return; }
      state.config = await request<ConfigFile>("config_read", {file: (event.target as HTMLSelectElement).value});
      draw();
    }));
    root.querySelector<HTMLFormElement>("#setup-form")?.addEventListener("submit", event => { event.preventDefault(); const message = new FormData(event.target as HTMLFormElement).get("message") as string; void action(() => sendSetup(message)); });
    bindTypeRows();
    if (state.page === "activity") bindActivity(s);
    if (state.page === "routines") bindCalendar(s);
  }

  async function runAction(id: string) {
    if (id === "refresh") await reload();
    if (id === "add-task") taskDialog();
    if (id === "add-deadline") deadlineDialog();
    if (id === "sync-generate") {
      const feedback = root.querySelector<HTMLTextAreaElement>('[name="feedback"]')?.value.trim();
      if (feedback) state.feedback.push(feedback);
      state.sync = await request<SyncProposal>("sync_generate", {feedback: state.feedback}); draw();
    }
    if (id === "sync-dismiss") { state.sync = null; draw(); }
    if (id === "sync-accept") { await request("sync_accept", {plan: state.sync!.plan}); state.sync = null; state.feedback = []; await reload(); state.notice = "Today’s plan is ready."; }
    if (id === "setup-new") { state.setup = null; state.messages = []; draw(); }
    if (id === "setup-confirm") await sendSetup("I confirm the schedule summary.", true);
    if (id === "setup-accept") { const result = await request<{message: string}>("setup_accept", {sessionId: state.setup!.sessionId}); state.setup = null; await reload(); state.notice = result.message; }
    if (id === "open-config") { state.config = await request<ConfigFile>("config_read", {file: "settings"}); draw(); }
    if (id === "config-save") {
      const content = root.querySelector<HTMLTextAreaElement>('[name="configContent"]')!.value;
      const result = await request<ConfigFile & {message: string}>("config_save", {...state.config, content}); state.config = result; await reload(); state.notice = result.message;
    }
    if (id === "types-save") { const taskTypes = Object.fromEntries([...root.querySelectorAll<HTMLElement>("[data-type-row]")].map(row => [row.querySelector<HTMLInputElement>('[name="typeId"]')!.value, row.querySelector<HTMLInputElement>('[name="typeName"]')!.value])); await request("settings_set_task_types", {taskTypes}); await reload(); state.notice = "Task and deadline types saved."; }
    if (id === "types-add") { const ids = [...root.querySelectorAll<HTMLInputElement>('[name="typeId"]')].map(input => Number(input.value)).filter(Number.isFinite); const next = String(Math.max(0, ...ids) + 1); root.querySelector("#type-rows")!.insertAdjacentHTML("beforeend", typeRow(next, "")); bindTypeRows(); }
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
        try { await request("task_delete", {description: task.description, action: kind}); }
        catch (error) { card?.classList.remove("completing"); throw error; }
        await reload();
        state.notice = kind === "deleted" ? "Task completed. Nice work." : "Task removed.";
      });
    });
  }
  async function removeDeadline(deadline: DeadlineItem) {
    await action(async () => { if (!window.confirm(`Remove “${deadline.event}”?`)) return; await request("deadline_delete", {event: deadline.event}); await reload(); });
  }
  function bindTypeRows() { root.querySelectorAll<HTMLButtonElement>("[data-type-remove]").forEach(btn => btn.onclick = () => btn.closest("[data-type-row]")!.remove()); }
  function typeRow(id: string, name: string) { return `<div class="type-row" data-type-row><label>Type ID<input name="typeId" value="${e(id)}" readonly></label><label>Name<input name="typeName" value="${e(name)}" placeholder="Task type name" required></label><button class="icon-button danger" type="button" data-type-remove aria-label="Remove type ${e(id)}">${icons.close}</button></div>`; }
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
  function overviewP(s: Snapshot) {
    return `<div class="page-heading p-heading"><div><p class="eyebrow">${icons.feather} P MODE · GO WITH THE FLOW</p><h1>Tasks by type<span class="accent-dot">.</span></h1><p class="muted">${s.tasks.length} open tasks. No clock to chase. Pick whatever pulls you in.</p></div><div class="heading-actions"><label class="search-pill">${icons.search}<input class="search" name="search" aria-label="Search tasks" placeholder="Find a task…" value="${e(state.search)}"></label>${button("add-task", "New task", icons.plus, "button primary")}</div></div>
      <section id="task-board" class="task-board flow-board">${taskBoard(s)}</section>`;
  }
  function taskSection(s: Snapshot) {
    return `<div class="section-heading board-heading"><div><h2>Tasks by type <span class="count">${s.tasks.length}</span></h2><p class="muted">Highest importance first within each task status.</p></div><label class="search-pill">${icons.search}<input class="search" name="search" aria-label="Search tasks" placeholder="Find a task…" value="${e(state.search)}"></label></div><section id="task-board" class="task-board">${taskBoard(s)}</section>`;
  }
  function taskBoard(s: Snapshot) {
    const query = state.search.toLowerCase();
    const groups = groupTasks(s).map(group => ({...group, tasks: group.tasks.filter(task => task.description.toLowerCase().includes(query))})).filter(group => group.tasks.length);
    if (!groups.length) return empty(state.search ? "No tasks match your search." : "A clear slate. Add a task to find your next focus.");
    return groups.map(group => `<article class="task-group ${sw(typeSwatch(s.taskTypes, group.id))}" style="--tilt:${(hash(group.id) % 5 - 2) * 0.6}deg; --shape:${hash(group.name) % 4}"><header class="group-heading"><span class="group-icon">${icons.tasks}</span><h3>${e(group.name)}</h3><span class="group-count">${group.tasks.length}</span></header><div class="task-cards">${group.tasks.map((task, position) => {
      const idx = s.tasks.indexOf(task);
      const days = task.alarmFrom ? Math.round((Date.parse(task.alarmFrom) - Date.parse(s.today.date)) / 86400000) : 0;
      const detail = days > 0 ? `Reminders ${days === 1 ? "tomorrow" : `in ${days} days`} · ${task.alarmFrom}` : task.procrastinated ? `Deferred${task.procrastinateDays === null ? "" : ` · ${task.procrastinateDays} days`}` : "";
      const level = task.priority >= 8 ? "high" : task.priority >= 5 ? "medium" : "low";
      return `<div class="task-card ${task.procrastinated ? "deferred" : ""} ${days > 0 ? "postponed" : task.priority >= 8 ? "high-priority" : ""}" style="--i:${position}"><div class="task-main"><button type="button" class="complete-button" data-task="${idx}" data-operation="deleted" aria-label="Complete ${e(task.description)}" title="Complete task">${icons.check}</button><h4>${e(task.description)}</h4></div><div class="task-meta"><span class="priority p-${level}">${icons.flag} Importance ${task.priority}/10</span><span class="energy" style="--p:${task.priority / 10}" aria-hidden="true"></span><button class="text-button edit" type="button" data-task="${idx}" data-operation="edit" aria-label="Edit ${e(task.description)}">Edit</button></div>${detail ? `<p class="task-detail">${icons.clock}${e(detail)}</p>` : ""}<details class="task-more"><summary>More options</summary><button type="button" data-task="${idx}" data-operation="cancelled">Cancel · added by mistake</button><button type="button" data-task="${idx}" data-operation="dropped">Drop · no longer pursuing</button></details></div>`;
    }).join("")}</div></article>`).join("");
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
      return `<li class="deadline-${e(d.status)} ${sw(deadlineSwatch(d, s.taskTypes))}"><span class="ddl-dot" aria-hidden="true"></span><div class="deadline-copy"><strong>${e(d.event)}</strong><div class="deadline-meta"><time>${e(d.deadline)}</time><span class="type-badge ${d.type ? sw(typeSwatch(s.taskTypes, d.type)) : "untyped"}">${e(d.typeName ?? d.type ?? "Uncategorized")}</span></div></div><span class="deadline-status ${e(d.status)}">${label}</span><button class="text-button" data-deadline="${index}" data-operation="edit" aria-label="Edit ${e(d.event)}" type="button">Edit</button><button class="icon-button danger" data-deadline="${index}" data-operation="remove" aria-label="Remove ${e(d.event)}" type="button">${icons.close}</button></li>`;
    }).join("")}</ul>` : empty("No deadlines to track.");
  }

  // ----- Schedule, routines, settings -------------------------------------------

  function schedulePage(s: Snapshot) {
    return `<div class="page-heading"><div><p class="eyebrow">A RHYTHM THAT WORKS FOR YOU</p><h1>Make a little space<span class="accent-dot">.</span></h1><p class="muted">${e(s.today.parity)} week · ${e(s.today.weekday)} · schedule ${s.config.activeId}</p></div>${button("schedule-pdf", "Download schedule", icons.schedule)}</div><section class="split-grid"><article class="card timeline-card"><div class="section-heading compact"><h2>Today’s timeline</h2><span class="pill light">${s.schedule.events.length} blocks</span></div>${timeline(s)}</article><div><article class="card sync-card"><p class="eyebrow">${icons.spark} PLAN YOUR FOCUS</p><h2>Give your blocks a purpose.</h2><p class="muted">Assign tasks to today’s focus blocks. Review the proposal before it is saved.</p>${state.sync ? `<p>${e(state.sync.summary ?? "")}</p><ol class="timeline">${state.sync.preview.map(item => `<li><time>${e(item.time)}</time><span>${e(item.label)}</span></li>`).join("")}</ol><label>What would you change?<textarea name="feedback" placeholder="More time for writing, fewer context switches…"></textarea></label><div class="button-row">${button("sync-accept", "Accept plan", icons.check, "button primary")}${button("sync-generate", "Revise", icons.refresh)}${button("sync-dismiss", "Discard", "", "button ghost")}</div>` : button("sync-generate", "Plan my day", icons.spark, "button primary")}</article><article class="card assistant"><div class="section-heading compact"><h2>Schedule assistant</h2>${button("setup-new", "New chat", "", "text-button")}</div><p class="muted">Build or adjust your weekly routine with a conversation.</p><div class="conversation">${state.messages.map(m => `<div class="message ${m.role}"><small>${m.role === "user" ? "You" : "Schedule assistant"}</small><p>${e(m.text)}</p></div>`).join("") || '<p class="muted">Tell me about your working hours, goals, habits, and fixed commitments.</p>'}</div>${state.setup?.schedule_summary && !state.setup.bundle ? `<div class="proposal"><h3>Schedule summary</h3><p>${e(state.setup.schedule_summary)}</p>${button("setup-confirm", "Confirm summary", icons.check, "button primary")}</div>` : ""}${state.setup?.bundle ? `<div class="proposal"><h3>Review your new routine</h3>${Object.entries(state.setup.bundle).map(([file, text]) => `<details><summary>${e(file)}</summary><pre>${e(text)}</pre></details>`).join("")}${state.setup.profile_markdown ? `<details><summary>Profile draft</summary><pre>${e(state.setup.profile_markdown)}</pre></details>` : ""}${button("setup-accept", "Apply schedule", icons.check, "button primary")}</div>` : ""}<form id="setup-form"><label class="sr-only" for="setup-message">Message to the schedule assistant</label><textarea id="setup-message" name="message" required placeholder="I work 9 to 5 and want to make room for…"></textarea><label class="attachment-field">Attach a timetable (text or image, up to 5 MB)<input type="file" name="timetable" accept="image/*,.txt,.md,.csv,.tsv,.json,.toml,.yaml,.yml"></label><button class="button primary" type="submit">${icons.spark} Send message</button></form><small class="muted">AI uses your locally configured pi account and model.</small></article></div></section>`;
  }
  function routines(s: Snapshot) {
    const done = s.habits.filter(h => h.completed).length;
    return `<div class="page-heading"><div><p class="eyebrow">SMALL THINGS, STEADY PROGRESS</p><h1>Routines & deadlines<span class="accent-dot">.</span></h1><p class="muted">Plant deadlines on your year, color them, and keep your daily habits close.</p></div>${button("add-deadline", "New deadline", icons.plus, "button primary")}</div>
      ${yearCalendar({deadlines: s.deadlines, taskTypes: s.taskTypes, today: s.today.date, state: state.calendar})}
      <section class="split-grid"><article class="card habits-card"><div class="section-heading"><h2>Today’s habits</h2><span class="pill light">${done}/${s.habits.length} complete</span></div>${s.habits.length ? `<div class="habit-hero">${ring({value: done / s.habits.length, size: 96, label: `${done} of ${s.habits.length} habits complete`, center: `${done}/${s.habits.length}`})}<p class="muted">${done === s.habits.length ? "Every habit done today. Lovely." : `${s.habits.length - done} to go. Small and steady.`}</p></div>${s.habits.map((h, i) => `<label class="habit-row ${h.completed ? "done" : ""}" style="--i:${i}"><input type="checkbox" data-habit="${e(h.id)}" ${h.completed ? "checked" : ""}><span class="habit-check" aria-hidden="true">${icons.check}</span><span class="habit-text">${e(h.description)}</span></label>`).join("")}` : empty("Add habits in Settings to build your daily rhythm.")}<p class="muted">Edit your habit definitions in Settings → Habits.</p></article><article class="card"><h2>All deadlines</h2>${deadlineList(s)}</article></section>`;
  }
  function activity(s: Snapshot) {
    const a = state.activity;
    if (a.view === "week" && a.week) return `<div class="page-heading"><div><p class="eyebrow">${icons.spark} WEEKLY OVERVIEW</p><h1>Your week in color<span class="accent-dot">.</span></h1><p class="muted">Compared with last week and the weeks before it.</p></div></div>${weeklyView(a.week)}`;
    return `<div class="page-heading"><div><p class="eyebrow">LOOK HOW FAR YOU’VE COME</p><h1>Little wins add up<span class="accent-dot">.</span></h1><p class="muted">A gallery of everything you finished, cancelled, or let go. Open any piece to see its story.</p></div></div>${galleryView({items: a.items, overview: a.summary, filter: a.filter, limit: a.limit, today: s.today.date, taskTypes: s.taskTypes})}`;
  }
  function settings(s: Snapshot) {
    const files = {settings: "General settings", odd: "Odd week schedule", even: "Even week schedule", habits: "Habits", deadlines: "Deadlines", profile: "Profile", model: "AI model"};
    return `<div class="page-heading"><div><p class="eyebrow">MAKE IT YOURS</p><h1>Your workspace, your rules<span class="accent-dot">.</span></h1><p class="muted">All changes are saved to your computer.</p></div></div><section class="split-grid"><article class="card"><h2>Reminder preferences</h2><p class="muted">${modeOf() === "p" ? "P mode · selected event alarms paused · overview shows tasks only" : "J mode · all reminders allowed · full structured overview"}</p>${button("mode", modeOf() === "p" ? "Switch to J mode" : "Switch to P mode", icons.settings)}<hr><label>Active schedule<select name="configId">${state.workspace?.configIds?.map(id => `<option value="${id}" ${id === s.config.activeId ? "selected" : ""}>Schedule ${id}</option>`).join("") ?? ""}</select></label><div class="button-row">${button("switch", "Activate")}${button("update", "Reload reminders", icons.refresh)}${button("stop", "Stop reminders", "", "button ghost danger")}</div><p class="muted path">${e(s.config.rootDir)}</p><hr><h3>Task & deadline types</h3><p class="muted">Shared categories for tasks and deadlines. Keep IDs stable so existing items retain their type and color.</p><div id="type-rows">${Object.entries(s.taskTypes).map(([id, name]) => typeRow(id, name)).join("")}</div><div class="button-row">${button("types-save", "Save types", icons.check)}${button("types-add", "Add type", icons.plus, "button ghost")}</div><hr>${button("shutdown", "Close browser service", "", "button ghost")}</article><article class="card"><h2>Configuration editor</h2><p class="muted">Edit your schedules, habits, settings, or optional AI model here. Syntax is checked before saving.</p>${state.config ? `<label>File<select name="configFile">${Object.entries(files).map(([id, label]) => `<option value="${id}" ${id === state.config!.file ? "selected" : ""}>${label}</option>`).join("")}</select></label><label>Contents<textarea class="code-editor" name="configContent" rows="22" spellcheck="false">${e(state.config.content)}</textarea></label>${button("config-save", "Save changes", icons.check, "button primary")}` : button("open-config", "Open editor", icons.settings)}<p class="muted">Saved edits reload the installed reminder service when available.</p></article></section>`;
  }

  // ----- Activity bindings ---------------------------------------------------

  function redrawGallery() {
    draw();
    root.querySelector("#gallery")?.classList.add("reflow");
  }
  function bindActivity(s: Snapshot) {
    const a = state.activity;
    root.querySelectorAll<HTMLButtonElement>("[data-filter-status]").forEach(btn => btn.onclick = () => { a.filter.status = btn.dataset.filterStatus as HistoryStatus; a.limit = GALLERY_PAGE; redrawGallery(); });
    root.querySelectorAll<HTMLButtonElement>("[data-filter-type]").forEach(btn => btn.onclick = () => { a.filter.type = a.filter.type === btn.dataset.filterType ? null : btn.dataset.filterType!; a.limit = GALLERY_PAGE; redrawGallery(); });
    root.querySelectorAll<HTMLButtonElement>("[data-heat-day]").forEach(btn => btn.onclick = () => { a.filter.day = a.filter.day === btn.dataset.heatDay ? null : btn.dataset.heatDay!; a.limit = GALLERY_PAGE; redrawGallery(); });
    root.querySelector<HTMLSelectElement>('[name="gallerySort"]')?.addEventListener("change", event => { a.filter.sort = (event.target as HTMLSelectElement).value as HistorySort; redrawGallery(); });
    root.querySelector<HTMLInputElement>('[name="gallerySearch"]')?.addEventListener("input", event => {
      a.filter.query = (event.target as HTMLInputElement).value;
      a.limit = GALLERY_PAGE;
      const region = root.querySelector<HTMLElement>("#gallery")!;
      region.innerHTML = galleryGroups(a.items ?? [], a.filter, a.limit, s.today.date, s.taskTypes);
      bindGallery(s);
    });
    root.querySelectorAll<HTMLButtonElement>("[data-week]").forEach(btn => btn.onclick = () => void action(async () => {
      if (btn.dataset.week === "today" || !a.week) a.week = a.summary;
      else {
        const result = await request<WeeklyOverview>("weekly_overview", {date: addDays(a.week.weekStart, btn.dataset.week === "prev" ? -7 : 7)});
        if (isOverview(result)) a.week = result;
      }
      draw(true);
    }));
    bindGallery(s);
    const rhythm = root.querySelector<HTMLElement>(".rhythm");
    if (rhythm) rhythm.scrollLeft = rhythm.scrollWidth;
  }
  function bindGallery(s: Snapshot) {
    const a = state.activity;
    root.querySelectorAll<HTMLElement>("[data-history]").forEach(card => {
      card.onclick = () => openLightbox(Number(card.dataset.history), s);
      card.onkeydown = event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); openLightbox(Number(card.dataset.history), s); } };
    });
    const more = root.querySelector<HTMLButtonElement>("[data-gallery-more]");
    if (more) more.onclick = () => {
      a.limit += GALLERY_PAGE;
      const region = root.querySelector<HTMLElement>("#gallery")!;
      region.innerHTML = galleryGroups(a.items ?? [], a.filter, a.limit, s.today.date, s.taskTypes);
      bindGallery(s);
    };
    bindTilt(root, ".gallery-card");
  }
  function openLightbox(index: number, s: Snapshot) {
    const items = state.activity.items ?? [];
    const visible = filterHistory(items, state.activity.filter);
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
    card.outerHTML = yearCalendar({deadlines: s.deadlines, taskTypes: s.taskTypes, today: s.today.date, state: state.calendar});
    bindCalendar(s);
    if (focusInput) root.querySelector<HTMLInputElement>('[name="popEvent"]')?.focus();
  }
  function openDay(day: string) {
    state.calendar.day = day;
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
  function bindCalendar(s: Snapshot) {
    const today = s.today.date;
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
        await request("deadline_add", {event: name, date: day, type: data.get("popType") ?? "", color: data.get("popColor") ?? ""});
        state.calendar.day = null;
        state.calendar.flash = day;
        await reload();
        state.notice = `Added “${name}” on ${formatDate(day, {weekday: "short", month: "short", day: "numeric"})}.`;
      });
    });
  }

  // ----- Dialogs ---------------------------------------------------------------

  async function sendSetup(message: string, confirmSummary = false) {
    const file = root.querySelector<HTMLInputElement>('[name="timetable"]')?.files?.[0];
    let attachment: {name: string; data: string} | undefined;
    if (file) {
      if (file.size > 5 * 1024 * 1024) throw new Error("Choose a file smaller than 5 MB.");
      const encoded = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result).split(",")[1]); reader.onerror = () => reject(new Error("Could not read the attachment.")); reader.readAsDataURL(file); });
      attachment = {name: file.name, data: encoded};
    }
    state.messages.push({role: "user", text: message + (attachment ? `\nAttached: ${attachment.name}` : "")}); draw();
    state.setup = await request<SetupTurn>("setup_turn", {message, sessionId: state.setup?.sessionId, confirmSummary, attachment});
    state.messages.push({role: "assistant", text: [state.setup.conversation, state.setup.question_to_user].filter(Boolean).join("\n\n")}); draw();
  }
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
    dialog(deadline ? "Edit deadline" : "New deadline", `${field("event", "Event", "text", deadline?.event ?? "", "required")}${field("date", "Due date", "date", deadline?.deadline ?? state.snapshot!.today.date, "required")}<label>Deadline type<select name="type"><option value="">Uncategorized</option>${typeOptions(state.snapshot!, selected)}${unknown}</select></label>${swatchPicker("color", isSwatch(color) ? color : "", "Match type color")}`, async data => {
      await request(deadline ? "deadline_update" : "deadline_add", {event: data.get("event"), date: data.get("date"), type: data.get("type"), color: data.get("color") ?? "", originalEvent: deadline?.event});
      state.calendar.day = null;
      await reload();
    });
  }

  root.addEventListener("workspace-refresh", () => { if (state.busy || document.hidden || root.querySelector("dialog[open]") || state.calendar.day || ["settings", "schedule", "activity"].includes(state.page) || ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName ?? "")) return; void reload().catch(() => { state.notice = "Could not refresh. Check the local server connection."; status(); }); }, {signal});
  root.addEventListener("workspace-activate", () => { state.notice = "Your existing workspace is ready."; status(); }, {signal});
  root.addEventListener("click", event => {
    if (!state.calendar.day || !(event.target instanceof Element) || !event.target.isConnected) return;
    if (event.target.closest(".day-popover, .day[data-day], .ddl-chip, dialog, [data-cal]")) return;
    state.calendar.day = null;
    drawCalendar();
  }, {signal});
  root.addEventListener("keydown", event => {
    if (event.key !== "Escape" || !state.calendar.day || root.querySelector("dialog[open]")) return;
    const day = state.calendar.day;
    state.calendar.day = null;
    drawCalendar();
    root.querySelector<HTMLElement>(`.day[data-day="${day}"]`)?.focus();
  }, {signal});
  window.addEventListener("resize", () => positionPopover(), {signal});
  try { await reload(); } catch (error) { root.innerHTML = `<div class="loading"><h1>Could not open your workspace</h1><p>${e(error instanceof Error ? error.message : String(error))}</p><button class="button" id="retry">Try again</button></div>`; root.querySelector("#retry")?.addEventListener("click", () => void renderApp(root, client)); }
}
