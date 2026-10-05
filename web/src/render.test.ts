import { beforeEach, describe, expect, it, vi } from "vitest";
import { groupTasks, renderApp } from "./render";
import type { BridgeClient, Snapshot } from "./types";

const snapshot: Snapshot = {
  config: {rootDir:'/local/config',activeId:0,activeConfigDir:'/local/config/user_config_0',tasksPath:'',deadlinesPath:'',habitsPath:'',recordsPath:''},
  today:{date:'2026-10-05',weekday:'monday',parity:'even'},
  schedule:{isSkipped:false,current:'Write proposal',next:'Break',timeToNext:'15m',events:[{time:'09:00',label:'Write proposal',block:'pomodoro',syncable:true}],hasSyncedOverlay:true},
  taskTypes:{'1':'Coding','2':'Writing','3':'Reading'},
  tasks:[{description:'Write notes',priority:10,type:'2',typeName:'Writing',alarmFrom:null,procrastinated:false,procrastinateDays:null},...Array.from({length:12},(_,i)=>({description:`Task ${i}`,priority:9-i%5,type:'1',typeName:'Coding',alarmFrom:null,procrastinated:false,procrastinateDays:null})),{description:'Read paper',priority:5,type:'3',typeName:'Reading',alarmFrom:'2026-10-07',procrastinated:true,procrastinateDays:2}],
  deadlines:[{event:'Conference',deadline:'2026-10-06',daysLeft:1,status:'urgent'}],
  habits:[{id:'1',description:'Exercise',completed:false}],history:[]
};
let root: HTMLElement;
let send: ReturnType<typeof vi.fn>;
let client: BridgeClient;
const flush=()=>new Promise(resolve=>setTimeout(resolve,0));
const click=(selector:string)=>root.querySelector<HTMLButtonElement>(selector)!.click();
beforeEach(()=>{
  document.body.innerHTML='<div id="app"></div>';root=document.querySelector('#app')!;
  HTMLDialogElement.prototype.showModal=function(){this.setAttribute('open','');};
  HTMLDialogElement.prototype.close=function(){this.removeAttribute('open');};
  send=vi.fn(async(command:string)=>command==='status_snapshot'?structuredClone(snapshot):command==='workspace_info'?{configIds:[0,1],activeId:0,mode:'j',rootDir:'/local/config'}:{});
  client={send}; vi.spyOn(window,'confirm').mockReturnValue(true);
});
describe('browser workspace',()=>{
  it('groups all tasks by type without truncating or changing importance order',async()=>{
    const groups=groupTasks(snapshot);expect(groups.map(g=>g.name)).toEqual(['Coding','Writing','Reading']);expect(groups[0].tasks).toHaveLength(12);expect(groups[0].tasks[0].priority).toBe(9);
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
  it('keeps the interface usable after an operation fails',async()=>{await renderApp(root,client);send.mockRejectedValueOnce(new Error('Storage is busy'));click('[data-task="0"][data-operation="deleted"]');await flush();expect(root.querySelector('#notice')?.textContent).toBe('Storage is busy');expect(root.querySelectorAll('.task-card')).toHaveLength(14);expect(root.querySelector<HTMLButtonElement>('[data-action="add-task"]')!.disabled).toBe(false);});
  it('opens and saves configuration with a revision',async()=>{send.mockImplementation(async(command:string)=>command==='status_snapshot'?snapshot:command==='workspace_info'?{configIds:[0],mode:'j'}:{file:'settings',content:'[settings]',revision:'r1',activeId:0,message:'Saved'});await renderApp(root,client);click('[data-page="settings"]');click('[data-action="open-config"]');await flush();expect(root.querySelector('[name="configContent"]')).not.toBeNull();click('[data-action="config-save"]');await flush();expect(send).toHaveBeenCalledWith('config_save',expect.objectContaining({revision:'r1',activeId:0,content:'[settings]'}));});
  it('reviews sync proposals before applying them',async()=>{send.mockImplementation(async(command:string)=>command==='status_snapshot'?snapshot:command==='workspace_info'?{configIds:[0],mode:'j'}:command==='sync_generate'?{summary:'A focused day',plan:{target_date:'2026-10-05',parity:'even',weekday:'monday',assignments:{}},preview:[]}:{});await renderApp(root,client);click('[data-page="schedule"]');click('[data-action="sync-generate"]');await flush();expect(send).not.toHaveBeenCalledWith('sync_accept',expect.anything());click('[data-action="sync-accept"]');await flush();expect(send).toHaveBeenCalledWith('sync_accept',expect.objectContaining({plan:expect.anything()}));});
  it('conducts schedule conversations and confirms summaries',async()=>{send.mockImplementation(async(command:string)=>command==='status_snapshot'?snapshot:command==='workspace_info'?{configIds:[0],mode:'j'}:command==='setup_turn'?{sessionId:'session-1',phase:'summary',conversation:'Your routine',question_to_user:'Ready?',schedule_summary:'Work 9–5',profile_markdown:null,bundle:null}:{});await renderApp(root,client);click('[data-page="schedule"]');root.querySelector<HTMLTextAreaElement>('[name="message"]')!.value='I work 9 to 5';root.querySelector('#setup-form')!.dispatchEvent(new Event('submit',{cancelable:true}));await flush();expect(root.textContent).toContain('Work 9–5');click('[data-action="setup-confirm"]');await flush();expect(send).toHaveBeenCalledWith('setup_turn',expect.objectContaining({sessionId:'session-1',confirmSummary:true}));});
});

it('loads all task history on request',async()=>{
  send.mockImplementation(async(command:string)=>command==='status_snapshot'?snapshot:command==='workspace_info'?{configIds:[0],mode:'j'}:command==='task_history'?{activities:[{description:'Older task',priority:6,startedAt:'2026-09-01T09:00:00',endedAt:'2026-09-01T11:00:00',duration:'2 hours',status:'completed'}]}:{});
  await renderApp(root,client);click('[data-page="activity"]');const select=root.querySelector<HTMLSelectElement>('[name="historyCount"]')!;select.value='0';select.dispatchEvent(new Event('change'));await flush();expect(send).toHaveBeenCalledWith('task_history',{all:true});expect(root.textContent).toContain('Older task');
});
