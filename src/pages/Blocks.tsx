import { useEffect, useState } from "react";
import { Flame, Hourglass } from "lucide-react";
import { api, type Block, type CostMode } from "@/lib/api";
import {
  formatCost,
  formatDateTime,
  formatNumber,
  formatTime,
  formatTokens,
  shortModelName,
} from "@/lib/format";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { LoadError } from "@/components/LoadError";
import { useI18n } from "@/lib/i18n";

export function BlocksPage({
  costMode,
  refreshKey,
}: {
  costMode: CostMode;
  refreshKey: number;
}) {
  const { t } = useI18n();
  const [blocks, setBlocks] = useState<Block[] | null>(null);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setError(false);
    // Show roughly the last two weeks of 5-hour billing blocks.
    const sinceMs = Date.now() - 14 * 86_400_000;
    api
      .getBlocks({ sinceMs, costMode })
      .then((b) => !cancelled && setBlocks(b))
      .catch(() => !cancelled && setError(true));
    return () => {
      cancelled = true;
    };
  }, [costMode, refreshKey, attempt]);

  if (error) {
    return <LoadError onRetry={() => setAttempt((a) => a + 1)} />;
  }
  if (!blocks) {
    return <Skeleton className="h-80" />;
  }

  const usage = blocks.filter((b) => !b.isGap);

  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground">{t("blocks.desc")}</p>
      {usage.length === 0 && (
        <Card>
          <CardContent className="flex h-32 items-center justify-center text-sm text-muted-foreground">
            {t("blocks.empty")}
          </CardContent>
        </Card>
      )}
      {usage.map((b) => (
        <Card
          key={b.id}
          className={b.isActive ? "border-primary/50" : undefined}
        >
          <CardContent className="flex flex-wrap items-center gap-x-6 gap-y-2 p-4">
            <div className="flex min-w-44 items-center gap-2">
              {b.isActive ? (
                <Flame className="h-4 w-4 text-primary" />
              ) : (
                <Hourglass className="h-4 w-4 text-muted-foreground" />
              )}
              <div>
                <div className="text-sm font-medium">
                  {formatDateTime(b.startMs)} – {formatTime(b.endMs)}
                </div>
                <div className="text-xs text-muted-foreground">
                  {b.isActive ? remainingLabel(b.endMs, t) : t("blocks.completed")}
                </div>
              </div>
            </div>
            {b.isActive && <Badge variant="success">{t("blocks.live")}</Badge>}
            <Metric label={t("blocks.cost")} value={formatCost(b.cost)} />
            <Metric label={t("blocks.tokens")} value={formatTokens(b.totalTokens)} />
            <Metric label={t("blocks.requests")} value={formatNumber(b.requests)} />
            {b.burnRateTpm != null && (
              <Metric
                label={t("blocks.burnRate")}
                value={`${formatTokens(Math.round(b.burnRateTpm))}${t("unit.perMin")}`}
              />
            )}
            {b.burnRateCostPerHour != null && (
              <Metric
                label={t("blocks.costRate")}
                value={`${formatCost(b.burnRateCostPerHour)}${t("unit.perHour")}`}
              />
            )}
            <div className="ml-auto flex flex-wrap gap-1">
              {b.models.slice(0, 4).map((m) => (
                <Badge key={m} variant="outline">
                  {shortModelName(m)}
                </Badge>
              ))}
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

/** "还剩 2 小时 5 分" — the live badge already says "active", so the
 *  subtitle carries the genuinely useful fact: time left in the window. */
function remainingLabel(
  endMs: number,
  t: ReturnType<typeof useI18n>["t"],
): string {
  const mins = Math.max(0, Math.round((endMs - Date.now()) / 60_000));
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return h > 0
    ? t("blocks.remainHM", { h, m })
    : t("blocks.remainM", { m });
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="text-sm font-medium tabular-nums">{value}</div>
    </div>
  );
}
