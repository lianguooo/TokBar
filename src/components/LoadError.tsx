import { AlertTriangle } from "lucide-react";
import { useI18n } from "@/lib/i18n";
import { Button } from "@/components/ui/button";

/** Full-area error state with a retry button — shown when a page's
 *  backend query rejects instead of leaving the skeleton up forever. */
export function LoadError({ onRetry }: { onRetry: () => void }) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col items-center justify-center gap-3 py-16 text-muted-foreground">
      <AlertTriangle className="h-6 w-6" />
      <span className="text-sm">{t("common.loadFailed")}</span>
      <Button variant="outline" size="sm" onClick={onRetry}>
        {t("common.retry")}
      </Button>
    </div>
  );
}
