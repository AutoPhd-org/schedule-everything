import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { groupTasks, renderApp } from "./render";
import type { BridgeClient, Snapshot } from "./types";

const snapshot: Snapshot = {
  config: {rootDir:'/local/config',activeId:0,activeConfigDir:'/local/config/user_config_0',tasksPath:'',deadlinesPath:'',habitsPath:'',recordsPath:''},
  today:{date:'2026-10-05',weekday:'monday',parity:'even'},
  schedule:{isSkipped:false,current:'Write proposal',next:'Break',timeToNext:'15m',events:[{time:'09:00',label:'Write proposal',block:'pomodoro',syncable:true}],hasSyncedOverlay:true},
  taskTypes:{'1':'Coding','2':'Writing','3':'Reading'},
  tasks:[{description:'Write notes',priority:10,type:'2',typeName:'Writing',alarmFrom:null,procrastinated:false,procrastinateDays:null},...Array.from({length:12},(_,i)=>({description:`Task ${i}`,priority:9-i%5,type:'1',typeName:'Coding',alarmFrom:null,procrastinated:false,procrastinateDays:null})),{description:'Read paper',priority:5,type:'3',typeName:'Reading',alarmFrom:'2026-10-07',procrastinated:true,procrastinateDays:2}],
  deadlines:[{event:'Conference',deadline:'2026-10-06',daysLeft:1,status:'urgent',type:'2',typeName:'Writing'}],
  habits:[{id:'1',description:'Exercise',completed:false}],history:[]
};
let root: HTMLElement;
let send: ReturnType<typeof vi.fn>;
let client: BridgeClient;
const flush=()=>new Promise(resolve=>setTimeout(resolve,0));
const click=(selector:string)=>root.querySelector<HTMLButtonElement>(selector)!.click();
beforeEach(()=>{
  document.body.innerHTML='<div id="app"></div>';root=document.querySelector('#app')!;
  window.localStorage.clear();
  delete document.documentElement.dataset.theme;
  delete document.documentElement.dataset.themePreference;
  vi.stubGlobal('matchMedia',vi.fn(()=>({matches:false,addEventListener:vi.fn()})));
  HTMLDialogElement.prototype.showModal=function(){this.setAttribute('open','');};
  HTMLDialogElement.prototype.close=function(){this.removeAttribute('open');};
  send=vi.fn(async(command:string)=>command==='status_snapshot'?structuredClone(snapshot):command==='workspace_info'?{configIds:[0,1],activeId:0,mode:'j',rootDir:'/local/config'}:{});
  client={send}; vi.spyOn(window,'confirm').mockReturnValue(true);
});
afterEach(()=>{vi.restoreAllMocks();vi.unstubAllGlobals();});
describe('browser workspace',()=>{
  it('groups all tasks by type without truncating or changing importance order',async()=>{
    // Writing averages 10, Coding 7.3 over twelve tasks, Reading 5.
    const groups=groupTasks(snapshot);expect(groups.map(g=>g.name)).toEqual(['Writing','Coding','Reading']);expect(groups[1].tasks).toHaveLength(12);expect(groups[1].tasks[0].priority).toBe(9);expect(groups[1].average).toBe(7.3);
    await renderApp(root,client);expect(root.querySelectorAll('.task-card')).toHaveLength(14);expect(root.textContent).toContain('Importance 10/10');expect(root.textContent).toContain('Reminders in 2 days');expect(root.textContent).toContain('2026-10-07');
  });
  it('escapes user text',async()=>{
    const malicious=structuredClone(snapshot);malicious.tasks[0].description='<img src=x onerror=alert(1)>';
    send.mockImplementation(async(command:string)=>command==='status_snapshot'?malicious:{});
    await renderApp(root,client);expect(root.querySelector('img')).toBeNull();expect(root.textContent).toContain('<img src=x onerror=alert(1)>');
  });
  it('searches every task',async()=>{await renderApp(root,client);const input=root.querySelector<HTMLInputElement>('[name="search"]')!;input.value='Task 11';input.dispatchEvent(new Event('input'));expect(root.querySelectorAll('.task-card')).toHaveLength(1);});
  it('adds a task with type, priority, and reminder delay',async()=>{
    await renderApp(root,client);click('[data-action="add-task"]');await flush();
    root.querySelector<HTMLInputElement>('[name="description"]')!.value='Browser task';root.querySelector<HTMLInputElement>('[name="priority"]')!.value='8';root.querySelector<HTMLSelectElement>('[name="type"]')!.value='2';root.querySelector<HTMLInputElement>('[name="postpone"]')!.value='3';root.querySelector('dialog form')!.dispatchEvent(new Event('submit',{cancelable:true}));await flush();expect(send).toHaveBeenCalledWith('task_add',expect.objectContaining({description:'Browser task',priority:8,type:'2',postpone:3}));
  });
  it('edits tasks without losing their type or postponement',async()=>{await renderApp(root,client);click('[data-task="13"][data-operation="edit"]');expect(root.querySelector<HTMLSelectElement>('[name="type"]')!.value).toBe('3');expect(root.querySelector<HTMLInputElement>('[name="postpone"]')!.value).toBe('2');});
  it('distinguishes completed, cancelled, and dropped tasks',async()=>{await renderApp(root,client);click('[data-task="0"][data-operation="cancelled"]');await flush();expect(send).toHaveBeenCalledWith('task_delete',{description:'Write notes',action:'cancelled'});click('[data-task="0"][data-operation="deleted"]');await flush();expect(send).toHaveBeenCalledWith('task_delete',{description:'Write notes',action:'deleted'});});
  it('records habits in the browser',async()=>{await renderApp(root,client);click('[data-page="routines"]');const check=root.querySelector<HTMLInputElement>('[data-habit]')!;check.checked=true;check.dispatchEvent(new Event('change'));await flush();expect(send).toHaveBeenCalledWith('habit_mark',{habitIds:['1']});});
  it('keeps the interface usable after an operation fails',async()=>{await renderApp(root,client);send.mockRejectedValueOnce(new Error('Storage is busy'));click('[data-task="0"][data-operation="deleted"]');await flush();expect(root.querySelector('#notice .notice-text')?.textContent).toBe('Storage is busy');expect(root.querySelectorAll('.task-card')).toHaveLength(14);expect(root.querySelector<HTMLButtonElement>('[data-action="add-task"]')!.disabled).toBe(false);});
  it('opens and saves configuration with a revision',async()=>{send.mockImplementation(async(command:string)=>command==='status_snapshot'?snapshot:command==='workspace_info'?{configIds:[0],mode:'j'}:{file:'settings',content:'[settings]',revision:'r1',activeId:0,message:'Saved'});await renderApp(root,client);click('[data-page="settings"]');click('[data-action="open-config"]');await flush();expect(root.querySelector('[name="configContent"]')).not.toBeNull();click('[data-action="config-save"]');await flush();expect(send).toHaveBeenCalledWith('config_save',expect.objectContaining({revision:'r1',activeId:0,content:'[settings]'}));});
  it('shows agent plugin install steps on the Schedule page and copies commands',async()=>{
    const writeText=vi.fn(async()=>{});vi.stubGlobal('navigator',{clipboard:{writeText}});
    await renderApp(root,client);click('[data-page="schedule"]');
    expect(root.querySelector('#setup-form')).toBeNull();expect(root.querySelector('[data-action="sync-generate"]')).toBeNull();
    expect(root.querySelector('[data-agent-tab="claude"]')?.getAttribute('aria-selected')).toBe('true');
    expect(root.querySelector('.agent-steps')?.textContent).toContain('claude plugin marketplace add AutoPhd-org/schedule-everything');
    click('[data-agent-tab="antigravity"]');
    expect(root.querySelector('.agent-steps')?.textContent).toContain('agy plugin install ./schedule-everything/plugins/schedule-everything');
    click('.agent-steps [data-copy]');await flush();
    expect(writeText).toHaveBeenCalledWith('git clone https://github.com/AutoPhd-org/schedule-everything.git');
    expect(root.querySelector('#notice')?.textContent).toContain('Copied');
  });
  it('clears an agent-made plan for today',async()=>{
    await renderApp(root,client);click('[data-page="schedule"]');
    expect(root.querySelector('.plan-card')).not.toBeNull();
    click('[data-action="sync-clear"]');await flush();
    expect(send).toHaveBeenCalledWith('sync_clear',{});
  });

  it('adds a deadline with a shared task type',async()=>{
    await renderApp(root,client);click('[data-action="add-deadline"]');await flush();
    root.querySelector<HTMLInputElement>('[name="event"]')!.value='Submit draft';
    root.querySelector<HTMLInputElement>('[name="date"]')!.value='2026-10-10';
    root.querySelector<HTMLSelectElement>('[name="type"]')!.value='2';
    root.querySelector('dialog form')!.dispatchEvent(new Event('submit',{cancelable:true}));await flush();
    expect(send).toHaveBeenCalledWith('deadline_add',expect.objectContaining({event:'Submit draft',date:'2026-10-10',type:'2'}));
  });

  it('shows deadline categories on both pages and keeps the selected type when editing',async()=>{
    await renderApp(root,client);
    expect(root.querySelector('.deadline-list .type-badge')?.textContent).toBe('Writing');
    click('[data-page="routines"]');
    expect(root.querySelector('.deadline-list .type-badge')?.textContent).toBe('Writing');
    click('[data-deadline="0"][data-operation="edit"]');
    expect(root.querySelector<HTMLSelectElement>('[name="type"]')!.value).toBe('2');
    root.querySelector<HTMLInputElement>('[name="event"]')!.value='Conference draft';
    root.querySelector('dialog form')!.dispatchEvent(new Event('submit',{cancelable:true}));await flush();
    expect(send).toHaveBeenCalledWith('deadline_update',expect.objectContaining({event:'Conference draft',originalEvent:'Conference',type:'2'}));
  });

  it('supports uncategorized deadlines and preserves unavailable categories',async()=>{
    const data=structuredClone(snapshot);
    data.deadlines[0].type=null;data.deadlines[0].typeName=null;
    send.mockImplementation(async(command:string)=>command==='status_snapshot'?data:{});
    await renderApp(root,client);
    expect(root.querySelector('.type-badge')?.textContent).toBe('Uncategorized');
    click('[data-deadline="0"][data-operation="edit"]');
    expect(root.querySelector<HTMLSelectElement>('[name="type"]')!.value).toBe('');
    data.deadlines[0].type='99';data.deadlines[0].typeName='Retired category';
    await renderApp(root,client);click('[data-deadline="0"][data-operation="edit"]');
    expect(root.querySelector<HTMLSelectElement>('[name="type"]')!.value).toBe('99');
  });

  it('prioritizes deadlines without changing edit targets and shows all of them on the routines page',async()=>{
    const data=structuredClone(snapshot);
    data.deadlines=[7,-1,0,2].map((days,i)=>({event:`Deadline ${i}`,deadline:'2026-10-05',daysLeft:days,status:days<0?'overdue':days===0?'today':'soon',type:null,typeName:null}));
    send.mockImplementation(async(command:string)=>command==='status_snapshot'?data:{});
    await renderApp(root,client);
    expect([...root.querySelectorAll('.deadline-list strong')].map(el=>el.textContent)).toEqual(['Deadline 1','Deadline 2','Deadline 3']);
    expect(root.querySelector('[data-metric="overdue"] .status-value')?.textContent).toBe('1');
    expect(root.querySelector('[data-metric="today"] .status-value')?.textContent).toBe('1');
    click('[data-deadline="1"][data-operation="edit"]');
    expect(root.querySelector<HTMLInputElement>('[name="event"]')!.value).toBe('Deadline 1');
    root.querySelector('dialog')!.remove();click('[data-page="routines"]');
    expect(root.querySelectorAll('.deadline-list li')).toHaveLength(4);
  });

  it('excludes postponed tasks from ready high-importance counts and keeps category colors stable during search',async()=>{
    const data=structuredClone(snapshot);data.tasks[0].alarmFrom='2026-10-06';
    send.mockImplementation(async(command:string)=>command==='status_snapshot'?data:{});
    await renderApp(root,client);
    const ready=data.tasks.filter(task=>task.priority>=8&&!task.alarmFrom).length;
    expect(root.querySelector('[data-metric="priority"] .status-value')?.textContent).toBe(String(ready));
    expect(root.querySelector('.status-danger')).toBeNull();
    const group=[...root.querySelectorAll('.task-group')].find(el=>el.querySelector('h3')?.textContent==='Reading')!;
    const originalClass=group.className;
    const search=root.querySelector<HTMLInputElement>('[name="search"]')!;search.value='Read paper';search.dispatchEvent(new Event('input'));
    expect(root.querySelector('.task-group')?.className).toBe(originalClass);
  });

  it('follows system appearance by default and remembers explicit theme choices',async()=>{
    vi.stubGlobal('matchMedia',vi.fn(()=>({matches:true,addEventListener:vi.fn()})));
    await renderApp(root,client);
    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(root.querySelector('[data-theme-choice="system"]')?.getAttribute('aria-pressed')).toBe('true');
    click('[data-theme-choice="light"]');
    expect(document.documentElement.dataset.theme).toBe('light');
    expect(window.localStorage.getItem('schedule-everything-theme')).toBe('light');
    delete document.documentElement.dataset.themePreference;
    await renderApp(root,client);
    expect(root.querySelector('[data-theme-choice="light"]')?.getAttribute('aria-pressed')).toBe('true');
    click('[data-theme-choice="dark"]');click('[data-page="activity"]');
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('lets the notice be dismissed with its close button or Escape',async()=>{
    vi.stubGlobal('matchMedia',vi.fn((query:string)=>({matches:query.includes('reduced-motion'),addEventListener:vi.fn()})));
    await renderApp(root,client);
    const notice=()=>root.querySelector<HTMLElement>('#notice')!;
    click('[data-task="0"][data-operation="deleted"]');await flush();await flush();
    expect(notice().classList.contains('visible')).toBe(true);
    expect(notice().textContent).toContain('Completed “Write notes”');
    click('[data-notice-close]');
    expect(notice().classList.contains('visible')).toBe(false);
    expect(notice().querySelector('.notice-text')!.textContent).toBe('');
    click('[data-task="0"][data-operation="deleted"]');await flush();await flush();
    expect(notice().classList.contains('visible')).toBe(true);
    document.body.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
    expect(notice().classList.contains('visible')).toBe(false);
  });
  describe('undoing a removal from the notice',()=>{
    const token={description:'Write notes',action:'deleted',tasks:[{index:0,task:{description:'Write notes',priority:10,type:'2'}}],procrastinated:null,logged:['2026-10-05T16:00:00+00:00']};
    const undoButton=()=>root.querySelector<HTMLButtonElement>('[data-notice-undo]')!;
    const noticeText=()=>root.querySelector('#notice .notice-text')!.textContent;
    const ctrlZ=(target:EventTarget=document.body)=>target.dispatchEvent(new KeyboardEvent('keydown',{key:'z',ctrlKey:true,bubbles:true,cancelable:true}));
    beforeEach(()=>{
      const base=send.getMockImplementation()!;
      send.mockImplementation(async(command:string,payload:Record<string,unknown>)=>command==='task_delete'?{description:payload.description,deleted:1,undo:token}:command==='task_restore'?{description:'Write notes',restored:1}:base(command,payload));
    });
    it('offers Undo after completing a task and puts the task back',async()=>{
      await renderApp(root,client);
      expect(undoButton().hidden).toBe(true);
      click('[data-task="0"][data-operation="deleted"]');await flush();await flush();
      expect(noticeText()).toBe('Completed “Write notes”. Nice work.');
      expect(undoButton().hidden).toBe(false);
      undoButton().click();await flush();await flush();
      expect(send).toHaveBeenCalledWith('task_restore',token);
      expect(noticeText()).toBe('“Write notes” is back on your list.');
      expect(undoButton().hidden).toBe(true);
      expect(root.querySelector('.task-card.just-added h4')?.textContent).toBe('Write notes');
    });
    it('offers Undo after cancelling or dropping a task too',async()=>{
      await renderApp(root,client);
      click('[data-task="0"][data-operation="dropped"]');await flush();await flush();
      expect(noticeText()).toBe('Removed “Write notes”.');
      expect(undoButton().hidden).toBe(false);
    });
    it('undoes with Ctrl+Z, but not while typing in a field',async()=>{
      await renderApp(root,client);
      click('[data-task="0"][data-operation="deleted"]');await flush();await flush();
      const search=root.querySelector<HTMLInputElement>('[name="search"]')!;
      search.focus();ctrlZ(search);await flush();
      expect(send).not.toHaveBeenCalledWith('task_restore',expect.anything());
      search.blur();
      ctrlZ();await flush();await flush();
      expect(send).toHaveBeenCalledWith('task_restore',token);
      expect(send.mock.calls.filter(([command])=>command==='task_restore')).toHaveLength(1);
    });
    it('lets the chance to undo go once the notice is dismissed or replaced',async()=>{
      vi.stubGlobal('matchMedia',vi.fn((query:string)=>({matches:query.includes('reduced-motion'),addEventListener:vi.fn()})));
      await renderApp(root,client);
      click('[data-task="0"][data-operation="deleted"]');await flush();await flush();
      click('[data-notice-close]');
      expect(undoButton().hidden).toBe(true);
      ctrlZ();await flush();
      click('[data-task="0"][data-operation="deleted"]');await flush();await flush();
      root.dispatchEvent(new Event('workspace-activate'));
      expect(noticeText()).toBe('Your existing workspace is ready.');
      expect(undoButton().hidden).toBe(true);
      ctrlZ();await flush();
      expect(send).not.toHaveBeenCalledWith('task_restore',expect.anything());
    });
    it('reports a failed undo in the notice',async()=>{
      await renderApp(root,client);
      click('[data-task="0"][data-operation="deleted"]');await flush();await flush();
      send.mockRejectedValueOnce(new Error('“Write notes” is already on your list.'));
      undoButton().click();await flush();await flush();
      expect(noticeText()).toBe('“Write notes” is already on your list.');
      expect(undoButton().hidden).toBe(true);
    });
  });
  it('slides the notice away before hiding it',async()=>{
    await renderApp(root,client);
    click('[data-task="0"][data-operation="deleted"]');await flush();await flush();
    click('[data-notice-close]');
    const notice=root.querySelector<HTMLElement>('#notice')!;
    expect(notice.classList.contains('leaving')).toBe(true);
    notice.dispatchEvent(new Event('animationend'));
    expect(notice.classList.contains('visible')).toBe(false);
    expect(notice.classList.contains('leaving')).toBe(false);
  });
  it('switches themes without discarding unsaved form contents',async()=>{
    await renderApp(root,client);click('[data-compose="1"]');
    const draft=root.querySelector<HTMLInputElement>('form.composer [name="composerText"]')!;draft.value='An unsaved task';
    click('[data-theme-choice="dark"]');
    expect(root.querySelector('form.composer [name="composerText"]')).toBe(draft);
    expect(draft.value).toBe('An unsaved task');
  });
  it('points to the task board for managing types instead of editing them in Settings',async()=>{
    await renderApp(root,client);click('[data-page="settings"]');
    expect(root.querySelector('[data-type-row]')).toBeNull();
    expect(root.querySelector('[data-action="types-save"]')).toBeNull();
    expect(root.textContent).toContain('Types are managed on the task board');
    click('.page-settings [data-page="today"]');
    expect(root.querySelector('#task-board .new-type')).not.toBeNull();
  });
});
