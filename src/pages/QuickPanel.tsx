import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, Flame, LayoutDashboard, RefreshCw } from "lucide-react";
import { api, type Block } from "@/lib/api";
import { formatCost, formatNumber, formatTime, formatTokens } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { Logo } from "@/components/Logo";

interface QuickStats {
  todayCost: number;
  todayTokens: number;
  todayRequests: number;
  monthCost: number;
  activeBlock: Block | null;
}

function startOfToday(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function startOfMonth(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  d.setDate(1);
  return d.getTime();
}

export function QuickPanel() {
  const { t } = useI18n();
  const [stats, setStats] = useState<QuickStats | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [justRefreshed, setJustRefreshed] = useState(false);

  const load = useCallback(async () => {
    const [today, month, blocks] = await Promise.all([
      api.getOverview({ sinceMs: startOfToday() }),
      api.getOverview({ sinceMs: startOfMonth() }),
      api.getBlocks({ sinceMs: Date.now() - 86_400_000 }),
    ]);
    setStats({
      todayCost: today.totals.cost,
      todayTokens: today.totals.totalTokens,
      todayRequests: today.totals.requests,
      monthCost: month.totals.cost,
      activeBlock: blocks.find((b) => b.isActive) ?? null,
    });
  }, []);

  // Minimum spin + a brief check mark: the file watcher keeps data
  // current, so the scan usually finishes in milliseconds and the click
  // would otherwise look like a no-op.
  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      await Promise.all([
        api.refreshData(),
        new Promise((r) => setTimeout(r, 500)),
      ]);
      await load();
      setJustRefreshed(true);
      setTimeout(() => setJustRefreshed(false), 1800);
    } finally {
      setRefreshing(false);
    }
  }, [load]);

  // Reload when shown (focus), on live usage updates from the file
  // watcher, and on a slow poll so the burn rate stays current.
  useEffect(() => {
    load();
    window.addEventListener("focus", load);
    const timer = setInterval(load, 30_000);
    const unlisten = listen("usage-updated", load);
    return () => {
      window.removeEventListener("focus", load);
      clearInterval(timer);
      unlisten.then((fn) => fn());
    };
  }, [load]);

  const block = stats?.activeBlock ?? null;

  return (
    <div className="flex h-screen flex-col overflow-hidden rounded-2xl border border-border bg-background p-4 text-foreground">
      {/* Header */}
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Logo size={24} />
          <span className="text-sm font-semibold">TokBar</span>
        </div>
        <button
          onClick={refresh}
          className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          title={t("app.refresh")}
        >
          {justRefreshed && !refreshing ? (
            <Check className="h-3.5 w-3.5 text-primary" />
          ) : (
            <RefreshCw className={cn("h-3.5 w-3.5", refreshing && "animate-spin")} />
          )}
        </button>
      </div>

      {/* Today cost */}
      <div className="rounded-xl bg-card p-4">
        <div className="text-xs text-muted-foreground">{t("quick.todayCost")}</div>
        <div className="mt-1 text-3xl font-bold tabular-nums text-primary">
          {stats ? formatCost(stats.todayCost) : "—"}
        </div>
      </div>

      {/* Stats grid */}
      <div className="mt-3 grid grid-cols-3 gap-2">
        <MiniStat
          label={t("quick.todayTokens")}
          value={stats ? formatTokens(stats.todayTokens) : "—"}
        />
        <MiniStat
          label={t("quick.todayRequests")}
          value={stats ? formatNumber(stats.todayRequests) : "—"}
        />
        <MiniStat
          label={t("quick.monthCost")}
          value={stats ? formatCost(stats.monthCost) : "—"}
        />
      </div>

      {/* Active block */}
      <div className="mt-3 flex-1 rounded-xl bg-card p-4">
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Flame
            className={cn("h-3.5 w-3.5", block ? "text-primary" : "")}
          />
          {t("quick.activeBlock")}
        </div>
        {block ? (
          <div className="mt-2 space-y-2">
            <div className="flex items-baseline justify-between">
              <span className="text-xl font-semibold tabular-nums">
                {formatCost(block.cost)}
              </span>
              <span className="text-xs text-muted-foreground">
                {formatTokens(block.totalTokens)} tok
              </span>
            </div>
            <div className="space-y-1 text-xs text-muted-foreground">
              {block.burnRateTpm != null && (
                <div className="flex justify-between">
                  <span>{t("quick.burnRate")}</span>
                  <span className="tabular-nums text-foreground">
                    {formatTokens(Math.round(block.burnRateTpm))}/min
                  </span>
                </div>
              )}
              {block.burnRateCostPerHour != null && (
                <div className="flex justify-between">
                  <span>{t("quick.costRate")}</span>
                  <span className="tabular-nums text-foreground">
                    {formatCost(block.burnRateCostPerHour)}/h
                  </span>
                </div>
              )}
              <div className="flex justify-between">
                <span>{t("quick.blockEnds")}</span>
                <span className="tabular-nums text-foreground">
                  {formatTime(block.endMs)}
                </span>
              </div>
            </div>
          </div>
        ) : (
          <div className="mt-3 text-sm text-muted-foreground">
            {t("quick.noActiveBlock")}
          </div>
        )}
      </div>

      {/* Footer */}
      <button
        onClick={() => invoke("show_main_window")}
        className="mt-3 flex w-full items-center justify-center gap-2 rounded-lg bg-primary py-2 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
      >
        <LayoutDashboard className="h-4 w-4" />
        {t("quick.openDashboard")}
      </button>
    </div>
  );
}

function MiniStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl bg-card p-3">
      <div className="truncate text-[10px] text-muted-foreground">{label}</div>
      <div className="mt-0.5 truncate text-sm font-semibold tabular-nums">
        {value}
      </div>
    </div>
  );
}
