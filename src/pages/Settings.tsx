import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import {
  Database,
  DollarSign,
  FolderSearch,
  Languages,
  MenuSquare,
  Palette,
  Rocket,
} from "lucide-react";
import { api, type CostMode, type ScanStats, type SourceInfo } from "@/lib/api";
import { agentLabel } from "@/lib/format";
import { useI18n, type I18nKey, type Lang } from "@/lib/i18n";
import { ACCENTS, useTheme, type AccentKey, type ThemeMode } from "@/lib/theme";
import { cn } from "@/lib/utils";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

type TrayMode = "cost" | "tokens" | "off";

export function SettingsPage({
  costMode,
  onCostModeChange,
  lastScan,
}: {
  costMode: CostMode;
  onCostModeChange: (mode: CostMode) => void;
  lastScan: ScanStats | null;
}) {
  const { t, lang, setLang } = useI18n();
  const { mode, accent, setMode, setAccent } = useTheme();
  const [sources, setSources] = useState<SourceInfo[]>([]);
  const [trayMode, setTrayMode] = useState<TrayMode>("cost");
  const [autostart, setAutostart] = useState(false);

  useEffect(() => {
    api.getSources().then(setSources);
    invoke<string>("get_tray_mode").then((m) => setTrayMode(m as TrayMode));
    isEnabled().then(setAutostart).catch(console.error);
  }, []);

  const changeTrayMode = (m: TrayMode) => {
    setTrayMode(m);
    invoke("set_tray_mode", { mode: m }).catch(console.error);
  };

  const toggleAutostart = async () => {
    try {
      if (autostart) {
        await disable();
        setAutostart(false);
      } else {
        await enable();
        setAutostart(true);
      }
    } catch (e) {
      console.error("autostart toggle failed:", e);
    }
  };

  const languages: { value: Lang; label: string }[] = [
    { value: "zh", label: "中文" },
    { value: "en", label: "English" },
  ];

  const modes: { value: CostMode; label: string; desc: string }[] = [
    {
      value: "auto",
      label: t("settings.mode.auto"),
      desc: t("settings.mode.autoDesc"),
    },
    {
      value: "calculate",
      label: t("settings.mode.calculate"),
      desc: t("settings.mode.calculateDesc"),
    },
    {
      value: "display",
      label: t("settings.mode.display"),
      desc: t("settings.mode.displayDesc"),
    },
  ];

  return (
    <div className="max-w-3xl space-y-4">
      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <Languages className="h-4 w-4 text-muted-foreground" />
          <CardTitle>{t("settings.language")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <p className="text-xs text-muted-foreground">
            {t("settings.languageDesc")}
          </p>
          <div className="flex gap-2">
            {languages.map((l) => (
              <button
                key={l.value}
                onClick={() => setLang(l.value)}
                className={`rounded-lg border px-4 py-2 text-sm font-medium transition-colors ${
                  lang === l.value
                    ? "border-primary bg-primary/10 text-foreground"
                    : "border-border text-muted-foreground hover:bg-accent hover:text-foreground"
                }`}
              >
                {l.label}
              </button>
            ))}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <Rocket className="h-4 w-4 text-muted-foreground" />
          <CardTitle>{t("settings.general")}</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-between rounded-lg border border-border p-3">
            <div>
              <div className="text-sm font-medium">{t("settings.autostart")}</div>
              <div className="mt-0.5 text-xs text-muted-foreground">
                {t("settings.autostartDesc")}
              </div>
            </div>
            <button
              onClick={toggleAutostart}
              className={cn(
                "relative h-6 w-11 shrink-0 rounded-full transition-colors",
                autostart ? "bg-primary" : "bg-muted",
              )}
              role="switch"
              aria-checked={autostart}
            >
              <span
                className={cn(
                  "absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all",
                  autostart ? "left-[22px]" : "left-0.5",
                )}
              />
            </button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <Palette className="h-4 w-4 text-muted-foreground" />
          <CardTitle>{t("settings.appearance")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div>
            <div className="mb-2 text-xs text-muted-foreground">
              {t("settings.themeMode")}
            </div>
            <div className="flex gap-2">
              {(["dark", "light"] as ThemeMode[]).map((m) => (
                <button
                  key={m}
                  onClick={() => setMode(m)}
                  className={cn(
                    "rounded-lg border px-4 py-2 text-sm font-medium transition-colors",
                    mode === m
                      ? "border-primary bg-primary/10 text-foreground"
                      : "border-border text-muted-foreground hover:bg-accent hover:text-foreground",
                  )}
                >
                  {t(m === "dark" ? "settings.theme.dark" : "settings.theme.light")}
                </button>
              ))}
            </div>
          </div>
          <div>
            <div className="mb-2 text-xs text-muted-foreground">
              {t("settings.accentColor")}
            </div>
            <div className="flex gap-3">
              {(Object.keys(ACCENTS) as AccentKey[]).map((key) => (
                <button
                  key={key}
                  onClick={() => setAccent(key)}
                  title={ACCENTS[key].label}
                  className={cn(
                    "h-8 w-8 rounded-full transition-transform hover:scale-110",
                    accent === key &&
                      "ring-2 ring-foreground ring-offset-2 ring-offset-background",
                  )}
                  style={{ backgroundColor: ACCENTS[key].swatch }}
                />
              ))}
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <MenuSquare className="h-4 w-4 text-muted-foreground" />
          <CardTitle>{t("settings.trayDisplay")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <p className="text-xs text-muted-foreground">
            {t("settings.trayDisplayDesc")}
          </p>
          <div className="flex gap-2">
            {(
              [
                ["cost", "settings.tray.cost"],
                ["tokens", "settings.tray.tokens"],
                ["off", "settings.tray.off"],
              ] as [TrayMode, I18nKey][]
            ).map(([m, key]) => (
              <button
                key={m}
                onClick={() => changeTrayMode(m)}
                className={cn(
                  "rounded-lg border px-4 py-2 text-sm font-medium transition-colors",
                  trayMode === m
                    ? "border-primary bg-primary/10 text-foreground"
                    : "border-border text-muted-foreground hover:bg-accent hover:text-foreground",
                )}
              >
                {t(key)}
              </button>
            ))}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <FolderSearch className="h-4 w-4 text-muted-foreground" />
          <CardTitle>{t("settings.dataSources")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          {sources.map((s) => (
            <div
              key={s.agent}
              className="flex items-start justify-between gap-4 rounded-lg border border-border p-3"
            >
              <div>
                <div className="text-sm font-medium">{agentLabel(s.agent)}</div>
                {s.dirs.length > 0 ? (
                  s.dirs.map((d) => (
                    <div
                      key={d}
                      className="mt-1 font-mono text-xs text-muted-foreground"
                    >
                      {d}
                    </div>
                  ))
                ) : (
                  <div className="mt-1 text-xs text-muted-foreground">
                    {t("settings.notDetected")}
                  </div>
                )}
              </div>
              <Badge variant={s.fileCount > 0 ? "success" : "default"}>
                {t("settings.files", { n: s.fileCount })}
              </Badge>
            </div>
          ))}
          {lastScan && (
            <p className="text-xs text-muted-foreground">
              {t("settings.lastScan", {
                parsed: lastScan.filesParsed,
                total: lastScan.filesTotal,
                entries: lastScan.entriesInserted,
                ms: lastScan.durationMs,
              })}
            </p>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <DollarSign className="h-4 w-4 text-muted-foreground" />
          <CardTitle>{t("settings.costMode")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          {modes.map((m) => (
            <button
              key={m.value}
              onClick={() => onCostModeChange(m.value)}
              className={`w-full rounded-lg border p-3 text-left transition-colors ${
                costMode === m.value
                  ? "border-primary bg-primary/10"
                  : "border-border hover:bg-accent"
              }`}
            >
              <div className="text-sm font-medium">{m.label}</div>
              <div className="text-xs text-muted-foreground">{m.desc}</div>
            </button>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex-row items-center gap-2 space-y-0">
          <Database className="h-4 w-4 text-muted-foreground" />
          <CardTitle>{t("settings.pricing")}</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-xs text-muted-foreground">
            {t("settings.pricingDesc")}
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
