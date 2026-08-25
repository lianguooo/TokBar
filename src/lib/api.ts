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
  /** Session title (first user message); empty when unavailable. */
  title: string;
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

export interface RetentionSourcePreview {
  agent: string;
  sessions: number;
  files: number;
  bytes: number;
  totalTokens: number;
  totalCost: number;
}

export interface RetentionPreview {
  retentionDays: number;
  cutoffMs: number;
  sessions: number;
  files: number;
  bytes: number;
  totalTokens: number;
  totalCost: number;
  skippedSessions: number;
  sources: RetentionSourcePreview[];
}

export interface RetentionResult {
  preview: RetentionPreview;
  archivedFiles: number;
  deletedFiles: number;
  pendingFiles: number;
}

// --- Opt-in features -----------------------------------------------------
// Both default to off and are gated in Rust as well, so a disabled feature
// cannot be reached even if the UI is bypassed.

export interface CodexInjectStatus {
  /** Supervisor thread is alive. */
  running: boolean;
  /** CDP connected and the delete button installed in Codex. */
  attached: boolean;
  debugPort: number;
  codexAppPath: string;
  lastError: string;
  /** Codex is up but has no debug port, so it must be relaunched. */
  needsRelaunch: boolean;
}

export interface FeatureFlags {
  codexSwitchEnabled: boolean;
  sessionDeleteEnabled: boolean;
  /** Agents whose logs are one file per session; only these can be deleted
   *  one at a time. Comes from the backend so the list cannot drift. */
  sessionDeleteAgents: string[];
  /** Nested under sessionDelete: show the delete button inside Codex itself. */
  codexInjectEnabled: boolean;
  /** Override for the Codex app bundle; empty means autodetect. */
  codexAppPath: string;
  codexInjectStatus: CodexInjectStatus;
}

export type FeatureFlagKey = "codexSwitch" | "sessionDelete" | "codexInject";

export interface CodexAccount {
  id: string;
  name: string;
  /** Model written to config.toml when this account is selected. */
  model: string;
}

export interface CodexProvider {
  id: string;
  name: string;
  baseUrl: string;
  experimentalBearerToken: string;
  model: string;
}

export interface CodexSelection {
  kind: "account" | "provider";
  id: string;
  name: string;
}

export interface CodexSwitchState {
  accounts: CodexAccount[];
  providers: CodexProvider[];
  /** Selected provider id; empty means official ChatGPT. */
  currentProvider: string;
  officialMode: boolean;
  /** Account `auth.json` currently holds — set in provider mode too. */
  liveAccountId: string;
  /** Account that is the current switch target; empty in provider mode, so
   *  the account row always stays clickable as the way back. */
  currentAccountId: string;
  pendingAccount: CodexAccount | null;
  /** A login exists but is not saved yet, so it cannot be switched back to. */
  requiresCurrentAccountName: boolean;
  selection: CodexSelection | null;
  displayName: string;
  codexHome: string;
  /** Set once when a pending sign-in was adopted during this read. */
  capturedAccount: CodexAccount | null;
  /** Accounts in CodexPlusPlus's store that are not already saved here. */
  importableAccounts: number;
}

export interface CodexSwitchResult {
  state: CodexSwitchState;
  /** False when the config already matched and nothing was written. */
  changed: boolean;
  message: string;
}

export interface SessionDeletePreview {
  agent: string;
  sessionId: string;
  files: number;
  bytes: number;
  totalTokens: number;
  totalCost: number;
  /** Skipped: the log file also holds other sessions. */
  sharedFiles: number;
  /** Skipped: the log file changed since the last scan. */
  staleFiles: number;
}

export interface SessionDeleteResult {
  preview: SessionDeletePreview;
  archivedFiles: number;
  deletedFiles: number;
  pendingFiles: number;
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
  // Calendar arithmetic keeps the boundary at local midnight across DST.
  start.setDate(start.getDate() - (days - 1));
  return start.getTime();
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
  previewRetention: () => call<RetentionPreview>("preview_retention"),
  cleanupOldSessions: () => call<RetentionResult>("cleanup_old_sessions"),
  getSessionModels: (agent: string, sessionId: string, costMode?: CostMode) =>
    call<ModelRow[]>("get_session_models", { agent, sessionId, costMode }),
  getFeatureFlags: () => call<FeatureFlags>("get_feature_flags"),
  setFeatureFlag: (flag: FeatureFlagKey, enabled: boolean) =>
    call<FeatureFlags>("set_feature_flag", { flag, enabled }),

  codexSwitchState: () => call<CodexSwitchState>("codex_switch_state"),
  codexSwitchSelect: (kind: "account" | "provider", id: string) =>
    call<CodexSwitchResult>("codex_switch_select", { kind, id }),
  codexSwitchOfficial: () => call<CodexSwitchResult>("codex_switch_official"),
  codexProviderCreate: (p: {
    name: string;
    baseUrl: string;
    bearerToken: string;
    model: string;
  }) => call<CodexSwitchResult>("codex_provider_create", { ...p }),
  codexProviderUpdate: (p: {
    id: string;
    name: string;
    baseUrl: string;
    bearerToken: string;
    model: string;
  }) => call<CodexSwitchResult>("codex_provider_update", { ...p }),
  codexProviderDelete: (id: string) =>
    call<CodexSwitchResult>("codex_provider_delete", { id }),
  /** Copy account archives across from CodexPlusPlus. */
  codexImportAccounts: () => call<CodexSwitchResult>("codex_import_accounts"),
  /** Adopt the current login without signing out. */
  codexAccountCapture: (name: string, model: string) =>
    call<CodexSwitchResult>("codex_account_capture", { name, model }),
  /** Signs the current account out; the caller must confirm first. */
  codexAccountAdd: (p: {
    name: string;
    currentAccountName: string;
    model: string;
  }) => call<CodexSwitchResult>("codex_account_add", { ...p }),
  codexAccountUpdate: (p: { id: string; name: string; model: string }) =>
    call<CodexSwitchResult>("codex_account_update", { ...p }),
  codexAccountDelete: (id: string) =>
    call<CodexSwitchResult>("codex_account_delete", { id }),

  setCodexAppPath: (path: string) =>
    call<FeatureFlags>("set_codex_app_path", { path }),
  /** Relaunch Codex with the debug port and reattach. */
  codexInjectRestart: () => call<FeatureFlags>("codex_inject_restart"),

  previewSessionDelete: (agent: string, sessionId: string) =>
    call<SessionDeletePreview>("preview_session_delete", { agent, sessionId }),
  deleteSession: (agent: string, sessionId: string) =>
    call<SessionDeleteResult>("delete_session", { agent, sessionId }),

  getTrayMode: () => call<string>("get_tray_mode"),
  setTrayMode: (mode: string) => call<void>("set_tray_mode", { mode }),
  showMainWindow: () => call<void>("show_main_window"),
};
