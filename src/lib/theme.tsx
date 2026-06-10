import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type ThemeMode = "dark" | "light";
export type AccentKey = "amber" | "blue" | "emerald" | "violet" | "rose";

export const ACCENTS: Record<
  AccentKey,
  {
    label: string;
    swatch: string;
    primary: string;
    primaryFg: string;
    /// Chart series palette led by the accent — charts re-color with the theme.
    charts: [string, string, string, string, string];
  }
> = {
  amber: {
    label: "Amber",
    swatch: "#d97757",
    primary: "oklch(0.72 0.15 50)",
    primaryFg: "oklch(0.14 0.004 285)",
    charts: ["#d97757", "#38bdf8", "#34d399", "#fbbf24", "#c084fc"],
  },
  blue: {
    label: "Blue",
    swatch: "#3b82f6",
    primary: "oklch(0.62 0.19 260)",
    primaryFg: "oklch(0.98 0 0)",
    charts: ["#3b82f6", "#22d3ee", "#8b5cf6", "#34d399", "#f59e0b"],
  },
  emerald: {
    label: "Emerald",
    swatch: "#10b981",
    primary: "oklch(0.7 0.15 162)",
    primaryFg: "oklch(0.14 0.004 285)",
    charts: ["#10b981", "#38bdf8", "#a3e635", "#fbbf24", "#2dd4bf"],
  },
  violet: {
    label: "Violet",
    swatch: "#8b5cf6",
    primary: "oklch(0.61 0.22 293)",
    primaryFg: "oklch(0.98 0 0)",
    charts: ["#8b5cf6", "#ec4899", "#38bdf8", "#c084fc", "#f59e0b"],
  },
  rose: {
    label: "Rose",
    swatch: "#f43f5e",
    primary: "oklch(0.65 0.22 16)",
    primaryFg: "oklch(0.98 0 0)",
    charts: ["#f43f5e", "#fb923c", "#a78bfa", "#f472b6", "#38bdf8"],
  },
};

const MODE_KEY = "tokbar-theme-mode";
const ACCENT_KEY = "tokbar-accent";

function applyTheme(mode: ThemeMode, accent: AccentKey) {
  const root = document.documentElement;
  root.classList.toggle("light", mode === "light");
  // Keep the native window chrome (macOS traffic-light strip, Windows
  // title bar) in the same color world as the page.
  getCurrentWindow().setTheme(mode).catch(() => {});
  const a = ACCENTS[accent];
  root.style.setProperty("--primary", a.primary);
  root.style.setProperty("--primary-foreground", a.primaryFg);
  root.style.setProperty("--ring", a.primary);
  a.charts.forEach((color, i) => {
    root.style.setProperty(`--chart-${i + 1}`, color);
  });
}

function detectMode(): ThemeMode {
  const saved = localStorage.getItem(MODE_KEY);
  return saved === "light" ? "light" : "dark";
}

function detectAccent(): AccentKey {
  const saved = localStorage.getItem(ACCENT_KEY);
  return saved && saved in ACCENTS ? (saved as AccentKey) : "amber";
}

interface ThemeValue {
  mode: ThemeMode;
  accent: AccentKey;
  setMode: (mode: ThemeMode) => void;
  setAccent: (accent: AccentKey) => void;
}

const ThemeContext = createContext<ThemeValue>({
  mode: "dark",
  accent: "amber",
  setMode: () => {},
  setAccent: () => {},
});

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(detectMode);
  const [accent, setAccentState] = useState<AccentKey>(detectAccent);

  useEffect(() => {
    applyTheme(mode, accent);
  }, [mode, accent]);

  // Sync between the main window and the menu-bar quick panel.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === MODE_KEY && (e.newValue === "dark" || e.newValue === "light")) {
        setModeState(e.newValue);
      }
      if (e.key === ACCENT_KEY && e.newValue && e.newValue in ACCENTS) {
        setAccentState(e.newValue as AccentKey);
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  const setMode = useCallback((m: ThemeMode) => {
    localStorage.setItem(MODE_KEY, m);
    setModeState(m);
  }, []);

  const setAccent = useCallback((a: AccentKey) => {
    localStorage.setItem(ACCENT_KEY, a);
    setAccentState(a);
  }, []);

  return (
    <ThemeContext.Provider value={{ mode, accent, setMode, setAccent }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  return useContext(ThemeContext);
}
