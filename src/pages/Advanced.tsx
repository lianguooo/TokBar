import { useEffect, useState } from "react";
import {
  AlertTriangle,
  Archive,
  LoaderCircle,
  RefreshCw,
  Trash2,
} from "lucide-react";
import {
  api,
  type RetentionPreview,
  type RetentionResult,
  type FeatureFlagKey,
} from "@/lib/api";
import {
  agentLabel,
  formatBytes,
  formatCost,
  formatTokens,
} from "@/lib/format";
import { useFeatureFlags } from "@/lib/features";
import { useI18n, type I18nKey } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { CodexSwitchPanel } from "@/components/CodexSwitchPanel";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

type RetentionStatus =
  "idle" | "loading" | "ready" | "confirming" | "running" | "success" | "error";

/** Everything that reaches outside TokBar's own data lives here: two opt-in
 *  features that write to Codex, and the retention sweep that deletes source
 *  logs. Each is its own block so the blast radius of each is legible. */
export function AdvancedPage() {
  const { t } = useI18n();
  const { flags, setFlag, refresh: refreshFlags } = useFeatureFlags();
  const [appPathDraft, setAppPathDraft] = useState<string | null>(null);
  const [injectBusy, setInjectBusy] = useState(false);
  // A failed relaunch used to go to the console only, which made the button
  // look dead rather than broken.
  const [injectError, setInjectError] = useState("");
  const [retentionStatus, setRetentionStatus] =
    useState<RetentionStatus>("idle");
  const [retentionPreview, setRetentionPreview] =
    useState<RetentionPreview | null>(null);
  const [retentionResult, setRetentionResult] =
    useState<RetentionResult | null>(null);
  const [retentionError, setRetentionError] = useState("");

  const previewRetention = async () => {
    setRetentionStatus("loading");
    setRetentionError("");
    setRetentionResult(null);
    try {
      const preview = await api.previewRetention();
      setRetentionPreview(preview);
      setRetentionStatus("ready");
    } catch (error) {
      setRetentionError(String(error));
      setRetentionStatus("error");
    }
  };

  const cleanupRetention = async () => {
    setRetentionStatus("running");
    setRetentionError("");
    try {
      const result = await api.cleanupOldSessions();
      setRetentionResult(result);
      setRetentionPreview(null);
      setRetentionStatus("success");
    } catch (error) {
      setRetentionError(String(error));
      setRetentionStatus("error");
    }
  };

  // The injection status changes on its own (Codex restarts, page reloads),
  // so poll while it is on rather than showing a stale badge.
  useEffect(() => {
    if (!flags.codexInjectEnabled || !flags.sessionDeleteEnabled) return;
    const timer = setInterval(() => refreshFlags(), 4000);
    return () => clearInterval(timer);
  }, [flags.codexInjectEnabled, flags.sessionDeleteEnabled, refreshFlags]);

  const toggle = (flag: FeatureFlagKey, enabled: boolean) => (
    <button
      onClick={() => setFlag(flag, !enabled).catch(console.error)}
      className={cn(
        "relative h-6 w-11 shrink-0 rounded-full transition-colors",
        enabled ? "bg-primary" : "bg-muted",
      )}
      role="switch"
      aria-checked={enabled}
    >
      <span
        className={cn(
          "absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all",
          enabled ? "left-[22px]" : "left-0.5",
        )}
      />
    </button>
  );

  const featureRow = (
    labelKey: I18nKey,
    descKey: I18nKey,
    flag: FeatureFlagKey,
    enabled: boolean,
  ) => (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-border p-3">
      <div>
        <div className="text-sm font-medium">{t(labelKey)}</div>
        <div className="mt-0.5 text-xs text-muted-foreground">{t(descKey)}</div>
      </div>
      {toggle(flag, enabled)}
    </div>
  );

  const pillBtn =
    "rounded-lg border border-border px-4 py-2 text-sm font-medium transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50";

  return (
    <div className="space-y-6">
      <p className="text-xs text-muted-foreground">{t("advanced.desc")}</p>

      {/* Block 1: Codex account / provider switch */}
      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <RefreshCw className="h-4 w-4 text-muted-foreground" />
          <CardTitle>{t("settings.codexSwitch")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {featureRow(
            "advanced.enable",
            "settings.codexSwitchDesc",
            "codexSwitch",
            flags.codexSwitchEnabled,
          )}
          {flags.codexSwitchEnabled && <CodexSwitchPanel />}
        </CardContent>
      </Card>

      {/* Block 2: delete one session, in TokBar and optionally inside Codex */}
      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <Trash2 className="h-4 w-4 text-muted-foreground" />
          <CardTitle>{t("settings.sessionDelete")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          {featureRow(
            "advanced.enable",
            "settings.sessionDeleteDesc",
            "sessionDelete",
            flags.sessionDeleteEnabled,
          )}

          {/* Flat, not indented: the Codex-side toggle is a sibling of the main
              one, not a child of it. */}
          {flags.sessionDeleteEnabled && (
            <>
              {featureRow(
                "settings.codexInject",
                "settings.codexInjectDesc",
                "codexInject",
                flags.codexInjectEnabled,
              )}

              {flags.codexInjectEnabled && (
                <div className="space-y-2 rounded-lg border border-border p-3">
                  <p className="text-xs text-amber-500">
                    {t("settings.codexInjectConflict")}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {t("settings.codexInjectNoAutoLaunch")}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {t("settings.codexInjectMac")}
                  </p>

                  <div className="flex flex-wrap items-center gap-2">
                    <Badge
                      variant={
                        flags.codexInjectStatus.attached ? "success" : "warning"
                      }
                    >
                      {flags.codexInjectStatus.attached
                        ? t("settings.codexInjectAttached", {
                            port: flags.codexInjectStatus.debugPort,
                          })
                        : t("settings.codexInjectWaiting")}
                    </Badge>
                    <button
                      className={pillBtn}
                      disabled={injectBusy}
                      onClick={async () => {
                        setInjectBusy(true);
                        setInjectError("");
                        try {
                          await api.codexInjectRestart();
                          await refreshFlags();
                        } catch (error) {
                          setInjectError(String(error));
                        } finally {
                          setInjectBusy(false);
                        }
                      }}
                    >
                      {injectBusy && (
                        <LoaderCircle className="mr-1 inline h-3.5 w-3.5 animate-spin" />
                      )}
                      {t("settings.codexInjectRelaunch")}
                    </button>
                  </div>

                  {injectError && (
                    <p className="text-xs text-destructive">{injectError}</p>
                  )}

                  {flags.codexInjectStatus.lastError && (
                    <p className="text-xs text-destructive">
                      {flags.codexInjectStatus.lastError}
                    </p>
                  )}

                  <label className="block space-y-1">
                    <span className="text-xs text-muted-foreground">
                      {t("settings.codexInjectApp")}
                    </span>
                    <input
                      className="w-full rounded-lg border border-border bg-transparent px-3 py-2 text-xs outline-none focus:border-primary"
                      placeholder={t("settings.codexInjectAppPlaceholder")}
                      value={appPathDraft ?? flags.codexAppPath}
                      onChange={(e) => setAppPathDraft(e.target.value)}
                      onBlur={async () => {
                        if (appPathDraft === null) return;
                        const next = appPathDraft;
                        setAppPathDraft(null);
                        if (next === flags.codexAppPath) return;
                        try {
                          await api.setCodexAppPath(next);
                          await refreshFlags();
                        } catch (error) {
                          setInjectError(String(error));
                        }
                      }}
                    />
                    <span className="block font-mono text-xs text-muted-foreground">
                      {flags.codexInjectStatus.codexAppPath}
                    </span>
                  </label>
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>

      {/* Block 3: retention sweep */}
      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <Archive
            className="h-4 w-4 text-muted-foreground"
            aria-hidden="true"
          />
          <CardTitle>{t("advanced.retention")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col justify-between gap-3 sm:flex-row sm:items-start">
            <div className="space-y-1">
              <div className="text-sm font-medium">
                {t("settings.retentionPolicy")}
              </div>
              <p className="max-w-xl text-xs leading-relaxed text-muted-foreground">
                {t("settings.retentionDesc")}
              </p>
            </div>
            <button
              onClick={previewRetention}
              disabled={
                retentionStatus === "loading" || retentionStatus === "running"
              }
              className="inline-flex min-h-11 shrink-0 items-center justify-center gap-2 rounded-lg border border-border px-4 text-sm font-medium transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
            >
              {retentionStatus === "loading" && (
                <LoaderCircle
                  className="h-4 w-4 animate-spin motion-reduce:animate-none"
                  aria-hidden="true"
                />
              )}
              {t(
                retentionStatus === "loading"
                  ? "settings.retentionPreviewing"
                  : "settings.retentionPreview",
              )}
            </button>
          </div>

          <div aria-live="polite">
            {retentionPreview && retentionStatus !== "success" && (
              <div className="space-y-3 rounded-xl border border-border bg-muted/20 p-4">
                {retentionPreview.files > 0 ? (
                  <>
                    <div className="flex flex-wrap items-baseline justify-between gap-2">
                      <div className="text-sm font-semibold">
                        {t("settings.retentionSummary", {
                          sessions: retentionPreview.sessions,
                          files: retentionPreview.files,
                          size: formatBytes(retentionPreview.bytes),
                        })}
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {new Date(
                          retentionPreview.cutoffMs,
                        ).toLocaleDateString()}
                      </div>
                    </div>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("settings.retentionPreserve", {
                        tokens: formatTokens(retentionPreview.totalTokens),
                        cost: formatCost(retentionPreview.totalCost),
                      })}
                    </p>
                    <div className="flex flex-wrap gap-2">
                      {retentionPreview.sources.map((source) => (
                        <Badge key={source.agent} variant="outline">
                          {agentLabel(source.agent)} · {source.sessions}
                        </Badge>
                      ))}
                    </div>
                    {retentionPreview.skippedSessions > 0 && (
                      <p className="text-xs text-muted-foreground">
                        {t("settings.retentionSkipped", {
                          n: retentionPreview.skippedSessions,
                        })}
                      </p>
                    )}
                    {retentionStatus === "confirming" ? (
                      <div className="rounded-lg border border-destructive/40 bg-destructive/5 p-3">
                        <div className="flex gap-2">
                          <AlertTriangle
                            className="mt-0.5 h-4 w-4 shrink-0 text-destructive"
                            aria-hidden="true"
                          />
                          <div>
                            <div className="text-sm font-semibold text-destructive">
                              {t("settings.retentionConfirmTitle")}
                            </div>
                            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                              {t("settings.retentionConfirmDesc")}
                            </p>
                          </div>
                        </div>
                        <div className="mt-3 flex flex-wrap justify-end gap-2">
                          <button
                            onClick={() => setRetentionStatus("ready")}
                            className="min-h-11 rounded-lg border border-border px-4 text-sm font-medium transition-colors hover:bg-muted"
                          >
                            {t("settings.retentionCancel")}
                          </button>
                          <button
                            onClick={cleanupRetention}
                            className="min-h-11 rounded-lg bg-destructive px-4 text-sm font-semibold text-destructive-foreground transition-opacity hover:opacity-90"
                          >
                            {t("settings.retentionConfirm")}
                          </button>
                        </div>
                      </div>
                    ) : retentionStatus === "running" ? (
                      <div className="flex min-h-11 items-center gap-2 text-sm text-muted-foreground">
                        <LoaderCircle
                          className="h-4 w-4 animate-spin motion-reduce:animate-none"
                          aria-hidden="true"
                        />
                        {t("settings.retentionDeleting")}
                      </div>
                    ) : (
                      <button
                        onClick={() => setRetentionStatus("confirming")}
                        className="min-h-11 rounded-lg border border-destructive/50 px-4 text-sm font-semibold text-destructive transition-colors hover:bg-destructive/10"
                      >
                        {t("settings.retentionDelete")}
                      </button>
                    )}
                  </>
                ) : (
                  <p className="text-sm text-muted-foreground">
                    {t("settings.retentionEmpty")}
                  </p>
                )}
              </div>
            )}

            {retentionStatus === "success" && retentionResult && (
              <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/5 p-3 text-sm text-emerald-600 dark:text-emerald-400">
                {t("settings.retentionSuccess", {
                  sessions: retentionResult.preview.sessions,
                  files: retentionResult.deletedFiles,
                })}
                {retentionResult.pendingFiles > 0 && (
                  <div className="mt-1 text-xs text-amber-600 dark:text-amber-400">
                    {t("settings.retentionPending", {
                      n: retentionResult.pendingFiles,
                    })}
                  </div>
                )}
              </div>
            )}

            {retentionStatus === "error" && (
              <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
                {t("settings.retentionFailed", { error: retentionError })}
              </div>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
