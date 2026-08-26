import { useSyncExternalStore } from "react";
import type { DownloadEvent, Update } from "@tauri-apps/plugin-updater";
import { IN_TAURI } from "@/lib/api";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export interface UpdateSnapshot {
  phase: UpdatePhase;
  currentVersion: string | null;
  availableVersion: string | null;
  progress: number;
  error: string;
  errorKind: "check" | "install" | null;
  lastCheckedAt: number | null;
}

const AUTO_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;
const ACTIVE_RECHECK_INTERVAL_MS = 60 * 60 * 1000;
const MANUAL_FEEDBACK_MS = 350;

let snapshot: UpdateSnapshot = {
  phase: "idle",
  currentVersion: null,
  availableVersion: null,
  progress: 0,
  error: "",
  errorKind: null,
  lastCheckedAt: null,
};
let pendingUpdate: Update | null = null;
let checkInFlight: Promise<UpdateSnapshot> | null = null;
let installInFlight: Promise<void> | null = null;
let versionInFlight: Promise<string | null> | null = null;
const listeners = new Set<() => void>();

function publish(patch: Partial<UpdateSnapshot>): UpdateSnapshot {
  snapshot = { ...snapshot, ...patch };
  for (const listener of listeners) listener();
  return snapshot;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot() {
  return snapshot;
}

async function ensureCurrentVersion(): Promise<string | null> {
  if (snapshot.currentVersion || !IN_TAURI) return snapshot.currentVersion;
  versionInFlight ??= import("@tauri-apps/api/app")
    .then(({ getVersion }) => getVersion())
    .then((version) => {
      publish({ currentVersion: version });
      return version;
    })
    .catch((error) => {
      console.error("failed to read app version:", error);
      return null;
    })
    .finally(() => {
      versionInFlight = null;
    });
  return versionInFlight;
}

/** Shared update check used by the banner, automatic timer, and Settings. */
export function checkForUpdates(options: { manual?: boolean } = {}) {
  if (
    !IN_TAURI ||
    snapshot.phase === "downloading" ||
    snapshot.phase === "installing"
  ) {
    return Promise.resolve(snapshot);
  }
  if (checkInFlight) return checkInFlight;

  const startedAt = Date.now();
  publish({ phase: "checking", error: "", errorKind: null });
  checkInFlight = Promise.all([
    import("@tauri-apps/plugin-updater").then(({ check }) =>
      check({ timeout: 20_000 }),
    ),
    ensureCurrentVersion(),
  ])
    .then(async ([update, currentVersion]) => {
      if (options.manual) {
        const remaining = MANUAL_FEEDBACK_MS - (Date.now() - startedAt);
        if (remaining > 0) {
          await new Promise((resolve) => window.setTimeout(resolve, remaining));
        }
      }

      const previousUpdate = pendingUpdate;
      pendingUpdate = update;
      if (previousUpdate && previousUpdate !== update) {
        void previousUpdate.close().catch(() => {});
      }

      if (update) {
        return publish({
          phase: "available",
          currentVersion: update.currentVersion || currentVersion,
          availableVersion: update.version,
          progress: 0,
          error: "",
          errorKind: null,
          lastCheckedAt: Date.now(),
        });
      }
      return publish({
        phase: "upToDate",
        currentVersion,
        availableVersion: null,
        progress: 0,
        error: "",
        errorKind: null,
        lastCheckedAt: Date.now(),
      });
    })
    .catch((error) =>
      publish({
        phase: pendingUpdate ? "available" : "error",
        error: String(error),
        errorKind: "check",
        lastCheckedAt: Date.now(),
      }),
    )
    .finally(() => {
      checkInFlight = null;
    });
  return checkInFlight;
}

/** Download, install, and relaunch using the update found by the latest check. */
export function installAvailableUpdate() {
  if (!pendingUpdate) return Promise.resolve();
  if (installInFlight) return installInFlight;

  publish({ phase: "downloading", progress: 0, error: "", errorKind: null });
  let downloaded = 0;
  let total = 0;
  const onDownload = (event: DownloadEvent) => {
    if (event.event === "Started") {
      total = event.data.contentLength ?? 0;
    } else if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
      if (total > 0) {
        publish({
          progress: Math.min(100, Math.round((downloaded / total) * 100)),
        });
      }
    } else if (event.event === "Finished") {
      publish({ phase: "installing", progress: 100 });
    }
  };

  installInFlight = pendingUpdate
    .downloadAndInstall(onDownload)
    .then(async () => {
      publish({ phase: "installing", progress: 100 });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    })
    .catch((error) => {
      publish({
        phase: "available",
        error: String(error),
        errorKind: "install",
      });
    })
    .finally(() => {
      installInFlight = null;
    });
  return installInFlight;
}

function automaticCheckIfDue() {
  if (
    snapshot.availableVersion ||
    snapshot.phase === "downloading" ||
    snapshot.phase === "installing"
  ) {
    return;
  }
  if (
    snapshot.lastCheckedAt &&
    Date.now() - snapshot.lastCheckedAt < ACTIVE_RECHECK_INTERVAL_MS
  ) {
    return;
  }
  void checkForUpdates();
}

/**
 * Check at launch, every six hours while TokBar stays open, and when the main
 * window becomes active after at least an hour without another attempt.
 */
export function startAutomaticUpdateChecks() {
  if (!IN_TAURI) return () => {};
  void ensureCurrentVersion();
  automaticCheckIfDue();

  const timer = window.setInterval(automaticCheckIfDue, AUTO_CHECK_INTERVAL_MS);
  const onActive = () => {
    if (document.visibilityState === "visible") automaticCheckIfDue();
  };
  window.addEventListener("focus", onActive);
  document.addEventListener("visibilitychange", onActive);
  return () => {
    window.clearInterval(timer);
    window.removeEventListener("focus", onActive);
    document.removeEventListener("visibilitychange", onActive);
  };
}

export function useAppUpdater() {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
