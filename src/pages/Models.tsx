import { useEffect, useState } from "react";
import { api, type ModelRow, type QueryParams } from "@/lib/api";
import {
  formatCost,
  formatNumber,
  formatTokens,
  shortModelName,
} from "@/lib/format";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { DistributionPie } from "@/components/charts";
import { useI18n } from "@/lib/i18n";

export function ModelsPage({
  params,
  refreshKey,
}: {
  params: QueryParams;
  refreshKey: number;
}) {
  const { t } = useI18n();
  const [models, setModels] = useState<ModelRow[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    api.getModels(params).then((m) => !cancelled && setModels(m));
    return () => {
      cancelled = true;
    };
  }, [params.sinceMs, params.untilMs, params.costMode, refreshKey]);

  if (!models) {
    return <Skeleton className="h-80" />;
  }

  const costPie = models.slice(0, 8).map((m) => ({
    name: shortModelName(m.model),
    value: m.cost,
  }));
  const tokenPie = models.slice(0, 8).map((m) => ({
    name: shortModelName(m.model),
    value: m.totalTokens,
  }));

  return (
    <div className="space-y-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>{t("models.costDist")}</CardTitle>
          </CardHeader>
          <CardContent>
            <DistributionPie data={costPie} valueFormatter={formatCost} />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>{t("models.tokenDist")}</CardTitle>
          </CardHeader>
          <CardContent>
            <DistributionPie data={tokenPie} valueFormatter={formatTokens} />
          </CardContent>
        </Card>
      </div>
      <Card>
        <CardHeader>
          <CardTitle>{t("models.all")}</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t("th.model")}</TableHead>
                <TableHead className="text-right">{t("th.input")}</TableHead>
                <TableHead className="text-right">{t("th.output")}</TableHead>
                <TableHead className="text-right">{t("th.cacheWrite")}</TableHead>
                <TableHead className="text-right">{t("th.cacheRead")}</TableHead>
                <TableHead className="text-right">{t("th.requests")}</TableHead>
                <TableHead className="text-right">{t("th.cost")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {models.map((m) => (
                <TableRow key={m.model}>
                  <TableCell className="font-medium">{m.model}</TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatTokens(m.inputTokens)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatTokens(m.outputTokens)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatTokens(m.cacheCreationTokens)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatTokens(m.cacheReadTokens)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatNumber(m.requests)}
                  </TableCell>
                  <TableCell className="text-right font-medium tabular-nums">
                    {formatCost(m.cost)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  );
}
