import { useMemo } from "react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { DailyRow } from "@/lib/api";
import {
  agentColor,
  agentLabel,
  CHART_PALETTE,
  formatCost,
  formatTokens,
} from "@/lib/format";
import { useI18n } from "@/lib/i18n";

const AXIS_STYLE = { fontSize: 11, fill: "var(--muted-foreground)" };
const GRID_STROKE = "var(--border)";
const TOOLTIP_STYLE = {
  backgroundColor: "var(--card)",
  border: "1px solid var(--border)",
  borderRadius: 8,
  fontSize: 12,
};

/**
 * Aggregation buckets only exist where there was usage; quiet hours/days
 * are absent, which breaks lines in the chart. Fill the timeline so every
 * bucket between the first and last is present (zeros for quiet periods).
 */
function fillTimeline(labels: string[]): string[] {
  if (labels.length === 0) return labels;
  const sorted = [...labels].sort();
  const first = sorted[0];
  const last = sorted[sorted.length - 1];
  // Hourly ("HH:00"): from midnight to the last active hour.
  if (/^\d{2}:00$/.test(first)) {
    const lastHour = parseInt(last, 10);
    return Array.from(
      { length: lastHour + 1 },
      (_, h) => `${String(h).padStart(2, "0")}:00`,
    );
  }
  // Monthly ("YYYY-MM").
  if (/^\d{4}-\d{2}$/.test(first)) {
    const out: string[] = [];
    let [y, m] = first.split("-").map(Number);
    while (out.length < 1200) {
      const label = `${y}-${String(m).padStart(2, "0")}`;
      out.push(label);
      if (label === last) break;
      m += 1;
      if (m > 12) {
        m = 1;
        y += 1;
      }
    }
    return out;
  }
  // Daily / weekly ("YYYY-MM-DD"): weekly buckets are 7 days apart.
  if (/^\d{4}-\d{2}-\d{2}$/.test(first)) {
    const DAY = 86_400_000;
    const times = sorted.map((s) => new Date(`${s}T12:00:00`).getTime());
    const weekly =
      times.length > 1 &&
      times.every(
        (t, i) => i === 0 || Math.round((t - times[i - 1]) / DAY) % 7 === 0,
      );
    const step = (weekly ? 7 : 1) * DAY;
    const out: string[] = [];
    for (
      let t = times[0];
      t <= times[times.length - 1] && out.length < 5000;
      t += step
    ) {
      const d = new Date(t);
      out.push(
        `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`,
      );
    }
    return out;
  }
  return sorted;
}

/** Pivot daily (date, agent) rows into one row per date with per-agent keys. */
export function pivotDailyByAgent(
  rows: DailyRow[],
  metric: "cost" | "totalTokens" | "requests",
): { data: Record<string, number | string>[]; agents: string[] } {
  const agents = [...new Set(rows.map((r) => r.agent))].sort();
  const byDate = new Map<string, Record<string, number | string>>();
  for (const r of rows) {
    const row = byDate.get(r.date) ?? { date: r.date };
    row[r.agent] = ((row[r.agent] as number) ?? 0) + r[metric];
    byDate.set(r.date, row);
  }
  const data = fillTimeline([...byDate.keys()]).map((label) => {
    const row = byDate.get(label) ?? { date: label };
    for (const agent of agents) {
      row[agent] = (row[agent] as number) ?? 0;
    }
    return row;
  });
  return { data, agents };
}

function shortDate(d: string): string {
  // "YYYY-MM-DD" -> "MM-DD"; hourly buckets ("08:00") pass through as-is.
  return /^\d{4}-\d{2}-\d{2}$/.test(d) ? d.slice(5) : d;
}

export function CostTrendChart({ rows }: { rows: DailyRow[] }) {
  const { data, agents } = useMemo(() => pivotDailyByAgent(rows, "cost"), [rows]);
  return (
    <ResponsiveContainer width="100%" height={280}>
      <AreaChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" stroke={GRID_STROKE} />
        <XAxis dataKey="date" tick={AXIS_STYLE} tickFormatter={shortDate} />
        <YAxis tick={AXIS_STYLE} tickFormatter={(v) => formatCost(v)} width={70} />
        <Tooltip
          content={({ active, payload, label }) => {
            // Zero-cost agents are invisible in the stack; hide their
            // "$0.0000" rows in the tooltip too.
            const items = (payload ?? []).filter((p) => Number(p.value) > 0);
            if (!active || items.length === 0) return null;
            return (
              <div style={{ ...TOOLTIP_STYLE, padding: "8px 12px" }}>
                <div style={{ marginBottom: 4 }}>{label}</div>
                {items.map((p) => (
                  <div
                    key={String(p.dataKey)}
                    style={{
                      color: agentColor(
                        String(p.dataKey),
                        agents.indexOf(String(p.dataKey)),
                      ),
                    }}
                  >
                    {agentLabel(String(p.dataKey))} : {formatCost(Number(p.value))}
                  </div>
                ))}
              </div>
            );
          }}
        />
        <Legend formatter={(v) => agentLabel(String(v))} />
        {agents.map((agent, i) => (
          // Fill-only stacked bands: an agent's usage is its band
          // thickness. No per-series stroke or hover dot — a zero-usage
          // series would otherwise draw its line and dot on top of the
          // band below it, reading as if it had that band's value.
          <Area
            key={agent}
            type="monotone"
            dataKey={agent}
            stackId="cost"
            stroke="none"
            fill={agentColor(agent, i)}
            fillOpacity={0.45}
            activeDot={false}
          />
        ))}
      </AreaChart>
    </ResponsiveContainer>
  );
}

export function TokenTrendChart({ rows }: { rows: DailyRow[] }) {
  const { t } = useI18n();
  const data = useMemo(() => {
    const byDate = new Map<string, Record<string, number | string>>();
    for (const r of rows) {
      const row =
        byDate.get(r.date) ??
        ({ date: r.date, input: 0, output: 0, cacheRead: 0, cacheCreate: 0 } as Record<
          string,
          number | string
        >);
      row.input = (row.input as number) + r.inputTokens;
      row.output = (row.output as number) + r.outputTokens;
      row.cacheRead = (row.cacheRead as number) + r.cacheReadTokens;
      row.cacheCreate = (row.cacheCreate as number) + r.cacheCreationTokens;
      byDate.set(r.date, row);
    }
    return fillTimeline([...byDate.keys()]).map(
      (label) =>
        byDate.get(label) ?? {
          date: label,
          input: 0,
          output: 0,
          cacheRead: 0,
          cacheCreate: 0,
        },
    );
  }, [rows]);

  const series: { key: string; label: string; color: string }[] = [
    { key: "input", label: t("chart.input"), color: "var(--chart-2)" },
    { key: "output", label: t("chart.output"), color: "var(--chart-1)" },
    { key: "cacheRead", label: t("chart.cacheRead"), color: "var(--chart-3)" },
    { key: "cacheCreate", label: t("chart.cacheWrite"), color: "var(--chart-4)" },
  ];

  return (
    <ResponsiveContainer width="100%" height={280}>
      <BarChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
        <CartesianGrid strokeDasharray="3 3" stroke={GRID_STROKE} />
        <XAxis dataKey="date" tick={AXIS_STYLE} tickFormatter={shortDate} />
        <YAxis tick={AXIS_STYLE} tickFormatter={(v) => formatTokens(v)} width={70} />
        <Tooltip
          contentStyle={TOOLTIP_STYLE}
          formatter={(v, name) => [
            formatTokens(Number(v ?? 0)),
            series.find((s) => s.key === name)?.label ?? String(name),
          ]}
        />
        <Legend formatter={(v) => series.find((s) => s.key === v)?.label ?? String(v)} />
        {series.map((s) => (
          <Bar key={s.key} dataKey={s.key} stackId="tokens" fill={s.color} />
        ))}
      </BarChart>
    </ResponsiveContainer>
  );
}

interface PieDatum {
  name: string;
  value: number;
  color?: string;
}

export function DistributionPie({
  data,
  valueFormatter,
}: {
  data: PieDatum[];
  valueFormatter: (v: number) => string;
}) {
  // Colors are keyed off the caller's (unsorted) order so the same name
  // gets the same color in every pie on the page; slices and the legend
  // are then drawn largest-first. The legend payload is explicit because
  // recharts does not follow the sorted data order on its own.
  const { sorted, colorOf } = useMemo(() => {
    const colors = new Map<string, string>();
    data.forEach((d, i) =>
      colors.set(d.name, d.color ?? CHART_PALETTE[i % CHART_PALETTE.length]),
    );
    return {
      sorted: [...data].sort((a, b) => b.value - a.value),
      colorOf: (name: string) => colors.get(name) ?? CHART_PALETTE[0],
    };
  }, [data]);
  return (
    <ResponsiveContainer width="100%" height={260}>
      <PieChart>
        <Pie
          data={sorted}
          dataKey="value"
          nameKey="name"
          innerRadius={55}
          outerRadius={90}
          paddingAngle={2}
          strokeWidth={0}
        >
          {sorted.map((d) => (
            <Cell key={d.name} fill={colorOf(d.name)} />
          ))}
        </Pie>
        <Tooltip
          contentStyle={TOOLTIP_STYLE}
          formatter={(v, name) => [valueFormatter(Number(v ?? 0)), String(name)]}
        />
        <Legend
          layout="vertical"
          align="right"
          verticalAlign="middle"
          wrapperStyle={{ fontSize: 12 }}
          content={() => (
            <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
              {sorted.map((d) => (
                <li
                  key={d.name}
                  style={{
                    color: colorOf(d.name),
                    display: "flex",
                    alignItems: "center",
                    gap: 6,
                    marginBottom: 4,
                  }}
                >
                  <span
                    style={{
                      width: 10,
                      height: 10,
                      borderRadius: 2,
                      flexShrink: 0,
                      background: colorOf(d.name),
                    }}
                  />
                  {d.name}
                </li>
              ))}
            </ul>
          )}
        />
      </PieChart>
    </ResponsiveContainer>
  );
}
