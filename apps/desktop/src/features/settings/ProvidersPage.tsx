import { CheckCircle2, Cloud, HardDrive, KeyRound, Pencil, Plus, Trash2, Zap } from "lucide-react";
import { useState } from "react";
import type { ProviderConfig, ProviderKind, ProviderKindInfo } from "@dolphin/shared";
import { Badge, Button, Dialog, EmptyState, Field, Input, Switch } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";
import { PageHeader } from "./SettingsView";

const HINTS: Partial<Record<ProviderKind, string>> = {
  ollama: "Runs models locally. Install from ollama.com and pull a thinking model, e.g. `ollama pull qwen3.5:2b`.",
  lm_studio: "Start the local server in LM Studio (Developer → Start server).",
  custom_openai: "Any OpenAI-compatible server (vLLM, llama.cpp, LocalAI…).",
  openrouter: "One key for hundreds of models, including free ones.",
  azure_openai: "Use the v1 endpoint of your resource; enter deployment names as model ids.",
};

export function ProvidersPage() {
  const providers = useApp((s) => s.providers);
  const kinds = useApp((s) => s.kinds);
  const [editing, setEditing] = useState<ProviderConfig | null>(null);
  const [adding, setAdding] = useState(false);

  const startNew = (k: ProviderKindInfo) => {
    setAdding(false);
    setEditing({ id: "", kind: k.kind, name: k.label, baseUrl: k.defaultBaseUrl, extra: {}, enabled: true, hasKey: false, updatedAt: 0 });
  };

  return (
    <>
      <PageHeader
        title="Providers"
        description="Where your agents' models run. Local providers keep everything on this machine; cloud keys are stored in your system keychain, never in the database."
        actions={<Button variant="primary" onClick={() => setAdding(true)}><Plus className="size-4" />Add provider</Button>}
      />
      {providers.length === 0 ? (
        <div className="rounded-lg border border-dashed border-line-strong">
          <EmptyState icon={<Zap />} title="Connect your first provider">
            Start with Ollama or LM Studio for local models, or OpenRouter for cloud models with a single key.
          </EmptyState>
          <div className="flex justify-center gap-2 pb-8">
            {kinds.filter((k) => ["ollama", "lm_studio", "openrouter"].includes(k.kind)).map((k) => (
              <Button key={k.kind} onClick={() => startNew(k)}>
                {k.isLocal ? <HardDrive className="size-4" /> : <Cloud className="size-4" />}
                {k.label}
              </Button>
            ))}
          </div>
        </div>
      ) : (
        <div className="space-y-2">
          {providers.map((p) => (
            <ProviderRow key={p.id} p={p} kind={kinds.find((k) => k.kind === p.kind)} onEdit={() => setEditing(p)} />
          ))}
        </div>
      )}

      <Dialog open={adding} onOpenChange={setAdding} title="Add a provider" width={640}>
        {(["local", "cloud"] as const).map((group) => (
          <div key={group} className="mb-4">
            <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-faint">{group === "local" ? "Local" : "Cloud"}</div>
            <div className="grid grid-cols-3 gap-2">
              {kinds.filter((k) => (group === "local") === k.isLocal).map((k) => (
                <button key={k.kind} onClick={() => startNew(k)} className="flex items-center gap-2 rounded-md border border-line bg-hover/40 px-3 py-2.5 text-left text-[13px] font-medium text-fg transition-ui hover:border-line-strong hover:bg-hover">
                  {k.isLocal ? <HardDrive className="size-4 text-muted" /> : <Cloud className="size-4 text-muted" />}
                  {k.label}
                </button>
              ))}
            </div>
          </div>
        ))}
      </Dialog>
      {editing && <ProviderEditor initial={editing} kind={kinds.find((k) => k.kind === editing.kind)} onClose={() => setEditing(null)} />}
    </>
  );
}

function ProviderRow({ p, kind, onEdit }: { p: ProviderConfig; kind?: ProviderKindInfo; onEdit: () => void }) {
  const api = useApp((s) => s.api);
  const refresh = useApp((s) => s.refreshProviders);
  const run = useApp((s) => s.run);
  const [test, setTest] = useState<{ ok: boolean; text: string } | null>(null);
  const [testing, setTesting] = useState(false);
  return (
    <div className="rounded-lg border border-line bg-surface-0 px-4 py-3">
      <div className="flex items-center gap-3">
        <div className="flex size-9 items-center justify-center rounded-md border border-line bg-hover">
          {kind?.isLocal ? <HardDrive className="size-4 text-muted" /> : <Cloud className="size-4 text-muted" />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-[13.5px] font-semibold text-fg">{p.name}</span>
            <Badge>{kind?.label ?? p.kind}</Badge>
            {kind?.needsKey && (p.hasKey ? <Badge tone="success"><KeyRound className="size-3" />key saved</Badge> : <Badge tone="warning"><KeyRound className="size-3" />no key</Badge>)}
          </div>
          <div className="truncate font-mono text-[11.5px] text-faint">{p.baseUrl}</div>
        </div>
        <Button
          size="sm"
          loading={testing}
          onClick={async () => {
            if (!api) return;
            setTesting(true);
            try {
              const models = await api.listModels(p.id);
              const reasoning = models.filter((m) => m.reasoning).length;
              setTest({ ok: true, text: `Connected · ${models.length} models, ${reasoning} reasoning` });
            } catch (e) {
              setTest({ ok: false, text: errorText(e) });
            } finally {
              setTesting(false);
            }
          }}
        >
          Test
        </Button>
        <Button size="sm" variant="ghost" onClick={onEdit}><Pencil className="size-3.5" /></Button>
        <Switch checked={p.enabled} onChange={async (enabled) => api && (await run(api.saveProvider({ ...p, enabled }, null))) && refresh()} label="Enabled" />
      </div>
      {test && (
        <div className={cn("mt-2 flex items-start gap-1.5 text-[12px]", test.ok ? "text-success" : "text-danger")}>
          {test.ok && <CheckCircle2 className="size-3.5 mt-px" />}
          <span className="selectable">{test.text}</span>
        </div>
      )}
    </div>
  );
}

function ProviderEditor({ initial, kind, onClose }: { initial: ProviderConfig; kind?: ProviderKindInfo; onClose: () => void }) {
  const api = useApp((s) => s.api);
  const refresh = useApp((s) => s.refreshProviders);
  const toast = useApp((s) => s.toast);
  const [p, setP] = useState(initial);
  const [key, setKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const isNew = !initial.id;

  const save = async () => {
    if (!api) return;
    setSaving(true);
    try {
      await api.saveProvider(p, key ? key : null);
      await refresh();
      toast("success", isNew ? `${p.name} added` : `${p.name} saved`);
      onClose();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={isNew ? `Add ${kind?.label ?? "provider"}` : `Edit ${initial.name}`}
      description={kind ? HINTS[kind.kind] : undefined}
      footer={
        <>
          {!isNew && (
            <Button
              variant="danger"
              className="mr-auto"
              onClick={async () => {
                if (!confirmDelete) return setConfirmDelete(true);
                await api?.deleteProvider(initial.id);
                await refresh();
                onClose();
              }}
            >
              <Trash2 className="size-3.5" />
              {confirmDelete ? "Confirm delete" : "Delete"}
            </Button>
          )}
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" loading={saving} onClick={save}>{isNew ? "Add provider" : "Save"}</Button>
        </>
      }
    >
      <div className="space-y-4">
        <Field label="Name">
          <Input value={p.name} onChange={(e) => setP({ ...p, name: e.target.value })} />
        </Field>
        <Field label="Base URL" hint={kind?.isLocal ? "Change this if the server runs on another port or machine." : undefined}>
          <Input value={p.baseUrl} onChange={(e) => setP({ ...p, baseUrl: e.target.value })} className="font-mono text-[12.5px]" />
        </Field>
        {(kind?.needsKey || p.kind === "custom_openai") && (
          <Field label="API key" hint={p.hasKey ? "A key is saved in your keychain. Leave empty to keep it." : "Stored in your system keychain."}>
            <Input type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} placeholder={p.hasKey ? "••••••••••••" : "Paste your key"} />
          </Field>
        )}
      </div>
    </Dialog>
  );
}
