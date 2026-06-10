import { invoke } from "@tauri-apps/api/core";

export type CostMode = "auto" | "calculate" | "display";
export type RangeKey = "today" | "7d" | "30d" | "90d" | "all";

export interface Totals {
  cost: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  totalTokens: number;
  requests: number;
  sessions: number;
  activeDays: number;
}

export interface AgentBreakdown {
  agent: string;
  cost: number;
  totalTokens: number;
  requests: number;
  sessions: number;
}

export interface Overview {
  totals: Totals;
  byAgent: AgentBreakdown[];
}

export interface DailyRow {
  date: string;
  agent: string;
  cost: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  totalTokens: number;
  requests: number;
}

export interface ModelRow {
  model: string;
  cost: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  totalTokens: number;
  requests: number;
}

export interface SessionRow {
  sessionId: string;
  agent: string;
  project: string;
  firstTs: number;
  lastTs: number;
  cost: number;
  totalTokens: number;
  requests: number;
  models: string;
}

export interface ProjectRow {
  project: string;
  cost: number;
  totalTokens: number;
  requests: number;
  sessions: number;
}

export interface Block {
  id: string;
  startMs: number;
  endMs: number;
  actualEndMs: number | null;
  isActive: boolean;
  isGap: boolean;
  cost: number;
  totalTokens: number;
  requests: number;
  models: string[];
  burnRateTpm: number | null;
  burnRateCostPerHour: number | null;
}

export interface ScanStats {
  filesTotal: number;
  filesParsed: number;
  filesRemoved: number;
  entriesInserted: number;
  durationMs: number;
}

export interface SourceInfo {
  agent: string;
  dirs: string[];
  fileCount: number;
}

export interface QueryParams {
  sinceMs?: number;
  untilMs?: number;
  costMode?: CostMode;
}

/** Start of local day, `days - 1` days ago (so "7d" covers today + 6 prior days). */
export function rangeToSinceMs(range: RangeKey): number | undefined {
  if (range === "all") return undefined;
  const days = { today: 1, "7d": 7, "30d": 30, "90d": 90 }[range];
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  return start.getTime() - (days - 1) * 86_400_000;
}

export const api = {
  refreshData: () => invoke<ScanStats>("refresh_data"),
  getOverview: (p: QueryParams) => invoke<Overview>("get_overview", { ...p }),
  getDaily: (p: QueryParams) => invoke<DailyRow[]>("get_daily", { ...p }),
  /** Same row shape as getDaily, bucketed by local hour ("HH:00"). */
  getHourly: (p: QueryParams) => invoke<DailyRow[]>("get_hourly", { ...p }),
  getModels: (p: QueryParams) => invoke<ModelRow[]>("get_models", { ...p }),
  getSessions: (p: QueryParams & { limit?: number }) =>
    invoke<SessionRow[]>("get_sessions", { ...p }),
  getProjects: (p: QueryParams & { limit?: number }) =>
    invoke<ProjectRow[]>("get_projects", { ...p }),
  getBlocks: (p: { sinceMs?: number; costMode?: CostMode }) =>
    invoke<Block[]>("get_blocks", { ...p }),
  getSources: () => invoke<SourceInfo[]>("get_sources"),
  getSessionModels: (agent: string, sessionId: string, costMode?: CostMode) =>
    invoke<ModelRow[]>("get_session_models", { agent, sessionId, costMode }),
};
