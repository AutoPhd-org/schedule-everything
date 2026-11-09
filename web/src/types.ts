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

/** One step of a task. While a task has subtasks, its progress is the share of them done. */
export type Subtask = {text: string; done: boolean};

export type TaskItem = {
  description: string;
  priority: number;
  type: string;
  typeName: string;
  alarmFrom: string | null;
  procrastinated: boolean;
  procrastinateDays: number | null;
  /** A timed task's finish-by time, local `YYYY-MM-DDTHH:MM`; normal tasks have none. */
  due?: string | null;
  /** How far along the task is, 0–100. */
  progress?: number;
  subtasks?: Subtask[];
};

/** A task an AI agent suggested (`rmd suggest add`). It is not a task yet: it waits on the board, marked as a
 *  suggestion and left out of every count, until the user accepts it (it becomes a normal task) or rejects it. */
export type SuggestionItem = {
  id: string;
  description: string;
  priority: number;
  type: string;
  typeName: string | null;
  /** Finish-by time, local `YYYY-MM-DDTHH:MM`, when it would be a timed task. */
  due?: string | null;
  /** The daily urgent alarm would wait until this date. */
  alarmFrom?: string | null;
  /** Why the agent suggests it, in its own words. */
  reason?: string | null;
  /** Where the agent found it, such as Gmail. */
  source?: string | null;
  /** Who suggested it, such as Claude. */
  agent: string;
  suggestedAt?: string | null;
};

/** A timed task completed today that was due by tonight; it counts as finished work in today's progress. */
export type TimedDone = {description: string; priority?: number; type: string | null; due: string; endedAt: string};

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

/** A rebuttal with an `endDate` is a period that spans its days; without one it is a single date (often only the end is published). */
export type ConferenceKind = "abstract" | "submission" | "rebuttal" | "conference";
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

/** One day's statistics for the daily overview: what was finished, when (per hour, by type), and how that
 *  compares with yesterday and the usual day. Comparisons are null before there is history for them. */
export type DailyOverview = {
  date: string;
  today: string;
  isToday: boolean;
  firstActivity: string | null;
  taskTypes: Record<string, string>;
  completed: number;
  cancelled: number;
  dropped: number;
  added: number;
  byType: Record<string, number>;
  hours: Record<string, number>[];
  yesterday: number | null;
  usual: number | null;
  usualDays: number;
  habitsDone: number;
  habitsTotal: number;
  streak: number;
  medianLeadHours: number | null;
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
  timedDone?: TimedDone[];
  /** Tasks suggested by AI agents, waiting for review. */
  suggestions?: SuggestionItem[];
};

export type BridgeClient = {
  send<T = unknown>(command: string, payload: Record<string, unknown>): Promise<T>;
};
