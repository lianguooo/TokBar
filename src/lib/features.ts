import { useEffect, useState } from "react";
import { api, type FeatureFlagKey, type FeatureFlags } from "@/lib/api";

/** Both opt-in features start off; the Rust side enforces the same default,
 *  so this is only about what the UI offers. */
const DEFAULT: FeatureFlags = {
  codexSwitchEnabled: false,
  sessionDeleteEnabled: false,
  sessionDeleteAgents: [],
  codexInjectEnabled: false,
  codexAppPath: "",
  codexInjectStatus: {
    running: false,
    attached: false,
    debugPort: 0,
    codexAppPath: "",
    lastError: "",
    needsRelaunch: false,
  },
};

// Module-level cache + subscribers instead of a context provider: Settings and
// Sessions are lazy-loaded siblings, and this keeps a toggle in one visible in
// the other without threading state through App.
let cached: FeatureFlags = DEFAULT;
let loaded = false;
let inFlight: Promise<FeatureFlags> | null = null;
const listeners = new Set<(flags: FeatureFlags) => void>();

function publish(flags: FeatureFlags) {
  cached = flags;
  loaded = true;
  for (const listener of listeners) listener(flags);
}

function ensureLoaded(): Promise<FeatureFlags> {
  if (loaded) return Promise.resolve(cached);
  inFlight ??= api
    .getFeatureFlags()
    .then((flags) => {
      publish(flags);
      return flags;
    })
    .catch(() => DEFAULT)
    .finally(() => {
      inFlight = null;
    });
  return inFlight;
}

export function useFeatureFlags() {
  const [flags, setFlags] = useState<FeatureFlags>(cached);

  useEffect(() => {
    listeners.add(setFlags);
    ensureLoaded().then(setFlags);
    return () => {
      listeners.delete(setFlags);
    };
  }, []);

  const setFlag = async (flag: FeatureFlagKey, enabled: boolean) => {
    publish(await api.setFeatureFlag(flag, enabled));
  };

  const refresh = async () => {
    try {
      publish(await api.getFeatureFlags());
    } catch {
      // Leave the last known flags in place; the card shows lastError.
    }
  };

  return { flags, setFlag, refresh };
}
