import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  Bot,
  Coins,
  DollarSign,
  FolderGit2,
  MessageSquare,
} from "lucide-react";
import {
  api,
  type AgentBreakdown,
  type DailyRow,
  type ModelRow,
  type Overview as OverviewData,
  type ProjectRow,
  type QueryParams,
} from "@/lib/api";
import {
  agentColor,
  agentLabel,
  formatCost,
  formatNumber,
  formatTokens,
  shortModelName,
} from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { useSubscriptions } from "@/lib/subscriptions";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { LoadError } from "@/components/LoadError";
import { StatCard } from "@/components/StatCard";
import { RoiCard } from "@/components/RoiCard";
import { CostTrendChart, DistributionPie } from "@/components/charts";

/** Start of the current calendar month (local time). */
function startOfMonth(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  d.setDate(1);
  return d.getTime();
}

export function OverviewPage({
  params,
  refreshKey,
  hourly,
}: {
  params: QueryParams;
  refreshKey: number;
  hourly: boolean;
}) {
  const { t } = useI18n();
  const { subscriptions } = useSubscriptions();
  const [overview, setOverview] = useState<OverviewData | null>(null);
  const [daily, setDaily] = useState<DailyRow[]>([]);
  const [models, setModels] = useState<ModelRow[]>([]);
  const [projects, setProjects] = useState<ProjectRow[]>([]);
  const [monthByAgent, setMonthByAgent] = useState<AgentBreakdown[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);

  // ROI always reflects this calendar month at API prices, independent of
  // the page's range/cost-mode selector. Only fetched when there are
  // subscriptions to price against.
  useEffect(() => {
    if (subscriptions.length === 0) return;
    let cancelled = false;
    api
      .getOverview({ sinceMs: startOfMonth(), costMode: "calculate" })
      .then((o) => !cancelled && setMonthByAgent(o.byAgent))
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [subscriptions, refreshKey]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(false);
    Promise.all([
      api.getOverview(params),
      hourly ? api.getHourly(params) : api.getDaily(params),
      api.getModels(params),
      api.getProjects({ ...params, limit: 6 }),
    ])
      .then(([o, d, m, p]) => {
        if (cancelled) return;
        setOverview(o);
        setDaily(d);
        setModels(m);
        setProjects(p);
      })
      .catch(() => !cancelled && setError(true))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [params.sinceMs, params.untilMs, params.costMode, refreshKey, hourly, attempt]);

  // Stable references so DistributionPie's internal memo isn't defeated
  // by a fresh array on every unrelated re-render.
  const modelPie = useMemo(
    () =>
      models.slice(0, 6).map((m) => ({
        name: shortModelName(m.model),
        value: m.cost,
      })),
    [models],
  );
  const agentPie = useMemo(
    () =>
      (overview?.byAgent ?? []).map((a, i) => ({
        name: agentLabel(a.agent),
        value: a.cost,
        color: agentColor(a.agent, i),
      })),
    [overview],
  );

  if (error && !overview) {
    return <LoadError onRetry={() => setAttempt((a) => a + 1)} />;
  }
  if (loading && !overview) {
    return (
      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        {Array.from({ length: 8 }).map((_, i) => (
          <Skeleton key={i} className="h-28" />
        ))}
      </div>
    );
  }

  const tot = overview?.totals;

  return (
    <div className="space-y-4">
      <RoiCard monthByAgent={monthByAgent} />

      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        <StatCard
          title={t("overview.totalCost")}
          value={formatCost(tot?.cost ?? 0)}
          sub={t("overview.activeDays", { n: tot?.activeDays ?? 0 })}
          icon={DollarSign}
        />
        <StatCard
          title={t("overview.totalTokens")}
          value={formatTokens(tot?.totalTokens ?? 0)}
          sub={t("overview.inOut", {
            in: formatTokens(tot?.inputTokens ?? 0),
            out: formatTokens(tot?.outputTokens ?? 0),
          })}
          icon={Coins}
        />
        <StatCard
          title={t("overview.requests")}
          value={formatNumber(tot?.requests ?? 0)}
          sub={t("overview.cacheRead", {
            n: formatTokens(tot?.cacheReadTokens ?? 0),
          })}
          icon={MessageSquare}
        />
        <StatCard
          title={t("overview.sessions")}
          value={formatNumber(tot?.sessions ?? 0)}
          sub={t("overview.agents", { n: overview?.byAgent.length ?? 0 })}
          icon={Activity}
        />
      </div>

      <Card>
        <CardHeader>
          <CardTitle>{t("overview.costTrend")}</CardTitle>
        </CardHeader>
        <CardContent>
          {daily.length > 0 ? (
            <CostTrendChart rows={daily} />
          ) : (
            <EmptyHint />
          )}
        </CardContent>
      </Card>

      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader className="flex-row items-center justify-between space-y-0">
            <CardTitle>{t("overview.costByModel")}</CardTitle>
            <Bot className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {modelPie.length > 0 ? (
              <DistributionPie data={modelPie} valueFormatter={formatCost} />
            ) : (
              <EmptyHint />
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="flex-row items-center justify-between space-y-0">
            <CardTitle>{t("overview.costByAgent")}</CardTitle>
            <Bot className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {agentPie.length > 0 ? (
              <DistributionPie data={agentPie} valueFormatter={formatCost} />
            ) : (
              <EmptyHint />
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader className="flex-row items-center justify-between space-y-0">
          <CardTitle>{t("overview.topProjects")}</CardTitle>
          <FolderGit2 className="h-4 w-4 text-muted-foreground" />
        </CardHeader>
        <CardContent className="space-y-3">
          {projects.length === 0 && <EmptyHint />}
          {projects.map((p) => {
            const max = projects[0]?.cost || 1;
            return (
              <div key={p.project} className="space-y-1">
                <div className="flex items-center justify-between text-sm">
                  <span className="truncate font-medium">{p.project}</span>
                  <span className="ml-3 shrink-0 tabular-nums text-muted-foreground">
                    {t("overview.projectStats", {
                      cost: formatCost(p.cost),
                      tokens: formatTokens(p.totalTokens),
                      sessions: p.sessions,
                    })}
                  </span>
                </div>
                <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                  <div
                    className="h-full rounded-full bg-primary"
                    style={{ width: `${Math.max(2, (p.cost / max) * 100)}%` }}
                  />
                </div>
              </div>
            );
          })}
        </CardContent>
      </Card>
    </div>
  );
}

function EmptyHint() {
  const { t } = useI18n();
  return (
    <div className="flex h-40 items-center justify-center text-sm text-muted-foreground">
      {t("common.empty")}
    </div>
  );
}
