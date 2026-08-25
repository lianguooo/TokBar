import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ChevronRight,
  LoaderCircle,
  Search,
  Trash2,
} from "lucide-react";
import {
  api,
  type ModelRow,
  type QueryParams,
  type SessionDeletePreview,
  type SessionRow,
} from "@/lib/api";
import {
  agentLabel,
  formatBytes,
  formatCost,
  formatDateTime,
  formatNumber,
  formatTokens,
  shortModelName,
} from "@/lib/format";
import { useFeatureFlags } from "@/lib/features";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { LoadError } from "@/components/LoadError";

const AGENT_BADGE: Record<string, "warning" | "success" | "info"> = {
  "claude-code": "warning",
  codex: "success",
  kimi: "info",
};

/** 仅调整 rollout 文件名的时间分隔符，保留真实会话 ID 用于查询。 */
function formatSessionId(sessionId: string): string {
  return sessionId.replace(
    /^(rollout-\d{4}-\d{2}-\d{2}T)(\d{2})-(\d{2})-(\d{2})/,
    "$1$2:$3:$4",
  );
}

export function SessionsPage({
  params,
  refreshKey,
}: {
  params: QueryParams;
  refreshKey: number;
}) {
  const { t } = useI18n();
  const [sessions, setSessions] = useState<SessionRow[] | null>(null);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [search, setSearch] = useState("");
  const [agentFilter, setAgentFilter] = useState<string>("all");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [detail, setDetail] = useState<Record<string, ModelRow[]>>({});
  const { flags } = useFeatureFlags();
  // Deleting a log is permanent, so the trash icon only arms a confirmation
  // row that first shows what would actually be removed.
  const [pendingDelete, setPendingDelete] = useState<PendingDelete | null>(
    null,
  );

  useEffect(() => {
    let cancelled = false;
    setError(false);
    api
      .getSessions({ ...params, limit: 300 })
      .then((s) => !cancelled && setSessions(s))
      .catch(() => !cancelled && setError(true));
    return () => {
      cancelled = true;
    };
  }, [params.sinceMs, params.untilMs, params.costMode, refreshKey, attempt]);

  const agents = useMemo(
    () => [...new Set((sessions ?? []).map((s) => s.agent))].sort(),
    [sessions],
  );

  const filtered = useMemo(() => {
    if (!sessions) return [];
    const q = search.trim().toLowerCase();
    return sessions.filter((s) => {
      if (agentFilter !== "all" && s.agent !== agentFilter) return false;
      if (!q) return true;
      return (
        s.project.toLowerCase().includes(q) ||
        s.title.toLowerCase().includes(q) ||
        s.sessionId.toLowerCase().includes(q) ||
        s.models.toLowerCase().includes(q)
      );
    });
  }, [sessions, search, agentFilter]);

  const toggleExpand = (s: SessionRow) => {
    const key = `${s.agent}:${s.sessionId}`;
    if (expanded === key) {
      setExpanded(null);
      return;
    }
    setExpanded(key);
    if (!detail[key]) {
      api
        .getSessionModels(s.agent, s.sessionId, params.costMode)
        .then((rows) => setDetail((d) => ({ ...d, [key]: rows })));
    }
  };

  const requestDelete = async (s: SessionRow) => {
    const key = `${s.agent}:${s.sessionId}`;
    setPendingDelete({ key, preview: null, error: "", busy: false });
    try {
      const preview = await api.previewSessionDelete(s.agent, s.sessionId);
      setPendingDelete((p) => (p?.key === key ? { ...p, preview } : p));
    } catch (e) {
      setPendingDelete((p) =>
        p?.key === key ? { ...p, error: String(e) } : p,
      );
    }
  };

  const confirmDelete = async (s: SessionRow) => {
    const key = `${s.agent}:${s.sessionId}`;
    setPendingDelete((p) =>
      p?.key === key ? { ...p, busy: true, error: "" } : p,
    );
    try {
      await api.deleteSession(s.agent, s.sessionId);
      setPendingDelete(null);
      // Refetch rather than splicing the row out: the delete also rewrites
      // archived totals, so the whole list is stale.
      setAttempt((a) => a + 1);
    } catch (e) {
      setPendingDelete((p) =>
        p?.key === key ? { ...p, busy: false, error: String(e) } : p,
      );
    }
  };

  if (error) {
    return <LoadError onRetry={() => setAttempt((a) => a + 1)} />;
  }
  if (!sessions) {
    return <Skeleton className="h-80" />;
  }

  return (
    <Card>
      <CardHeader className="gap-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <CardTitle>
            {t("sessions.title")}{" "}
            <span className="text-xs font-normal">({filtered.length})</span>
          </CardTitle>
          <div className="flex flex-wrap items-center gap-2">
            {/* Agent filter */}
            <div className="flex rounded-lg border border-border p-0.5">
              <button
                onClick={() => setAgentFilter("all")}
                className={cn(
                  "rounded-md px-3 py-1 text-xs font-medium transition-colors",
                  agentFilter === "all"
                    ? "bg-primary/10 text-primary"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                {t("sessions.allAgents")}
              </button>
              {agents.map((a) => (
                <button
                  key={a}
                  onClick={() => setAgentFilter(a)}
                  className={cn(
                    "rounded-md px-3 py-1 text-xs font-medium transition-colors",
                    agentFilter === a
                      ? "bg-primary/10 text-primary"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {agentLabel(a)}
                </button>
              ))}
            </div>
            {/* Search */}
            <div className="relative">
              <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
              <input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t("sessions.search")}
                className="h-8 w-56 rounded-lg border border-border bg-transparent pl-8 pr-3 text-xs outline-none transition-colors focus:border-primary"
              />
            </div>
          </div>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("sessions.retentionHint")}
        </p>
      </CardHeader>
      <CardContent className="p-0">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-8" />
              <TableHead>{t("th.project")}</TableHead>
              <TableHead>{t("th.agent")}</TableHead>
              <TableHead>{t("th.models")}</TableHead>
              <TableHead>{t("th.lastActivity")}</TableHead>
              <TableHead className="text-right">{t("th.requests")}</TableHead>
              <TableHead className="text-right">{t("th.tokens")}</TableHead>
              <TableHead className="text-right">{t("th.cost")}</TableHead>
              {flags.sessionDeleteEnabled && <TableHead className="w-10" />}
            </TableRow>
          </TableHeader>
          <TableBody>
            {filtered.map((s) => {
              const key = `${s.agent}:${s.sessionId}`;
              const isOpen = expanded === key;
              return (
                <SessionRowGroup
                  key={key}
                  session={s}
                  isOpen={isOpen}
                  detail={detail[key]}
                  onToggle={() => toggleExpand(s)}
                  canDelete={
                    flags.sessionDeleteEnabled &&
                    flags.sessionDeleteAgents.includes(s.agent)
                  }
                  showDeleteColumn={flags.sessionDeleteEnabled}
                  pendingDelete={
                    pendingDelete?.key === key ? pendingDelete : null
                  }
                  onRequestDelete={() => requestDelete(s)}
                  onConfirmDelete={() => confirmDelete(s)}
                  onCancelDelete={() => setPendingDelete(null)}
                />
              );
            })}
          </TableBody>
        </Table>
        {filtered.length === 0 && (
          <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
            {t("sessions.empty")}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

interface PendingDelete {
  key: string;
  preview: SessionDeletePreview | null;
  error: string;
  busy: boolean;
}

function SessionRowGroup({
  session: s,
  isOpen,
  detail,
  onToggle,
  canDelete,
  showDeleteColumn,
  pendingDelete,
  onRequestDelete,
  onConfirmDelete,
  onCancelDelete,
}: {
  session: SessionRow;
  isOpen: boolean;
  detail?: ModelRow[];
  onToggle: () => void;
  /** This row's log can be removed (agent supported). */
  canDelete: boolean;
  /** The table has a delete column at all, so every row needs the cell. */
  showDeleteColumn: boolean;
  pendingDelete: PendingDelete | null;
  onRequestDelete: () => void;
  onConfirmDelete: () => void;
  onCancelDelete: () => void;
}) {
  const { t } = useI18n();
  const columns = showDeleteColumn ? 9 : 8;
  return (
    <>
      <TableRow
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
        tabIndex={0}
        aria-expanded={isOpen}
        className="cursor-pointer focus-visible:bg-accent/50 focus-visible:outline-none"
      >
        <TableCell className="pr-0">
          <ChevronRight
            className={cn(
              "h-3.5 w-3.5 text-muted-foreground transition-transform",
              isOpen && "rotate-90",
            )}
          />
        </TableCell>
        <TableCell>
          <div className="max-w-56">
            <div className="truncate font-medium" title={s.title || s.project}>
              {s.title || s.project}
            </div>
            <div className="truncate font-mono text-xs text-muted-foreground">
              {s.title ? `${s.project} · ` : ""}
              {formatSessionId(s.sessionId)}
            </div>
          </div>
        </TableCell>
        <TableCell>
          <Badge variant={AGENT_BADGE[s.agent] ?? "default"}>
            {agentLabel(s.agent)}
          </Badge>
        </TableCell>
        <TableCell className="max-w-44">
          <div className="flex flex-wrap gap-1">
            {s.models
              .split(",")
              .filter(Boolean)
              .map((m) => (
                <Badge key={m} variant="outline">
                  {shortModelName(m)}
                </Badge>
              ))}
          </div>
        </TableCell>
        <TableCell className="whitespace-nowrap text-muted-foreground">
          {formatDateTime(s.lastTs)}
        </TableCell>
        <TableCell className="text-right tabular-nums">
          {formatNumber(s.requests)}
        </TableCell>
        <TableCell className="text-right tabular-nums">
          {formatTokens(s.totalTokens)}
        </TableCell>
        <TableCell className="text-right font-medium tabular-nums">
          {formatCost(s.cost)}
        </TableCell>
        {showDeleteColumn && (
          <TableCell className="pl-0 text-right">
            {canDelete && (
              <button
                // The row itself toggles the detail panel; this must not.
                onClick={(e) => {
                  e.stopPropagation();
                  onRequestDelete();
                }}
                className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
                title={t("sessions.delete")}
                aria-label={t("sessions.delete")}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            )}
          </TableCell>
        )}
      </TableRow>
      {pendingDelete && (
        <TableRow className="hover:bg-transparent">
          <TableCell colSpan={columns} className="bg-destructive/5 p-0">
            <DeleteConfirm
              pending={pendingDelete}
              onConfirm={onConfirmDelete}
              onCancel={onCancelDelete}
            />
          </TableCell>
        </TableRow>
      )}
      {isOpen && (
        <TableRow className="hover:bg-transparent">
          <TableCell colSpan={columns} className="bg-muted/30 p-0">
            {detail ? (
              <table className="w-full text-xs">
                <thead>
                  <tr className="text-muted-foreground">
                    <th className="px-10 py-2 text-left font-medium uppercase tracking-wide">
                      {t("th.model")}
                    </th>
                    <th className="px-3 py-2 text-right font-medium uppercase tracking-wide">
                      {t("th.input")}
                    </th>
                    <th className="px-3 py-2 text-right font-medium uppercase tracking-wide">
                      {t("th.output")}
                    </th>
                    <th className="px-3 py-2 text-right font-medium uppercase tracking-wide">
                      {t("th.cacheWrite")}
                    </th>
                    <th className="px-3 py-2 text-right font-medium uppercase tracking-wide">
                      {t("th.cacheRead")}
                    </th>
                    <th className="px-3 py-2 text-right font-medium uppercase tracking-wide">
                      {t("th.requests")}
                    </th>
                    <th className="px-3 py-2 text-right font-medium uppercase tracking-wide">
                      {t("th.cost")}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {detail.map((m) => (
                    <tr key={m.model} className="border-t border-border/50">
                      <td className="px-10 py-2 font-medium">
                        {shortModelName(m.model)}
                      </td>
                      <td className="px-3 py-2 text-right tabular-nums">
                        {formatTokens(m.inputTokens)}
                      </td>
                      <td className="px-3 py-2 text-right tabular-nums">
                        {formatTokens(m.outputTokens)}
                      </td>
                      <td className="px-3 py-2 text-right tabular-nums">
                        {formatTokens(m.cacheCreationTokens)}
                      </td>
                      <td className="px-3 py-2 text-right tabular-nums">
                        {formatTokens(m.cacheReadTokens)}
                      </td>
                      <td className="px-3 py-2 text-right tabular-nums">
                        {formatNumber(m.requests)}
                      </td>
                      <td className="px-3 py-2 text-right font-medium tabular-nums">
                        {formatCost(m.cost)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <div className="px-10 py-3">
                <Skeleton className="h-8" />
              </div>
            )}
          </TableCell>
        </TableRow>
      )}
    </>
  );
}

/** Inline confirmation for a permanent log deletion: states what will be
 *  removed, what survives, and why the backend may have refused. */
function DeleteConfirm({
  pending,
  onConfirm,
  onCancel,
}: {
  pending: PendingDelete;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const preview = pending.preview;
  const blocked =
    !!preview &&
    preview.files === 0 &&
    (preview.sharedFiles > 0 || preview.staleFiles > 0);

  return (
    <div className="flex flex-wrap items-center justify-between gap-3 px-10 py-3">
      <div className="min-w-0 space-y-1">
        <div className="flex items-center gap-1.5 text-sm font-medium text-destructive">
          <AlertTriangle className="h-3.5 w-3.5" />
          {t("sessions.deleteTitle")}
        </div>
        {preview && !blocked && (
          <p className="text-xs text-muted-foreground">
            {t("sessions.deleteBody", { size: formatBytes(preview.bytes) })}
          </p>
        )}
        {preview && preview.sharedFiles > 0 && (
          <p className="text-xs text-amber-500">{t("sessions.deleteShared")}</p>
        )}
        {preview && preview.staleFiles > 0 && (
          <p className="text-xs text-amber-500">{t("sessions.deleteStale")}</p>
        )}
        {pending.error && (
          <p className="text-xs text-destructive">
            {t("sessions.deleteFailed")}: {pending.error}
          </p>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <Button
          size="sm"
          variant="ghost"
          onClick={onCancel}
          disabled={pending.busy}
        >
          {t("codexSwitch.cancel")}
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="border-destructive/40 text-destructive hover:bg-destructive/10"
          onClick={onConfirm}
          disabled={pending.busy || !preview || blocked}
        >
          {pending.busy && (
            <LoaderCircle className="h-3.5 w-3.5 animate-spin" />
          )}
          {t("sessions.deleteConfirm")}
        </Button>
      </div>
    </div>
  );
}
