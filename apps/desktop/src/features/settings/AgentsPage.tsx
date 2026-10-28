import { Smartphone, Wand2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Agent, ModelRef } from "@dolphin/shared";
import { AgentAvatar, AgentNameText, SPECIALTY_LABEL } from "@/components/agent";
import { GradientEditor } from "@/components/GradientEditor";
import { ModelPicker } from "@/components/ModelPicker";
import { Button, Dialog, Field, Input, Textarea } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { cn } from "@/lib/cn";
import { formatDuration, formatTokens } from "@/lib/format";
import { useApp } from "@/store/app";
import { AgentMemoryPanel } from "./AgentMemory";
import { PageHeader } from "./SettingsView";

/** Matches the core limit (settings::PERSONA_MAX_CHARS). */
const PERSONA_MAX_CHARS = 1500;

export function AgentsPage() {
  const agents = useApp((s) => s.agents);
  const focus = useApp((s) => s.focusAgentId);
  const [selected, setSelected] = useState<string>(focus ?? agents[0]?.id);
  const [bulk, setBulk] = useState(false);
  useEffect(() => {
    if (focus) setSelected(focus);
  }, [focus]);
  const agent = agents.find((a) => a.id === selected) ?? agents[0];
  const unconfigured = agents.filter((a) => !a.model).length;

  return (
    <>
      <PageHeader
        title="Agents"
        description="Rename, recolor and choose each agent's model. Only reasoning models can be picked; the reasoning-effort control adapts to what the chosen model supports."
        actions={<Button onClick={() => setBulk(true)}><Wand2 className="size-4" />Set model for all</Button>}
      />
      {unconfigured > 0 && (
        <div className="mb-4 rounded-md border border-warning/30 bg-warning/8 px-3.5 py-2.5 text-[12.5px] text-muted">
          {unconfigured} agent{unconfigured === 1 ? " has" : "s have"} no model yet. Use <span className="font-medium text-fg">Set model for all</span> to get going in one step.
        </div>
      )}
      <div className="grid grid-cols-[210px_1fr] gap-4">
        <div className="space-y-0.5">
          {agents.map((a) => (
            <button
              key={a.id}
              onClick={() => setSelected(a.id)}
              className={cn("flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left transition-ui", a.id === agent?.id ? "bg-active border border-line" : "border border-transparent hover:bg-hover")}
            >
              <AgentAvatar agent={a} size={28} />
              <span className="min-w-0 flex-1">
                <AgentNameText agent={a} className="block text-[13px] leading-tight" />
                <span className={cn("block truncate text-[11px]", a.model ? "text-faint" : "text-warning")}>{a.model ? a.model.modelId : "no model"}</span>
              </span>
            </button>
          ))}
        </div>
        {agent && <AgentEditor key={agent.id + agent.updatedAt} agent={agent} />}
      </div>
      <BulkModelDialog open={bulk} onOpenChange={setBulk} />
    </>
  );
}

function AgentEditor({ agent }: { agent: Agent }) {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const usage = useApp((s) => s.usage[agent.id]);
  const [draft, setDraft] = useState<Agent>(agent);
  const [saving, setSaving] = useState(false);
  const dirty = useMemo(() => JSON.stringify(draft) !== JSON.stringify(agent), [draft, agent]);

  const save = async () => {
    if (!api) return;
    setSaving(true);
    try {
      await api.updateAgent(draft);
      toast("success", `${draft.name} saved`);
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="rounded-lg border border-line bg-surface-0 p-5 space-y-5">
      <div className="flex items-center gap-4">
        <AgentAvatar agent={draft} size={56} />
        <div className="min-w-0 flex-1">
          <AgentNameText agent={draft} className="text-[18px]" />
          <div className="text-[12px] text-muted">{SPECIALTY_LABEL[agent.specialty]}{agent.role === "orchestrator" && " · works behind the scenes"}</div>
        </div>
        <div className="flex gap-2">
          {dirty && <Button variant="ghost" onClick={() => setDraft(agent)}>Revert</Button>}
          <Button variant="primary" disabled={!dirty} loading={saving} onClick={save}>Save</Button>
        </div>
      </div>
      <p className="text-[12.5px] leading-relaxed text-muted">{agent.job}</p>

      <Field label="Name" hint="One word, so @mentions work.">
        <Input value={draft.name} maxLength={32} onChange={(e) => setDraft({ ...draft, name: e.target.value.replace(/\s/g, "") })} />
      </Field>

      <Field label={agent.specialty === "image" ? "Reasoning model (understands requests, writes image prompts)" : "Model"}>
        <ModelPicker value={draft.model} onChange={(model) => setDraft({ ...draft, model })} />
      </Field>

      {agent.specialty === "image" && (
        <Field label="Image model" hint="Image models aren't reasoning models, so Sarah pairs her reasoning model with a separate image-generation model (e.g. gpt-image, Gemini image, or an OpenRouter image model).">
          <ModelPicker value={draft.imageModel} onChange={(imageModel) => setDraft({ ...draft, imageModel })} purpose="image" />
        </Field>
      )}

      <Field
        label="Persona"
        hint="How this agent sounds and behaves: tone, quirks, how it addresses you. It is added on top of what the agent is built to do, so it can't remove its skills."
      >
        <Textarea
          rows={4}
          maxLength={PERSONA_MAX_CHARS}
          placeholder="e.g. Dry and to the point. Skips the pleasantries and calls me chief."
          value={draft.persona ?? ""}
          onChange={(e) => setDraft({ ...draft, persona: e.target.value.trim() ? e.target.value : null })}
        />
      </Field>

      <AgentMemoryPanel agent={agent} learning={draft.learning} onLearningChange={(learning) => setDraft({ ...draft, learning })} />

      <Field label="Color">
        <GradientEditor value={draft.color} onChange={(color) => setDraft({ ...draft, color })} />
      </Field>

      <div className="flex items-start gap-3 rounded-md border border-dashed border-line-strong px-3.5 py-3 text-[12px] text-faint">
        <Smartphone className="size-4 shrink-0 mt-px" />
        <span>An iPhone-specific model for this agent arrives with the iOS app and sync. Models too large for the phone will fall back to that override.</span>
      </div>

      <div className="grid grid-cols-4 gap-2">
        {[
          ["Tokens", formatTokens((usage?.tokensIn ?? 0) + (usage?.tokensOut ?? 0))],
          ["Requests", String(usage?.requests ?? 0)],
          ["Thinking", formatDuration(usage?.thinkingMs ?? 0)],
          ["Working", formatDuration(usage?.workingMs ?? 0)],
        ].map(([k, v]) => (
          <div key={k} className="rounded-md border border-line bg-hover/50 px-3 py-2">
            <div className="text-[10.5px] uppercase tracking-wide text-faint">{k}</div>
            <div className="text-[14px] font-semibold tabular-nums text-fg">{v}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

function BulkModelDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const agents = useApp((s) => s.agents);
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const [model, setModel] = useState<ModelRef | null>(null);
  const [onlyMissing, setOnlyMissing] = useState(true);
  const targets = agents.filter((a) => !onlyMissing || !a.model);
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Set one model for every agent"
      description="A quick start: give all agents the same reasoning model, then fine-tune individual agents later."
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button
            variant="primary"
            disabled={!model || targets.length === 0}
            onClick={async () => {
              if (!api || !model) return;
              try {
                for (const a of targets) await api.updateAgent({ ...a, model });
                toast("success", `Model set for ${targets.length} agent${targets.length === 1 ? "" : "s"}`);
                onOpenChange(false);
              } catch (e) {
                toast("error", errorText(e));
              }
            }}
          >
            Apply to {targets.length} agent{targets.length === 1 ? "" : "s"}
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <ModelPicker value={model} onChange={setModel} allowNone={false} />
        <label className="flex items-center gap-2 text-[12.5px] text-muted">
          <input type="checkbox" checked={onlyMissing} onChange={(e) => setOnlyMissing(e.target.checked)} className="accent-[var(--accent)]" />
          Only agents without a model
        </label>
      </div>
    </Dialog>
  );
}
