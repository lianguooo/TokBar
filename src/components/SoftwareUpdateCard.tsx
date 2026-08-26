import { LoaderCircle, RefreshCw } from "lucide-react";
import { IN_TAURI } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import {
  checkForUpdates,
  installAvailableUpdate,
  useAppUpdater,
} from "@/lib/updater";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

/** Manual update entry point backed by the same process-wide state as the banner. */
export function SoftwareUpdateCard() {
  const { t } = useI18n();
  const appUpdate = useAppUpdater();
  const busy =
    appUpdate.phase === "checking" ||
    appUpdate.phase === "downloading" ||
    appUpdate.phase === "installing";

  let status = t("update.automatic");
  if (!IN_TAURI) {
    status = t("update.desktopOnly");
  } else if (appUpdate.error) {
    status = t(
      appUpdate.errorKind === "install"
        ? "update.installFailed"
        : "update.checkFailed",
      { error: appUpdate.error },
    );
  } else if (appUpdate.availableVersion) {
    status = t("update.available", { v: appUpdate.availableVersion });
  } else if (appUpdate.phase === "upToDate") {
    status = t("update.upToDate");
  }

  let action = t("update.check");
  if (appUpdate.phase === "checking") {
    action = t("update.checking");
  } else if (appUpdate.phase === "downloading") {
    action = t("update.downloading", { pct: appUpdate.progress });
  } else if (appUpdate.phase === "installing") {
    action = t("update.installing");
  } else if (appUpdate.availableVersion) {
    action = t("update.action");
  }

  const run = () => {
    if (appUpdate.availableVersion) {
      void installAvailableUpdate();
    } else {
      void checkForUpdates({ manual: true });
    }
  };

  return (
    <Card>
      <CardHeader className="flex-row items-center gap-2 space-y-0">
        <RefreshCw
          className="h-4 w-4 text-muted-foreground"
          aria-hidden="true"
        />
        <CardTitle>{t("settings.softwareUpdate")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-col justify-between gap-3 sm:flex-row sm:items-center">
          <div className="space-y-1" aria-live="polite">
            <p className="text-sm font-medium">
              {appUpdate.currentVersion
                ? t("update.currentVersion", { v: appUpdate.currentVersion })
                : "TokBar"}
            </p>
            <p
              className={cn(
                "text-xs text-muted-foreground",
                appUpdate.error && "text-destructive",
              )}
            >
              {status}
            </p>
          </div>
          <Button
            variant="outline"
            onClick={run}
            disabled={!IN_TAURI || busy}
            aria-busy={busy}
            className="h-11 shrink-0"
          >
            {busy && (
              <LoaderCircle
                className="h-4 w-4 animate-spin motion-reduce:animate-none"
                aria-hidden="true"
              />
            )}
            {action}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
