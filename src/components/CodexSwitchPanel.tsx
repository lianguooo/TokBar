import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  Check,
  Download,
  LoaderCircle,
  Pencil,
  Plus,
  Trash2,
  UserPlus,
} from "lucide-react";
import {
  api,
  type CodexAccount,
  type CodexProvider,
  type CodexSwitchResult,
  type CodexSwitchState,
} from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

/** While an account waits for its sign-in, poll so the panel adopts it
 *  without the user having to come back and refresh. */
const PENDING_POLL_MS = 4000;

type Form =
  | {
      kind: "provider";
      id?: string;
      name: string;
      baseUrl: string;
      bearerToken: string;
      model: string;
    }
  | { kind: "account"; id: string; name: string; model: string }
  | {
      kind: "addAccount";
      name: string;
      currentAccountName: string;
      model: string;
    }
  | { kind: "captureAccount"; name: string; model: string };

const emptyProviderForm: Form = {
  kind: "provider",
  name: "",
  baseUrl: "",
  bearerToken: "",
  model: "",
};

/** Content only: the caller supplies the surrounding card, so the switcher and
 *  its enable toggle read as one block. */
export function CodexSwitchPanel() {
  const { t } = useI18n();
  const [state, setState] = useState<CodexSwitchState | null>(null);
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(
    null,
  );
  const [form, setForm] = useState<Form | null>(null);
  // Two-step delete, same pattern as the subscription rows in Settings.
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const disarmTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const load = useCallback(async () => {
    try {
      const next = await api.codexSwitchState();
      setState(next);
      setLoadError("");
      if (next.capturedAccount) {
        setMessage({
          ok: true,
          text: t("codexSwitch.captured", { name: next.capturedAccount.name }),
        });
      }
    } catch (error) {
      setLoadError(String(error));
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  // Only poll while a sign-in is outstanding; otherwise the panel is idle.
  useEffect(() => {
    if (!state?.pendingAccount) return;
    const timer = setInterval(() => {
      if (!busy) load();
    }, PENDING_POLL_MS);
    return () => clearInterval(timer);
  }, [state?.pendingAccount, busy, load]);

  useEffect(
    () => () => {
      if (disarmTimer.current) clearTimeout(disarmTimer.current);
    },
    [],
  );

  const armDelete = (id: string) => {
    setConfirmDelete(id);
    if (disarmTimer.current) clearTimeout(disarmTimer.current);
    disarmTimer.current = setTimeout(() => setConfirmDelete(null), 3000);
  };

  /** Every mutation returns the fresh state, so one helper covers them all. */
  const run = async (action: () => Promise<CodexSwitchResult>) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await action();
      setState(result.state);
      setLoadError("");
      setForm(null);
      setConfirmDelete(null);
      setMessage({
        ok: true,
        text: result.changed ? result.message : t("codexSwitch.noChange"),
      });
    } catch (error) {
      setMessage({ ok: false, text: String(error) });
    } finally {
      setBusy(false);
    }
  };

  const submitForm = () => {
    if (!form) return;
    if (form.kind === "provider") {
      const payload = {
        name: form.name,
        baseUrl: form.baseUrl,
        bearerToken: form.bearerToken,
        model: form.model,
      };
      return run(() =>
        form.id
          ? api.codexProviderUpdate({ id: form.id, ...payload })
          : api.codexProviderCreate(payload),
      );
    }
    if (form.kind === "account") {
      return run(() =>
        api.codexAccountUpdate({
          id: form.id,
          name: form.name,
          model: form.model,
        }),
      );
    }
    if (form.kind === "captureAccount") {
      return run(() => api.codexAccountCapture(form.name, form.model));
    }
    return run(() =>
      api.codexAccountAdd({
        name: form.name,
        currentAccountName: form.currentAccountName,
        model: form.model,
      }),
    );
  };

  if (loadError && !state) {
    return (
      <p className="text-xs text-destructive">
        {t("codexSwitch.loadFailed")}: {loadError}
      </p>
    );
  }

  if (!state) {
    return (
      <p className="text-xs text-muted-foreground">
        {t("codexSwitch.working")}
      </p>
    );
  }

  const isEmpty = state.accounts.length === 0 && state.providers.length === 0;

  return (
    <div className="space-y-4">
      <div className="space-y-1">
        <p className="font-mono text-xs text-muted-foreground">
          {t("codexSwitch.home", { path: state.codexHome })}
        </p>
        <p className="text-xs text-muted-foreground">
          {t("codexSwitch.restartHint")}
        </p>
      </div>

      {/* Offered only while the other store holds something we do not:
          importing reads credential files from another app, so it stays an
          explicit action rather than anything automatic. */}
      {state.importableAccounts > 0 && (
        <div className="flex items-start gap-3 rounded-lg border border-border p-3">
          <Download className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
          <div className="flex-1 space-y-2">
            <p className="text-xs text-muted-foreground">
              {t("codexSwitch.importHint", { n: state.importableAccounts })}
            </p>
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => run(() => api.codexImportAccounts())}
            >
              {t("codexSwitch.import")}
            </Button>
          </div>
        </div>
      )}

      {state.requiresCurrentAccountName && form?.kind !== "captureAccount" && (
        <div className="flex items-start gap-3 rounded-lg border border-amber-500/40 bg-amber-500/5 p-3">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-500" />
          <div className="flex-1 space-y-2">
            <p className="text-xs text-muted-foreground">
              {t("codexSwitch.saveCurrentDesc")}
            </p>
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() =>
                setForm({ kind: "captureAccount", name: "", model: "" })
              }
            >
              {t("codexSwitch.saveCurrent")}
            </Button>
          </div>
        </div>
      )}

      {/* Official ChatGPT: the way back when no archived account matches. */}
      <Row
        title={t("codexSwitch.official")}
        subtitle={t("codexSwitch.officialDesc")}
        badges={
          state.officialMode
            ? [{ label: t("codexSwitch.current"), variant: "success" as const }]
            : []
        }
        actions={
          <Button
            size="sm"
            variant="outline"
            disabled={busy || state.officialMode}
            onClick={() => run(() => api.codexSwitchOfficial())}
          >
            {t("codexSwitch.use")}
          </Button>
        }
      />

      <Section title={t("codexSwitch.accounts")}>
        {state.accounts.map((account) => (
          <AccountRow
            key={account.id}
            account={account}
            state={state}
            busy={busy}
            confirmDelete={confirmDelete}
            onUse={() =>
              run(() => api.codexSwitchSelect("account", account.id))
            }
            onEdit={() =>
              setForm({
                kind: "account",
                id: account.id,
                name: account.name,
                model: account.model,
              })
            }
            onDelete={() =>
              confirmDelete === account.id
                ? run(() => api.codexAccountDelete(account.id))
                : armDelete(account.id)
            }
          />
        ))}

        {state.pendingAccount && (
          <Row
            title={t("codexSwitch.pending", {
              name: state.pendingAccount.name,
            })}
            subtitle={t("codexSwitch.pendingHint", {
              name: state.pendingAccount.name,
            })}
            badges={[
              { label: t("codexSwitch.working"), variant: "warning" as const },
            ]}
            actions={
              <Button
                size="sm"
                variant="ghost"
                disabled={busy}
                onClick={() =>
                  run(() => api.codexAccountDelete(state.pendingAccount!.id))
                }
              >
                {t("codexSwitch.cancel")}
              </Button>
            }
          />
        )}

        <Button
          size="sm"
          variant="ghost"
          className="w-full justify-start"
          disabled={busy || !!state.pendingAccount}
          onClick={() =>
            setForm({
              kind: "addAccount",
              name: "",
              currentAccountName: "",
              model: "",
            })
          }
        >
          <UserPlus className="h-3.5 w-3.5" />
          {t("codexSwitch.addAccount")}
        </Button>
      </Section>

      <Section title={t("codexSwitch.providers")}>
        {state.providers.map((provider) => (
          <ProviderRow
            key={provider.id}
            provider={provider}
            current={state.currentProvider === provider.id}
            busy={busy}
            confirmDelete={confirmDelete}
            currentLabel={t("codexSwitch.current")}
            useLabel={t("codexSwitch.use")}
            editLabel={t("codexSwitch.edit")}
            deleteLabel={t("codexSwitch.delete")}
            confirmLabel={t("codexSwitch.confirmDelete")}
            onUse={() =>
              run(() => api.codexSwitchSelect("provider", provider.id))
            }
            onEdit={() =>
              setForm({
                kind: "provider",
                id: provider.id,
                name: provider.name,
                baseUrl: provider.baseUrl,
                bearerToken: provider.experimentalBearerToken,
                model: provider.model,
              })
            }
            onDelete={() =>
              confirmDelete === provider.id
                ? run(() => api.codexProviderDelete(provider.id))
                : armDelete(provider.id)
            }
          />
        ))}

        <Button
          size="sm"
          variant="ghost"
          className="w-full justify-start"
          disabled={busy}
          onClick={() => setForm({ ...emptyProviderForm })}
        >
          <Plus className="h-3.5 w-3.5" />
          {t("codexSwitch.addProvider")}
        </Button>
      </Section>

      {isEmpty && !state.pendingAccount && (
        <p className="text-xs text-muted-foreground">
          {t("codexSwitch.empty")}
        </p>
      )}

      {form && (
        <FormPanel
          form={form}
          busy={busy}
          onChange={setForm}
          onCancel={() => setForm(null)}
          onSubmit={submitForm}
        />
      )}

      {message && (
        <p
          className={cn(
            "flex items-center gap-1.5 text-xs",
            message.ok ? "text-emerald-500" : "text-destructive",
          )}
        >
          {message.ok ? (
            <Check className="h-3.5 w-3.5" />
          ) : (
            <AlertTriangle className="h-3.5 w-3.5" />
          )}
          {message.text}
        </p>
      )}
    </div>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
        {title}
      </div>
      {children}
    </div>
  );
}

function Row({
  title,
  subtitle,
  badges,
  actions,
}: {
  title: string;
  subtitle?: string;
  badges: { label: string; variant: "success" | "warning" | "info" }[];
  actions: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-border p-3">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-medium">{title}</span>
          {badges.map((badge) => (
            <Badge key={badge.label} variant={badge.variant}>
              {badge.label}
            </Badge>
          ))}
        </div>
        {subtitle && (
          <div className="mt-0.5 truncate text-xs text-muted-foreground">
            {subtitle}
          </div>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-1">{actions}</div>
    </div>
  );
}

function AccountRow({
  account,
  state,
  busy,
  confirmDelete,
  onUse,
  onEdit,
  onDelete,
}: {
  account: CodexAccount;
  state: CodexSwitchState;
  busy: boolean;
  confirmDelete: string | null;
  onUse: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  // The distinction that matters: `currentAccountId` is the switch target and
  // is only set in official mode, while `liveAccountId` is who is signed in and
  // survives a provider switch. Disabling on the latter is what made the
  // account unreachable in the upstream app.
  const isCurrent = state.currentAccountId === account.id;
  const isSignedIn = state.liveAccountId === account.id;
  const badges: { label: string; variant: "success" | "warning" | "info" }[] =
    [];
  if (isCurrent)
    badges.push({ label: t("codexSwitch.current"), variant: "success" });
  else if (isSignedIn)
    badges.push({ label: t("codexSwitch.signedIn"), variant: "info" });

  return (
    <Row
      title={account.name}
      subtitle={
        isSignedIn && !isCurrent
          ? t("codexSwitch.signedInHint")
          : account.model || undefined
      }
      badges={badges}
      actions={
        <>
          <Button
            size="sm"
            variant="outline"
            disabled={busy || isCurrent}
            onClick={onUse}
          >
            {t("codexSwitch.use")}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={onEdit}
            title={t("codexSwitch.edit")}
          >
            <Pencil className="h-3.5 w-3.5" />
          </Button>
          <Button
            size="sm"
            variant="ghost"
            // The signed-in account's archive is the only copy of that login.
            disabled={busy || isSignedIn}
            onClick={onDelete}
            className={cn(confirmDelete === account.id && "text-destructive")}
            title={t("codexSwitch.delete")}
          >
            {confirmDelete === account.id ? (
              <span className="text-xs">{t("codexSwitch.confirmDelete")}</span>
            ) : (
              <Trash2 className="h-3.5 w-3.5" />
            )}
          </Button>
        </>
      }
    />
  );
}

function ProviderRow({
  provider,
  current,
  busy,
  confirmDelete,
  currentLabel,
  useLabel,
  editLabel,
  deleteLabel,
  confirmLabel,
  onUse,
  onEdit,
  onDelete,
}: {
  provider: CodexProvider;
  current: boolean;
  busy: boolean;
  confirmDelete: string | null;
  currentLabel: string;
  useLabel: string;
  editLabel: string;
  deleteLabel: string;
  confirmLabel: string;
  onUse: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  return (
    <Row
      title={provider.name}
      subtitle={provider.baseUrl}
      badges={
        current ? [{ label: currentLabel, variant: "success" as const }] : []
      }
      actions={
        <>
          <Button
            size="sm"
            variant="outline"
            disabled={busy || current}
            onClick={onUse}
          >
            {useLabel}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={onEdit}
            title={editLabel}
          >
            <Pencil className="h-3.5 w-3.5" />
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy || current}
            onClick={onDelete}
            className={cn(confirmDelete === provider.id && "text-destructive")}
            title={deleteLabel}
          >
            {confirmDelete === provider.id ? (
              <span className="text-xs">{confirmLabel}</span>
            ) : (
              <Trash2 className="h-3.5 w-3.5" />
            )}
          </Button>
        </>
      }
    />
  );
}

function FormPanel({
  form,
  busy,
  onChange,
  onCancel,
  onSubmit,
}: {
  form: Form;
  busy: boolean;
  onChange: (form: Form) => void;
  onCancel: () => void;
  onSubmit: () => void;
}) {
  const { t } = useI18n();
  const field =
    "w-full rounded-lg border border-border bg-transparent px-3 py-2 text-sm outline-none focus:border-primary";

  return (
    <form
      className="space-y-3 rounded-lg border border-border p-3"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit();
      }}
    >
      {form.kind === "addAccount" && (
        <div className="flex items-start gap-2 rounded-lg bg-amber-500/10 p-2.5">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-500" />
          <p className="text-xs text-muted-foreground">
            {t("codexSwitch.addAccountWarning")}
          </p>
        </div>
      )}

      <label className="block space-y-1">
        <span className="text-xs text-muted-foreground">
          {t("codexSwitch.name")}
        </span>
        <input
          className={field}
          value={form.name}
          autoFocus
          onChange={(event) => onChange({ ...form, name: event.target.value })}
        />
      </label>

      {form.kind === "addAccount" && (
        <label className="block space-y-1">
          <span className="text-xs text-muted-foreground">
            {t("codexSwitch.currentAccountName")}
          </span>
          <input
            className={field}
            value={form.currentAccountName}
            onChange={(event) =>
              onChange({ ...form, currentAccountName: event.target.value })
            }
          />
        </label>
      )}

      {form.kind === "provider" && (
        <>
          <label className="block space-y-1">
            <span className="text-xs text-muted-foreground">
              {t("codexSwitch.baseUrl")}
            </span>
            <input
              className={field}
              placeholder="https://api.example.com/v1"
              value={form.baseUrl}
              onChange={(event) =>
                onChange({ ...form, baseUrl: event.target.value })
              }
            />
          </label>
          <label className="block space-y-1">
            <span className="text-xs text-muted-foreground">
              {t("codexSwitch.token")}
            </span>
            <input
              className={field}
              type="password"
              value={form.bearerToken}
              onChange={(event) =>
                onChange({ ...form, bearerToken: event.target.value })
              }
            />
          </label>
        </>
      )}

      <label className="block space-y-1">
        <span className="text-xs text-muted-foreground">
          {t("codexSwitch.model")}
        </span>
        <input
          className={field}
          value={form.model}
          onChange={(event) => onChange({ ...form, model: event.target.value })}
        />
        <span className="block text-xs text-muted-foreground">
          {t("codexSwitch.modelHint")}
        </span>
      </label>

      <div className="flex justify-end gap-2">
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={onCancel}
          disabled={busy}
        >
          {t("codexSwitch.cancel")}
        </Button>
        <Button type="submit" size="sm" disabled={busy || !form.name.trim()}>
          {busy && <LoaderCircle className="h-3.5 w-3.5 animate-spin" />}
          {t("codexSwitch.save")}
        </Button>
      </div>
    </form>
  );
}
