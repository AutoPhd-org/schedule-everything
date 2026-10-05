import { CalendarDays, Check, ChevronRight, CircleCheck, Clock3, Code2, Flag, LayoutDashboard, ListTodo, Plus, RefreshCw, Settings2, Sparkles, Sprout, X } from "lucide";
import type { BridgeClient, Snapshot, SyncProposal, TaskItem, DeadlineItem } from "./types";

type SvgNode = readonly [tag: string, attrs: Record<string, string | number>, children?: readonly SvgNode[]];
function icon(nodes: SvgNode): string {
  const node = ([tag, attrs, children]: SvgNode): string => `<${tag} ${Object.entries(attrs).map(([key, value]) => `${key}="${escapeHtml(String(value))}"`).join(" ")}>${children?.map(node).join("") ?? ""}</${tag}>`;
  return `<svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${(nodes[2] ?? []).map(node).join("")}</svg>`;
}
const icons = {sprout: icon(Sprout), today: icon(LayoutDashboard), schedule: icon(CalendarDays), routines: icon(Sprout), activity: icon(CircleCheck), settings: icon(Settings2), plus: icon(Plus), refresh: icon(RefreshCw), close: icon(X), check: icon(Check), spark: icon(Sparkles), tasks: icon(ListTodo), clock: icon(Clock3), code: icon(Code2), flag: icon(Flag), arrow: icon(ChevronRight)};
type Page = "today" | "schedule" | "routines" | "activity" | "settings";
type ConfigFile = {file: string; content: string; revision: string; activeId: number};
type Workspace = {configIds: number[]; activeId: number; rootDir: string; mode: string};
type SetupTurn = {sessionId: string; phase: string; conversation: string; question_to_user: string | null; schedule_summary: string | null; profile_markdown: string | null; bundle: Record<string, string> | null};
type State = {page: Page; snapshot: Snapshot | null; workspace: Workspace | null; sync: SyncProposal | null; feedback: string[]; setup: SetupTurn | null; messages: {role: string; text: string}[]; search: string; busy: boolean; notice: string; config: ConfigFile | null; historyCount: number};

export function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, char => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;"}[char]!));
}
const e = escapeHtml;
const empty = (text: string) => `<div class="empty">${icons.sprout}<p>${text}</p></div>`;
const button = (id: string, text: string, image = "", classes = "button") => `<button type="button" class="${classes}" data-action="${id}">${image}${text}</button>`;
const field = (name: string, label: string, type = "text", value = "", attrs = "") => `<label>${label}<input name="${name}" type="${type}" value="${e(value)}" ${attrs}></label>`;
const typeOptions = (snapshot: Snapshot, selected = "") => Object.entries(snapshot.taskTypes).map(([id, name]) => `<option value="${e(id)}" ${id === selected ? "selected" : ""}>${e(name)}</option>`).join("");

export function groupTasks(snapshot: Snapshot): {id: string; name: string; tasks: TaskItem[]}[] {
  const groups = new Map<string, {id: string; name: string; tasks: TaskItem[]}>();
  for (const task of snapshot.tasks) {
    if (!groups.has(task.type)) groups.set(task.type, {id: task.type, name: snapshot.taskTypes[task.type] ?? task.typeName ?? "Other", tasks: []});
    groups.get(task.type)!.tasks.push(task);
  }
  return [...groups.values()].sort((a, b) => a.id.localeCompare(b.id, undefined, {numeric: true}));
}

export async function renderApp(root: HTMLElement, client: BridgeClient): Promise<void> {
  const state: State = {page: "today", snapshot: null, workspace: null, sync: null, feedback: [], setup: null, messages: [], search: "", busy: false, notice: "", config: null, historyCount: 5};
  root.innerHTML = `<div class="loading">${icons.sprout}<p>Opening your workspace…</p></div>`;
  const request = async <T>(command: string, payload: Record<string, unknown> = {}): Promise<T> => client.send<T>(command, payload);
  async function reload() {
    state.snapshot = await request<Snapshot>("status_snapshot");
    state.workspace = await request<Workspace>("workspace_info");
    if (state.historyCount !== 5) state.snapshot.history = (await request<{activities: Snapshot["history"]}>("task_history", state.historyCount === 0 ? {all:true} : {count:state.historyCount})).activities;
    draw();
  }
  async function action(work: () => Promise<void>) {
    if (state.busy) return;
    state.busy = true;
    state.notice = "";
    status();
    root.querySelectorAll<HTMLButtonElement>("button").forEach(btn => btn.disabled = true);
    root.querySelectorAll<HTMLInputElement>("[data-habit]").forEach(input => input.disabled = true);
    try { await work(); }
    catch (error) { state.notice = error instanceof Error ? error.message : String(error); }
    finally { state.busy = false; root.querySelectorAll<HTMLButtonElement>("button").forEach(btn => btn.disabled = false); root.querySelectorAll<HTMLInputElement>("[data-habit]").forEach(input => input.disabled = false); status(); }
  }
  function status() {
    const region = root.querySelector<HTMLElement>("#notice");
    if (region) { region.textContent = state.busy ? "Working on it…" : state.notice; region.classList.toggle("visible", state.busy || !!state.notice); }
  }
  function draw() {
    const s = state.snapshot!;
    root.innerHTML = `<div class="workspace"><aside class="sidebar"><a class="brand" href="#">${icons.sprout}<span>schedule<span class="brand-sub">everything</span></span></a><div class="workspace-label">YOUR WORKSPACE</div><nav aria-label="Main navigation">${([['today','Overview'],['schedule','Schedule'],['routines','Routines & deadlines'],['activity','Activity'],['settings','Settings']] as [Page,string][]).map(([id,label])=>`<button type="button" data-page="${id}" class="nav-item ${state.page === id ? 'active' : ''}">${icons[id]}${label}${id === 'today' ? `<span class="nav-count">${s.tasks.length}</span>` : ''}</button>`).join('')}</nav><div class="sidebar-bottom"><span class="local-dot"></span> Saved on your computer<p>One day, one thing at a time.</p></div></aside><main class="main"><header class="topbar"><span class="breadcrumb">Workspace ${icons.arrow} <strong>${{today:'Overview',schedule:'Schedule',routines:'Routines & deadlines',activity:'Activity',settings:'Settings'}[state.page]}</strong></span><span class="date">${icons.today}${e(new Date(s.today.date + 'T12:00:00').toLocaleDateString(undefined,{weekday:'short',month:'short',day:'numeric'}))}</span>${button('refresh','Refresh',icons.refresh,'button small ghost')}</header><div class="content"><div id="notice" role="status" aria-live="polite" class="notice"></div>${s.config.error ? `<div class="config-banner"><h3>Let’s get your schedule ready.</h3><p>${e(s.config.error)}</p><button class="button primary" type="button" data-page="schedule">Build a schedule</button><button class="button ghost" type="button" data-page="settings">Edit configuration</button></div>` : ''}${state.page === 'today' ? overview(s) : state.page === 'schedule' ? schedulePage(s) : state.page === 'routines' ? routines(s) : state.page === 'activity' ? activity(s) : settings(s)}</div></main></div>`;
    root.querySelectorAll<HTMLButtonElement>('[data-page]').forEach(btn => btn.onclick = () => {state.page = btn.dataset.page as Page; draw();});
    root.querySelector<HTMLAnchorElement>('.brand')!.onclick = event => {event.preventDefault(); state.page = 'today'; draw();};
    root.querySelectorAll<HTMLButtonElement>('[data-action]').forEach(btn => btn.onclick = () => void action(async () => {
      const id = btn.dataset.action!;
      if (id === 'refresh') await reload();
      if (id === 'add-task') taskDialog();
      if (id === 'add-deadline') deadlineDialog();
      if (id === 'sync-generate') {
        const feedback = root.querySelector<HTMLTextAreaElement>('[name="feedback"]')?.value.trim();
        if (feedback) state.feedback.push(feedback);
        state.sync = await request<SyncProposal>('sync_generate',{feedback: state.feedback}); draw();
      }
      if (id === 'sync-dismiss') {state.sync = null; draw();}
      if (id === 'sync-accept') {await request('sync_accept',{plan:state.sync!.plan}); state.sync = null; state.feedback = []; await reload(); state.notice = 'Today’s plan is ready.';}
      if (id === 'setup-new') {state.setup = null; state.messages = []; draw();}
      if (id === 'setup-confirm') await sendSetup('I confirm the schedule summary.', true);
      if (id === 'setup-accept') {const result = await request<{message:string}>('setup_accept',{sessionId:state.setup!.sessionId}); state.setup = null; await reload(); state.notice = result.message;}
      if (id === 'open-config') {state.config=await request<ConfigFile>('config_read',{file:'settings'});draw();}
      if (id === 'config-save') {
        const content = root.querySelector<HTMLTextAreaElement>('[name="configContent"]')!.value;
        const result = await request<ConfigFile & {message:string}>('config_save',{...state.config,content}); state.config = result; await reload(); state.notice = result.message;
      }
      if (id === 'types-save') {const taskTypes = Object.fromEntries([...root.querySelectorAll<HTMLElement>('[data-type-row]')].map(row=>[row.querySelector<HTMLInputElement>('[name="typeId"]')!.value,row.querySelector<HTMLInputElement>('[name="typeName"]')!.value])); await request('settings_set_task_types',{taskTypes}); await reload(); state.notice = 'Task types saved.';}
      if (id === 'types-add') {const ids=[...root.querySelectorAll<HTMLInputElement>('[name="typeId"]')].map(input=>Number(input.value)).filter(Number.isFinite);const next=String(Math.max(0,...ids)+1);root.querySelector('#type-rows')!.insertAdjacentHTML('beforeend',typeRow(next,''));bindTypeRows();}
      if (id === 'shutdown') {if (!window.confirm('Stop the local browser service? You can relaunch it whenever you need.')) return;await request('server_shutdown');root.dispatchEvent(new Event('workspace-closed'));root.innerHTML='<div class="loading"><h1>See you next time.</h1><p>Your local data is saved. Relaunch the browser workspace to return.</p></div>';}

      if (id === 'mode') {await request('service_action',{action:'mode',mode:state.workspace?.mode === 'p' ? 'j' : 'p'}); await reload();}
      if (['update','stop','switch'].includes(id)) {
        if (id === 'stop' && !window.confirm('Stop reminder notifications?')) return;
        const result = await request<{message:string}>('service_action',{action:id,configId:Number(root.querySelector<HTMLSelectElement>('[name="configId"]')?.value)}); state.config = null; await reload(); state.notice = result.message;
      }
      if (['schedule-pdf','weekly','monthly'].includes(id)) {
        const result = await request<{url:string;filename:string}>('export_pdf',{kind:id === 'schedule-pdf' ? 'schedule' : id,date:root.querySelector<HTMLInputElement>('[name="reportDate"]')?.value || undefined});
        const blob = await (await fetch(result.url)).blob(); const url = URL.createObjectURL(blob); const link = document.createElement('a'); link.href = url; link.download = result.filename; link.click(); setTimeout(()=>URL.revokeObjectURL(url),60000); state.notice = 'PDF downloaded.';
      }
    }));
    root.querySelectorAll<HTMLButtonElement>('[data-task]').forEach(btn => btn.onclick = () => {
      const task = s.tasks[Number(btn.dataset.task)];
      if (btn.dataset.operation === 'edit') taskDialog(task);
      else void action(async()=>{const kind = btn.dataset.operation!; if (kind !== 'deleted' && !window.confirm(`${kind === 'cancelled' ? 'Cancel' : 'Drop'} “${task.description}”?`)) return; await request('task_delete',{description:task.description,action:kind}); await reload(); state.notice = kind === 'deleted' ? 'Task completed. Nice work.' : 'Task removed.';});
    });
    root.querySelectorAll<HTMLButtonElement>('[data-deadline]').forEach(btn => btn.onclick = () => {
      const deadline = s.deadlines[Number(btn.dataset.deadline)];
      if (btn.dataset.operation === 'edit') deadlineDialog(deadline);
      else void action(async()=>{if (!window.confirm(`Remove “${deadline.event}”?`)) return; await request('deadline_delete',{event:deadline.event}); await reload();});
    });
    root.querySelectorAll<HTMLInputElement>('[data-habit]').forEach(check => check.onchange = () => void action(async()=>{await request('habit_mark',{habitIds:[...root.querySelectorAll<HTMLInputElement>('[data-habit]:checked')].map(el=>el.dataset.habit)}); await reload();}));
    root.querySelector<HTMLInputElement>('[name="search"]')?.addEventListener('input',event=>{
      state.search = (event.target as HTMLInputElement).value;
      const board = root.querySelector<HTMLElement>('#task-board')!; board.innerHTML = taskBoard(s); bindBoard();
    });
    function bindBoard() {root.querySelectorAll<HTMLButtonElement>('#task-board [data-task]').forEach(btn => btn.onclick=()=>{const task=s.tasks[Number(btn.dataset.task)]; if(btn.dataset.operation==='edit') taskDialog(task); else void action(async()=>{const kind=btn.dataset.operation!; if(kind!=='deleted'&&!window.confirm(`Remove “${task.description}” as ${kind}?`))return; await request('task_delete',{description:task.description,action:kind}); await reload();});});}
    root.querySelector<HTMLSelectElement>('[name="configFile"]')?.addEventListener('change',event=>void action(async()=>{if (state.config && root.querySelector<HTMLTextAreaElement>('[name="configContent"]')?.value !== state.config.content && !window.confirm('Discard unsaved edits?')){(event.target as HTMLSelectElement).value=state.config.file;return;} state.config=await request<ConfigFile>('config_read',{file:(event.target as HTMLSelectElement).value}); draw();}));
    root.querySelector<HTMLFormElement>('#setup-form')?.addEventListener('submit',event=>{event.preventDefault(); const message = new FormData(event.target as HTMLFormElement).get('message') as string; void action(()=>sendSetup(message));});
    root.querySelector<HTMLSelectElement>('[name="historyCount"]')?.addEventListener('change',event=>void action(async()=>{state.historyCount=Number((event.target as HTMLSelectElement).value);s.history=(await request<{activities:Snapshot["history"]}>('task_history',state.historyCount===0?{all:true}:{count:state.historyCount})).activities;draw();}));
    function bindTypeRows(){root.querySelectorAll<HTMLButtonElement>('[data-type-remove]').forEach(btn=>btn.onclick=()=>btn.closest('[data-type-row]')!.remove());}
    bindTypeRows();
    status();
  }
  function typeRow(id:string,name:string){return `<div class="type-row" data-type-row><label>Type ID<input name="typeId" value="${e(id)}" readonly></label><label>Name<input name="typeName" value="${e(name)}" placeholder="Task type name" required></label><button class="text-button danger" type="button" data-type-remove aria-label="Remove type ${e(id)}">${icons.close}</button></div>`;}
  function overview(s: Snapshot) {
    const complete = s.habits.filter(h=>h.completed).length;
    const urgent = s.tasks.filter(t=>t.priority>=8).length;
    return `<div class="page-heading"><div><p class="eyebrow">MAKE ROOM FOR WHAT MATTERS</p><h1>Your day, in focus<span class="mint">.</span></h1><p class="muted">A little clarity for everything you want to get done.</p></div>${button('add-task','New task',icons.plus,'button primary')}</div><section class="overview-grid"><article class="focus-card"><div class="focus-top"><span class="focus-label">${icons.clock} RIGHT NOW</span><span class="pill">${s.schedule.hasSyncedOverlay ? 'Your plan' : 'Weekly routine'}</span></div><h2>${e(s.schedule.current ?? (s.schedule.isSkipped ? 'A day to recharge' : 'Space to breathe'))}</h2><p>Up next <strong>${e(s.schedule.next ?? 'No upcoming blocks')}</strong>${s.schedule.timeToNext ? ` · in ${e(s.schedule.timeToNext)}` : ''}</p><div class="focus-foot">${icons.spark}<span>Your next small step makes a difference.</span><div class="focus-art"><span></span><span></span><span></span></div></div></article><article class="card day-card"><p class="eyebrow">ON YOUR RADAR</p><div class="metric-row"><span class="metric">${s.tasks.length}</span><span>open tasks<br><small>${urgent} high importance</small></span>${icons.tasks}</div><div class="mini-metrics"><div><strong>${s.deadlines.length}</strong><span>Deadlines</span></div><div><strong>${complete}/${s.habits.length}</strong><span>Habits today</span></div></div></article></section><div class="section-heading"><div><h2>Your task landscape <span class="count">${s.tasks.length}</span></h2><p class="muted">Grouped by type. Your priorities stay in view.</p></div><input class="search" name="search" aria-label="Search tasks" placeholder="Find a task…" value="${e(state.search)}"></div><section id="task-board" class="task-board">${taskBoard(s)}</section><section class="bottom-grid"><article class="card"><div class="section-heading compact"><h3>${icons.schedule} Coming up today</h3><button class="text-button" type="button" data-page="schedule">Full schedule ${icons.arrow}</button></div>${timeline(s,5)}</article><article class="card"><div class="section-heading compact"><h3>${icons.flag} Deadlines</h3>${button('add-deadline','Add',icons.plus,'text-button')}</div>${deadlineList(s)}</article></section>`;
  }
  function taskBoard(s: Snapshot) {
    const groups = groupTasks(s).map(group=>({...group,tasks:group.tasks.filter(task=>task.description.toLowerCase().includes(state.search.toLowerCase()))})).filter(group=>group.tasks.length);
    if (!groups.length) return empty(state.search ? 'No tasks match your search.' : 'A clear slate. Add a task to find your next focus.');
    return groups.map((group,index)=>`<article class="task-group tone-${index%5}"><header class="group-heading"><span class="group-icon">${index%2 ? icons.tasks : icons.code}</span><h3>${e(group.name)}</h3><span class="group-count">${group.tasks.length}</span></header><div class="task-cards">${group.tasks.map(task=>{
      const idx=s.tasks.indexOf(task); const days=task.alarmFrom ? Math.round((Date.parse(task.alarmFrom)-Date.parse(s.today.date))/86400000) : 0;
      const detail=days>0 ? `Reminders ${days===1 ? 'tomorrow' : `in ${days} days`} · ${task.alarmFrom}` : task.procrastinated ? `Deferred${task.procrastinateDays===null ? '' : ` · ${task.procrastinateDays} days`}` : '';
      return `<div class="task-card ${task.procrastinated ? 'deferred' : ''}"><div class="task-main"><button type="button" class="complete-button" data-task="${idx}" data-operation="deleted" aria-label="Complete ${e(task.description)}" title="Complete task">${icons.check}</button><h4>${e(task.description)}</h4></div><div class="task-meta"><span class="priority p-${task.priority>=8?'high':task.priority>=5?'medium':'low'}">${icons.flag} Importance ${task.priority}/10</span><button class="text-button edit" type="button" data-task="${idx}" data-operation="edit" aria-label="Edit ${e(task.description)}">Edit</button></div>${detail?`<p class="task-detail">${icons.clock}${e(detail)}</p>`:''}<details class="task-more"><summary>More options</summary><button type="button" data-task="${idx}" data-operation="cancelled">Cancel · added by mistake</button><button type="button" data-task="${idx}" data-operation="dropped">Drop · no longer pursuing</button></details></div>`;
    }).join('')}</div></article>`).join('');
  }
  function timeline(s: Snapshot, count?: number) {
    const events = count ? s.schedule.events.slice(0,count) : s.schedule.events;
    return events.length ? `<ol class="timeline">${events.map(event=>`<li><time>${e(event.time)}</time><span class="timeline-dot ${event.syncable ? 'work' : ''}"></span><span>${e(event.label)}</span>${event.syncable?'<span class="pill light">Focus</span>':''}</li>`).join('')}</ol>` : empty('No blocks scheduled for today. Build your routine in Schedule.');
  }
  function deadlineList(s: Snapshot) {
    return s.deadlines.length ? `<ul class="deadline-list">${s.deadlines.map((d,i)=>`<li><div><strong>${e(d.event)}</strong><small>${e(d.deadline)}</small></div><span class="deadline-status ${e(d.status)}">${d.daysLeft===null?'Invalid date':d.daysLeft===0?'Today':d.daysLeft<0?`${-d.daysLeft}d overdue`:`${d.daysLeft}d left`}</span><button class="text-button" data-deadline="${i}" data-operation="edit" type="button">Edit</button><button class="text-button danger" data-deadline="${i}" data-operation="remove" aria-label="Remove ${e(d.event)}" type="button">${icons.close}</button></li>`).join('')}</ul>` : empty('Nothing due. Enjoy the breathing room.');
  }
  function schedulePage(s: Snapshot) {
    return `<div class="page-heading"><div><p class="eyebrow">A RHYTHM THAT WORKS FOR YOU</p><h1>Make a little space<span class="mint">.</span></h1><p class="muted">${e(s.today.parity)} week · ${e(s.today.weekday)} · schedule ${s.config.activeId}</p></div>${button('schedule-pdf','Download schedule',icons.schedule)}</div><section class="split-grid"><article class="card"><div class="section-heading compact"><h2>Today’s timeline</h2><span class="pill light">${s.schedule.events.length} blocks</span></div>${timeline(s)}</article><div><article class="card sync-card"><p class="eyebrow">${icons.spark} PLAN YOUR FOCUS</p><h2>Give your blocks a purpose.</h2><p class="muted">Assign tasks to today’s focus blocks. Review the proposal before it is saved.</p>${state.sync ? `<p>${e(state.sync.summary ?? '')}</p><ol class="timeline">${state.sync.preview.map(item=>`<li><time>${e(item.time)}</time><span>${e(item.label)}</span></li>`).join('')}</ol><label>What would you change?<textarea name="feedback" placeholder="More time for writing, fewer context switches…"></textarea></label><div class="button-row">${button('sync-accept','Accept plan',icons.check,'button primary')}${button('sync-generate','Revise',icons.refresh)}${button('sync-dismiss','Discard','','button ghost')}</div>` : button('sync-generate','Plan my day',icons.spark,'button primary')}</article><article class="card assistant"><div class="section-heading compact"><h2>Schedule assistant</h2>${button('setup-new','New chat','','text-button')}</div><p class="muted">Build or adjust your weekly routine with a conversation.</p><div class="conversation">${state.messages.map(m=>`<div class="message ${m.role}"><small>${m.role === 'user' ? 'You' : 'Schedule assistant'}</small><p>${e(m.text)}</p></div>`).join('') || '<p class="muted">Tell me about your working hours, goals, habits, and fixed commitments.</p>'}</div>${state.setup?.schedule_summary && !state.setup.bundle ? `<div class="proposal"><h3>Schedule summary</h3><p>${e(state.setup.schedule_summary)}</p>${button('setup-confirm','Confirm summary',icons.check,'button primary')}</div>` : ''}${state.setup?.bundle ? `<div class="proposal"><h3>Review your new routine</h3>${Object.entries(state.setup.bundle).map(([file,text])=>`<details><summary>${e(file)}</summary><pre>${e(text)}</pre></details>`).join('')}${state.setup.profile_markdown ? `<details><summary>Profile draft</summary><pre>${e(state.setup.profile_markdown)}</pre></details>` : ''}${button('setup-accept','Apply schedule',icons.check,'button primary')}</div>` : ''}<form id="setup-form"><label class="sr-only" for="setup-message">Message to the schedule assistant</label><textarea id="setup-message" name="message" required placeholder="I work 9 to 5 and want to make room for…"></textarea><label class="attachment-field">Attach a timetable (text or image, up to 5 MB)<input type="file" name="timetable" accept="image/*,.txt,.md,.csv,.tsv,.json,.toml,.yaml,.yml"></label><button class="button primary" type="submit">${icons.spark} Send message</button></form><small class="muted">AI uses your locally configured pi account and model.</small></article></div></section>`;
  }
  function routines(s: Snapshot) {
    const done=s.habits.filter(h=>h.completed).length;
    return `<div class="page-heading"><div><p class="eyebrow">SMALL THINGS, STEADY PROGRESS</p><h1>Keep your promises<span class="mint">.</span></h1><p class="muted">Daily habits and the dates that matter.</p></div>${button('add-deadline','New deadline',icons.plus,'button primary')}</div><section class="split-grid"><article class="card"><div class="section-heading"><h2>Today’s habits</h2><span class="pill light">${done}/${s.habits.length} complete</span></div><div class="habit-progress"><div style="width:${s.habits.length?done/s.habits.length*100:0}%"></div></div>${s.habits.length ? s.habits.map(h=>`<label class="habit-row ${h.completed?'done':''}"><input type="checkbox" data-habit="${e(h.id)}" ${h.completed?'checked':''}><span>${e(h.description)}</span>${h.completed?icons.check:''}</label>`).join('') : empty('Add habits in Settings to build your daily rhythm.')}<p class="muted">Edit your habit definitions in Settings → Habits.</p></article><article class="card"><h2>Deadlines</h2>${deadlineList(s)}</article></section>`;
  }
  function activity(s: Snapshot) {
    return `<div class="page-heading"><div><p class="eyebrow">LOOK HOW FAR YOU’VE COME</p><h1>Little wins add up<span class="mint">.</span></h1><p class="muted">Completed, cancelled, and dropped tasks keep their own history.</p></div></div><article class="card"><div class="section-heading"><h2>Recent activity</h2><label>Show<select name="historyCount">${[5,10,25,50,0].map(count=>`<option value="${count}" ${state.historyCount===count?'selected':''}>${count===0?'All activity':`Last ${count}`}</option>`).join('')}</select></label></div>${s.history.length ? `<ul class="activity-list">${s.history.map(h=>`<li><span class="activity-check">${icons.check}</span><div><strong>${e(h.description)}</strong><small>${e(new Date(h.endedAt).toLocaleString())} · ${e(h.duration)}</small></div><span class="pill light">${e(h.status ?? 'completed')}</span><span class="priority">Importance ${h.priority}</span></li>`).join('')}</ul>` : empty('Your first small win will appear here.')}</article><article class="card reports"><h2>A bigger picture</h2><p class="muted">Download a weekly or monthly productivity report.</p><div class="button-row">${field('reportDate','Period containing','date',s.today.date)}${button('weekly','Weekly PDF',icons.activity)}${button('monthly','Monthly PDF',icons.activity)}</div></article>`;
  }
  function settings(s: Snapshot) {
    const files={settings:'General settings',odd:'Odd week schedule',even:'Even week schedule',habits:'Habits',deadlines:'Deadlines',profile:'Profile',model:'AI model'};
    return `<div class="page-heading"><div><p class="eyebrow">MAKE IT YOURS</p><h1>Your workspace, your rules<span class="mint">.</span></h1><p class="muted">All changes are saved to your computer.</p></div></div><section class="split-grid"><article class="card"><h2>Reminder preferences</h2><p class="muted">${state.workspace?.mode === 'p' ? 'P mode · selected event alarms paused' : 'J mode · all reminders allowed'}</p>${button('mode',state.workspace?.mode === 'p' ? 'Switch to J mode' : 'Switch to P mode',icons.settings)}<hr><label>Active schedule<select name="configId">${state.workspace?.configIds.map(id=>`<option value="${id}" ${id===s.config.activeId?'selected':''}>Schedule ${id}</option>`).join('')}</select></label><div class="button-row">${button('switch','Activate')}${button('update','Reload reminders',icons.refresh)}${button('stop','Stop reminders','','button ghost danger')}</div><p class="muted path">${e(s.config.rootDir)}</p><hr><h3>Task types</h3><p class="muted">Names used to group your task landscape. Keep IDs stable so existing tasks retain their type.</p><div id="type-rows">${Object.entries(s.taskTypes).map(([id,name])=>typeRow(id,name)).join('')}</div><div class="button-row">${button('types-save','Save task types',icons.check)}${button('types-add','Add type',icons.plus,'button ghost')}</div><hr>${button('shutdown','Close browser service','','button ghost')}</article><article class="card"><h2>Configuration editor</h2><p class="muted">Edit your schedules, habits, settings, or optional AI model here. Syntax is checked before saving.</p>${state.config ? `<label>File<select name="configFile">${Object.entries(files).map(([id,label])=>`<option value="${id}" ${id===state.config!.file?'selected':''}>${label}</option>`).join('')}</select></label><label>Contents<textarea class="code-editor" name="configContent" rows="22" spellcheck="false">${e(state.config.content)}</textarea></label>${button('config-save','Save changes',icons.check,'button primary')}` : button('open-config','Open editor',icons.settings)}<p class="muted">Saved edits reload the installed reminder service when available.</p></article></section>`;
  }
  async function sendSetup(message: string, confirmSummary=false) {
    const file = root.querySelector<HTMLInputElement>('[name="timetable"]')?.files?.[0];
    let attachment: {name: string; data: string} | undefined;
    if (file) {
      if (file.size > 5 * 1024 * 1024) throw new Error('Choose a file smaller than 5 MB.');
      const encoded = await new Promise<string>((resolve,reject)=>{const reader=new FileReader();reader.onload=()=>resolve(String(reader.result).split(',')[1]);reader.onerror=()=>reject(new Error('Could not read the attachment.'));reader.readAsDataURL(file);});
      attachment={name:file.name,data:encoded};
    }
    state.messages.push({role:'user',text:message + (attachment ? `\nAttached: ${attachment.name}` : '')}); draw();
    state.setup = await request<SetupTurn>('setup_turn',{message,sessionId:state.setup?.sessionId,confirmSummary,attachment});
    state.messages.push({role:'assistant',text:[state.setup.conversation,state.setup.question_to_user].filter(Boolean).join('\n\n')}); draw();
  }
  function dialog(title: string, body: string, submit: (data: FormData)=>Promise<void>) {
    root.querySelector('dialog')?.remove();
    const el=document.createElement('dialog'); el.className='editor-dialog'; el.innerHTML=`<form><div class="section-heading"><h2>${title}</h2><button class="text-button" type="button" aria-label="Close">${icons.close}</button></div>${body}<div class="button-row"><button class="button primary" type="submit">Save</button><button class="button ghost" type="button" data-close>Cancel</button></div><p class="dialog-error" role="alert"></p></form>`;
    root.append(el); el.showModal();
    el.querySelectorAll<HTMLButtonElement>('button[type="button"]').forEach(btn=>btn.onclick=()=>{el.close();el.remove();});
    el.querySelector('form')!.onsubmit=event=>{event.preventDefault(); const btn=el.querySelector<HTMLButtonElement>('[type="submit"]')!; btn.disabled=true; void submit(new FormData(event.target as HTMLFormElement)).then(()=>{el.close();el.remove();}).catch(error=>{el.querySelector('.dialog-error')!.textContent=error instanceof Error?error.message:String(error);btn.disabled=false;});};
  }
  function taskDialog(task?: TaskItem) {
    const days=task?.alarmFrom?Math.max(0,Math.round((Date.parse(task.alarmFrom)-Date.parse(state.snapshot!.today.date))/86400000)):0;
    dialog(task?'Edit task':'A new thing to work on',`${field('description','Task','text',task?.description ?? '', 'required maxlength="500"')}${field('priority','Importance · 1 to 10','number',String(task?.priority??5),'required min="1" max="10"')}<label>Task type<select name="type">${typeOptions(state.snapshot!,task?.type)}</select></label>${field('postpone','Start reminders in (days)','number',String(days),'required min="0" max="3650"')}`,async data=>{await request(task?'task_update':'task_add',{description:data.get('description'),priority:Number(data.get('priority')),type:data.get('type'),postpone:Number(data.get('postpone')),originalDescription:task?.description});await reload();});
  }
  function deadlineDialog(deadline?: DeadlineItem) {
    dialog(deadline?'Edit deadline':'A date to remember',`${field('event','Event','text',deadline?.event??'','required')}${field('date','Due date','date',deadline?.deadline??state.snapshot!.today.date,'required')}`,async data=>{await request(deadline?'deadline_update':'deadline_add',{event:data.get('event'),date:data.get('date'),originalEvent:deadline?.event});await reload();});
  }
  root.addEventListener('workspace-refresh',()=>{if (state.busy || document.hidden || root.querySelector('dialog[open]') || ['settings','schedule','activity'].includes(state.page) || ['INPUT','TEXTAREA','SELECT'].includes(document.activeElement?.tagName ?? '')) return;void reload().catch(()=>{state.notice='Could not refresh. Check the local server connection.';status();});});
  root.addEventListener('workspace-activate',()=>{state.notice='Your existing workspace is ready.';status();});
  try {await reload();} catch(error){root.innerHTML=`<div class="loading"><h1>Could not open your workspace</h1><p>${e(error instanceof Error?error.message:String(error))}</p><button class="button" id="retry">Try again</button></div>`;root.querySelector('#retry')?.addEventListener('click',()=>void renderApp(root,client));}
}
