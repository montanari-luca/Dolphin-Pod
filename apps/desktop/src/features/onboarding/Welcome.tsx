import { motion } from "motion/react";
import { ArrowRight, Cloud, HardDrive, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import type { ModelRef, ProviderConfig, ProviderKind } from "@dolphin/shared";
import { ModelPicker } from "@/components/ModelPicker";
import { Button, Dialog, Input } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";

/** First run: connect a provider and give every agent a model, in two steps. */
export function Welcome() {
  const ready = useApp((s) => s.ready);
  const onboarded = useApp((s) => s.settings["app.onboarded"]);
  const providers = useApp((s) => s.providers);
  const agents = useApp((s) => s.agents);
  const setSetting = useApp((s) => s.setSetting);
  const [open, setOpen] = useState(false);
  const [step, setStep] = useState<1 | 2>(1);
  useEffect(() => {
    if (ready && onboarded === false && agents.every((a) => !a.model)) setOpen(true);
  }, [ready, onboarded, agents]);
  useEffect(() => {
    if (open && providers.length > 0) setStep(2);
  }, [open, providers.length]);

  const finish = async () => {
    await setSetting("app.onboarded", true);
    setOpen(false);
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && finish()} title={step === 1 ? "Welcome to Dolphin Pod" : "Give your agents a brain"} width={560}
      description={step === 1 ? "A pod of AI specialists in one chat. Roger moderates, Lincoln orchestrates, and the specialists do the work." : "Pick one reasoning model for everyone. You can tune each agent later in Settings → Agents."}>
      {step === 1 ? <ConnectStep /> : <ModelStep onDone={finish} />}
    </Dialog>
  );
}

function ConnectStep() {
  const api = useApp((s) => s.api);
  const kinds = useApp((s) => s.kinds);
  const refresh = useApp((s) => s.refreshProviders);
  const [busy, setBusy] = useState<ProviderKind | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [key, setKey] = useState("");

  const connect = async (kind: ProviderKind, apiKey: string | null) => {
    if (!api) return;
    const info = kinds.find((k) => k.kind === kind)!;
    setBusy(kind);
    setError(null);
    try {
      const cfg: ProviderConfig = { id: "", kind, name: info.label, baseUrl: info.defaultBaseUrl, extra: {}, enabled: true, hasKey: false, updatedAt: 0 };
      const saved = await api.saveProvider(cfg, apiKey);
      try {
        await api.listModels(saved.id);
      } catch (e) {
        await api.deleteProvider(saved.id);
        throw e;
      }
      await refresh();
    } catch (e) {
      setError(`${info.label}: ${errorText(e)}`);
    } finally {
      setBusy(null);
    }
  };

  const Option = ({ kind, icon, title, body }: { kind: ProviderKind; icon: React.ReactNode; title: string; body: string }) => (
    <button onClick={() => connect(kind, null)} disabled={!!busy} className="flex w-full items-center gap-3 rounded-lg border border-line bg-hover/40 px-4 py-3 text-left transition-ui hover:border-line-strong hover:bg-hover disabled:opacity-60">
      <div className="flex size-9 items-center justify-center rounded-md border border-line bg-surface-2">{icon}</div>
      <div className="flex-1">
        <div className="text-[13.5px] font-semibold text-fg">{title}</div>
        <div className="text-[12px] text-muted">{body}</div>
      </div>
      {busy === kind ? <Loader2 className="size-4 animate-spin text-faint" /> : <ArrowRight className="size-4 text-faint" />}
    </button>
  );

  return (
    <div className="space-y-2.5">
      <Option kind="ollama" icon={<HardDrive className="size-4 text-muted" />} title="Ollama (local)" body="Use models running on this computer. Free and private." />
      <Option kind="lm_studio" icon={<HardDrive className="size-4 text-muted" />} title="LM Studio (local)" body="Connect to LM Studio's local server." />
      <div className="rounded-lg border border-line bg-hover/40 px-4 py-3">
        <div className="flex items-center gap-3">
          <div className="flex size-9 items-center justify-center rounded-md border border-line bg-surface-2">
            <Cloud className="size-4 text-muted" />
          </div>
          <div className="flex-1">
            <div className="text-[13.5px] font-semibold text-fg">OpenRouter (cloud)</div>
            <div className="text-[12px] text-muted">Hundreds of models with one key, including free ones.</div>
          </div>
        </div>
        <form className="mt-2.5 flex gap-2" onSubmit={(e) => (e.preventDefault(), key.trim() && connect("openrouter", key.trim()))}>
          <Input type="password" value={key} onChange={(e) => setKey(e.target.value)} placeholder="sk-or-…" autoComplete="off" />
          <Button type="submit" loading={busy === "openrouter"} disabled={!key.trim()}>Connect</Button>
        </form>
      </div>
      {error && <p className="text-[12px] text-danger selectable">{error}</p>}
      <p className={cn("pt-1 text-[11.5px] text-faint")}>Other providers (OpenAI, Anthropic, Gemini, xAI, Mistral, DeepSeek, Groq, Together, Fireworks, Azure) are in Settings → Providers.</p>
    </div>
  );
}

function ModelStep({ onDone }: { onDone: () => void }) {
  const api = useApp((s) => s.api);
  const agents = useApp((s) => s.agents);
  const toast = useApp((s) => s.toast);
  const [model, setModel] = useState<ModelRef | null>(null);
  const [saving, setSaving] = useState(false);
  return (
    <div className="space-y-4">
      <ModelPicker value={model} onChange={setModel} allowNone={false} />
      <div className="flex items-center justify-between">
        <button className="text-[12px] text-faint hover:text-muted" onClick={onDone}>Skip for now</button>
        <Button
          variant="primary"
          disabled={!model}
          loading={saving}
          onClick={async () => {
            if (!api || !model) return;
            setSaving(true);
            try {
              for (const a of agents) await api.updateAgent({ ...a, model });
              toast("success", "All agents are ready. Say hi!");
              onDone();
            } catch (e) {
              toast("error", errorText(e));
            } finally {
              setSaving(false);
            }
          }}
        >
          Use for all {agents.length} agents
        </Button>
      </div>
      <motion.p initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="text-[11.5px] text-faint leading-relaxed">
        Only reasoning models are listed: agents think before they answer, and you can watch their thinking stream in the chat.
      </motion.p>
    </div>
  );
}
