import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

/** A flat-rate plan the user pays for (e.g. Claude Max $100/mo), pinned to
 *  the agent whose local usage it covers so we can price that usage at API
 *  rates and show the real ROI. */
export interface Subscription {
  id: string;
  name: string;
  monthlyUsd: number;
  /** Agent keys this subscription covers (see AGENT_LABELS in format.ts). */
  agents: string[];
}

const STORAGE_KEY = "tokbar-subscriptions";

function uid(): string {
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID();
  }
  return `s-${Date.now()}-${Math.floor(Math.random() * 1e9)}`;
}

function load(): Subscription[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.flatMap((s): Subscription[] => {
      if (!s || typeof s !== "object") return [];
      const o = s as Record<string, unknown>;
      if (
        typeof o.id !== "string" ||
        typeof o.name !== "string" ||
        typeof o.monthlyUsd !== "number"
      ) {
        return [];
      }
      let agents: string[] = [];
      if (Array.isArray(o.agents)) {
        agents = o.agents.filter((a): a is string => typeof a === "string");
      } else if (typeof o.agent === "string") {
        // Migrate the original single-agent shape.
        agents = [o.agent];
      }
      return [{ id: o.id, name: o.name, monthlyUsd: o.monthlyUsd, agents }];
    });
  } catch {
    return [];
  }
}

interface SubscriptionsValue {
  subscriptions: Subscription[];
  add: (sub: Omit<Subscription, "id">) => void;
  update: (id: string, patch: Partial<Omit<Subscription, "id">>) => void;
  remove: (id: string) => void;
}

const SubscriptionsContext = createContext<SubscriptionsValue>({
  subscriptions: [],
  add: () => {},
  update: () => {},
  remove: () => {},
});

export function SubscriptionsProvider({ children }: { children: ReactNode }) {
  const [subscriptions, setSubscriptions] = useState<Subscription[]>(load);

  const mutate = useCallback(
    (fn: (prev: Subscription[]) => Subscription[]) => {
      setSubscriptions((prev) => {
        const next = fn(prev);
        localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
        return next;
      });
    },
    [],
  );

  // Keep the main window and the menu-bar quick panel in sync: a storage
  // event fires in the other window when one of them writes.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === STORAGE_KEY) setSubscriptions(load());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  const add = useCallback(
    (sub: Omit<Subscription, "id">) =>
      mutate((prev) => [...prev, { ...sub, id: uid() }]),
    [mutate],
  );
  const update = useCallback(
    (id: string, patch: Partial<Omit<Subscription, "id">>) =>
      mutate((prev) =>
        prev.map((s) => (s.id === id ? { ...s, ...patch } : s)),
      ),
    [mutate],
  );
  const remove = useCallback(
    (id: string) => mutate((prev) => prev.filter((s) => s.id !== id)),
    [mutate],
  );

  return (
    <SubscriptionsContext.Provider
      value={{ subscriptions, add, update, remove }}
    >
      {children}
    </SubscriptionsContext.Provider>
  );
}

export function useSubscriptions() {
  return useContext(SubscriptionsContext);
}
