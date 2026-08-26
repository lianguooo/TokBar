import { useEffect, useState } from "react";
import { Download, X } from "lucide-react";
import { useI18n } from "@/lib/i18n";
import {
  installAvailableUpdate,
  startAutomaticUpdateChecks,
  useAppUpdater,
} from "@/lib/updater";
import { Button } from "@/components/ui/button";

/**
 * Starts the process-wide automatic checker and surfaces a signed release.
 * Settings consumes the same state, so checks and downloads never overlap.
 */
export function UpdateBanner() {
  const { t } = useI18n();
  const update = useAppUpdater();
  const [dismissedVersion, setDismissedVersion] = useState<string | null>(null);

  useEffect(() => startAutomaticUpdateChecks(), []);

  if (
    !update.availableVersion ||
    dismissedVersion === update.availableVersion
  ) {
    return null;
  }
  const busy = update.phase === "downloading" || update.phase === "installing";

  return (
    <div
      className="flex items-center gap-3 border-b border-primary/20 bg-primary/10 px-6 py-2 text-sm"
      aria-live="polite"
    >
      <Download className="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
      <span className="min-w-0 flex-1 truncate">
        {t("update.available", { v: update.availableVersion })}
      </span>
      <Button
        size="sm"
        onClick={installAvailableUpdate}
        disabled={busy}
        className="min-w-28"
      >
        {update.phase === "downloading"
          ? t("update.downloading", { pct: update.progress })
          : update.phase === "installing"
            ? t("update.installing")
            : t("update.action")}
      </Button>
      <button
        onClick={() => setDismissedVersion(update.availableVersion)}
        disabled={busy}
        aria-label={t("update.later")}
        className="rounded-md p-1 text-muted-foreground transition-colors hover:text-foreground disabled:opacity-40"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}
