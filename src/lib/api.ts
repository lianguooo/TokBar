import { invoke } from "@tauri-apps/api/core";
import {
  listen,
  type EventCallback,
  type UnlistenFn,
} from "@tauri-apps/api/event";

/** True inside the Tauri app; false in a plain browser (dev preview /
 *  design QA), where deterministic mock data is served instead. */
export const IN_TAURI = "__TAURI_INTERNALS__" in window;

const call = <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
  IN_TAURI
    ? invoke<T>(cmd, args)
    : import("./mockApi").then((m) => m.mockInvoke<T>(cmd, args));

/** `listen()` that no-ops in the browser preview. */
export function onEvent<T>(
  event: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  return IN_TAURI ? listen(event, handler) : Promise.resolve(() => {});
}

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
  refreshData: () => call<ScanStats>("refresh_data"),
  /** Pull the latest LiteLLM pricing table and re-price all usage.
   *  Resolves to the number of models in the refreshed table. */
  refreshPricing: () => call<number>("refresh_pricing"),
  getOverview: (p: QueryParams) => call<Overview>("get_overview", { ...p }),
  getDaily: (p: QueryParams) => call<DailyRow[]>("get_daily", { ...p }),
  /** Same row shape as getDaily, bucketed by local hour ("HH:00"). */
  getHourly: (p: QueryParams) => call<DailyRow[]>("get_hourly", { ...p }),
  getModels: (p: QueryParams) => call<ModelRow[]>("get_models", { ...p }),
  getSessions: (p: QueryParams & { limit?: number }) =>
    call<SessionRow[]>("get_sessions", { ...p }),
  getProjects: (p: QueryParams & { limit?: number }) =>
    call<ProjectRow[]>("get_projects", { ...p }),
  getBlocks: (p: { sinceMs?: number; costMode?: CostMode }) =>
    call<Block[]>("get_blocks", { ...p }),
  getSources: () => call<SourceInfo[]>("get_sources"),
  getSessionModels: (agent: string, sessionId: string, costMode?: CostMode) =>
    call<ModelRow[]>("get_session_models", { agent, sessionId, costMode }),
  getTrayMode: () => call<string>("get_tray_mode"),
  setTrayMode: (mode: string) => call<void>("set_tray_mode", { mode }),
  showMainWindow: () => call<void>("show_main_window"),
};
