export type BridgeError = {
  code: string;
  message: string;
  details: Record<string, unknown>;
};

export type BridgeResponse<T> =
  | { ok: true; data: T }
  | { ok: false; error: BridgeError };

export type ScheduleEvent = {
  time: string;
  label: string;
  block: string | null;
  syncable: boolean;
};

export type SchedulePreviewEvent = {
  time: string;
  label: string;
  block: string | null;
};

export type TaskItem = {
  description: string;
  priority: number;
  type: string;
  typeName: string;
  alarmFrom: string | null;
  procrastinated: boolean;
  procrastinateDays: number | null;
};

export type DeadlineItem = {
  event: string;
  deadline: string;
  daysLeft: number | null;
  status: string;
};

export type HabitItem = {
  id: string;
  description: string;
  completed: boolean;
};

export type HistoryItem = {
  description: string;
  priority: number;
  startedAt: string;
  endedAt: string;
  status?: string;
  duration: string;
};

export type Snapshot = {
  config: {
    rootDir: string;
    activeId: number;
    activeConfigDir: string;
    tasksPath: string;
    deadlinesPath: string;
    habitsPath: string;
    recordsPath: string;
    error?: string | null;
  };
  today: {
    date: string;
    weekday: string;
    parity: string;
  };
  schedule: {
    isSkipped: boolean;
    current: string | null;
    next: string | null;
    timeToNext: string | null;
    events: ScheduleEvent[];
    hasSyncedOverlay: boolean;
  };
  tasks: TaskItem[];
  deadlines: DeadlineItem[];
  habits: HabitItem[];
  taskTypes: Record<string, string>;
  history: HistoryItem[];
};

export type SyncProposal = {
  summary: string | null;
  plan: {
    target_date: string;
    parity: string;
    weekday: string;
    assignments: Record<string, { block: string; title: string }>;
  };
  preview: SchedulePreviewEvent[];
};

export type BridgeClient = {
  send<T = unknown>(command: string, payload: Record<string, unknown>): Promise<T>;
};
