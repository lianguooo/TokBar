import { useEffect, useState } from "react";
import { Download, X } from "lucide-react";
import { IN_TAURI } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { Button } from "@/components/ui/button";

// Minimal shape of the plugin-updater `Update` object we rely on. The
// plugin is imported dynamically so it never lands in the browser-preview
// bundle (where no Tauri runtime exists).
interface TauriUpdate {
  version: string;
  downloadAndInstall: (
    onEvent?: (e: { event: string; data?: { contentLength?: number; chunkLength?: number } }) => void,
  ) => Promise<void>;
}

type Phase = "idle" | "downloading" | "installing";

/**
 * Checks GitHub Releases (via tauri-plugin-updater) once on mount and, when a
 * newer signed release exists, offers a one-click update-and-restart. Renders
 * nothing outside Tauri, when up to date, or after the user dismisses it.
 */
export function UpdateBanner() {
  const { t } = useI18n();
  const [update, setUpdate] = useState<TauriUpdate | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [phase, setPhase] = useState<Phase>("idle");
  const [pct, setPct] = useState(0);

  useEffect(() => {
    if (!IN_TAURI) return;
    let cancelled = false;
    import("@tauri-apps/plugin-updater")
      .then((m) => m.check())
      .then((u) => {
        if (!cancelled && u) setUpdate(u as unknown as TauriUpdate);
      })
      .catch((e) => console.error("update check failed:", e));
    return () => {
      cancelled = true;
    };
  }, []);

  if (!update || dismissed) return null;

  const runUpdate = async () => {
    try {
      setPhase("downloading");
      let downloaded = 0;
      let total = 0;
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") {
          total = e.data?.contentLength ?? 0;
        } else if (e.event === "Progress") {
          downloaded += e.data?.chunkLength ?? 0;
          if (total > 0) setPct(Math.min(100, Math.round((downloaded / total) * 100)));
        } else if (e.event === "Finished") {
          setPhase("installing");
        }
      });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      console.error("update failed:", e);
      setPhase("idle");
    }
  };

  const busy = phase !== "idle";

  return (
    <div className="flex items-center gap-3 border-b border-primary/20 bg-primary/10 px-6 py-2 text-sm">
      <Download className="h-4 w-4 shrink-0 text-primary" />
      <span className="min-w-0 flex-1 truncate">
        {t("update.available", { v: update.version })}
      </span>
      <Button size="sm" onClick={runUpdate} disabled={busy} className="min-w-28">
        {phase === "downloading"
          ? t("update.downloading", { pct })
          : phase === "installing"
            ? t("update.installing")
            : t("update.action")}
      </Button>
      <button
        onClick={() => setDismissed(true)}
        disabled={busy}
        aria-label={t("update.later")}
        className="rounded-md p-1 text-muted-foreground transition-colors hover:text-foreground disabled:opacity-40"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}
