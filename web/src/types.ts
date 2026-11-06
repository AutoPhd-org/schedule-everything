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
  type: string | null;
  typeName: string | null;
  color?: string | null;
  repeat?: RepeatRule | null;
  repeatDay?: number | null;
};

export type RepeatRule = "weekly" | "biweekly" | "monthly";

export type ConferenceKind = "abstract" | "submission" | "conference";
export type ConferenceGroup = "ml" | "vision" | "language" | "ai" | "robotics";
export type ConferenceEvent = {
  id: string;
  conference: string;
  name: string;
  group: ConferenceGroup;
  edition: number;
  fullName?: string | null;
  link?: string | null;
  place?: string | null;
  kind: ConferenceKind;
  label: string;
  date: string;
  endDate: string | null;
  time: string | null;
  sourceTime: string | null;
};
export type ConferencePayload = {
  year: number;
  source: string;
  fetchedAt: string | null;
  error: string | null;
  conferences: {id: string; name: string; group: ConferenceGroup}[];
  events: ConferenceEvent[];
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
  type?: string;
  typeName?: string;
};

export type WeekStats = {
  weekStart: string;
  active: boolean;
  completed: number;
  cancelled: number;
  dropped: number;
  added: number;
  habitRate: number | null;
  avgPriority: number | null;
  medianLeadHours: number | null;
  byType: Record<string, number>;
};

export type WeekBaseline = Omit<WeekStats, "weekStart" | "active" | "completed" | "cancelled" | "dropped" | "added"> & {
  weeks: number;
  completed: number | null;
  cancelled: number | null;
  dropped: number | null;
  added: number | null;
};

export type WeekDay = {
  date: string;
  completed: number;
  byType: Record<string, number>;
  habitsDone: number;
  habitsTotal: number;
  future: boolean;
};

export type WeeklyOverview = {
  weekStart: string;
  weekEnd: string;
  today: string;
  isCurrentWeek: boolean;
  firstActivity: string | null;
  taskTypes: Record<string, string>;
  current: WeekStats;
  previous: WeekStats;
  baseline: WeekBaseline;
  trend: WeekStats[];
  days: WeekDay[];
  previousDays: WeekDay[];
  habits: {id: string; description: string; days: (boolean | null)[]; rate: number | null; previousRate: number | null}[];
  streak: number;
  highlights: {description: string; priority: number; type: string; endedAt: string}[];
};

export type Workspace = {configIds: number[]; activeId: number; rootDir: string; mode: string};

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
  /** Saved type colors; a type without one is colored by its place among the types. */
  taskTypeColors?: Record<string, string>;
  history: HistoryItem[];
};

export type BridgeClient = {
  send<T = unknown>(command: string, payload: Record<string, unknown>): Promise<T>;
};
