import { AlertCircle, Check, ChevronDown, Eye, Image as ImageIcon, Lock, RefreshCw, Search, Wrench } from "lucide-react";
import * as Popover from "@radix-ui/react-popover";
import { useEffect, useMemo, useState } from "react";
import type { EffortControl, EffortValue, ModelInfo, ModelRef } from "@dolphin/shared";
import { Badge, Button, Input, Segmented, Select, Spinner, Switch } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";

const modelCache = new Map<string, ModelInfo[]>();

export function useModels(providerId: string | null | undefined) {
  const api = useApp((s) => s.api);
  const [models, setModels] = useState<ModelInfo[] | null>(providerId ? (modelCache.get(providerId) ?? null) : null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const load = async (force = false) => {
    if (!api || !providerId) return;
    if (!force && modelCache.has(providerId)) {
      setModels(modelCache.get(providerId)!);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const m = await api.listModels(providerId);
      modelCache.set(providerId, m);
      setModels(m);
    } catch (e) {
      setError(errorText(e));
      setModels(null);
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => {
    setModels(providerId ? (modelCache.get(providerId) ?? null) : null);
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providerId, api]);
  return { models, error, loading, reload: () => load(true) };
}

export function defaultEffort(c: EffortControl): EffortValue | null {
  switch (c.type) {
    case "levels":
      return { type: "level", value: c.default };
    case "budget":
      return { type: "budget", value: c.default };
    case "toggle":
      return { type: "toggle", value: true };
    default:
      return null;
  }
}

/** Provider → model → effort. `purpose="image"` lists image-generation models instead of reasoning models. */
export function ModelPicker({ value, onChange, purpose = "reasoning", allowNone = true }: {
  value: ModelRef | null;
  onChange: (m: ModelRef | null) => void;
  purpose?: "reasoning" | "image" | "utility";
  allowNone?: boolean;
}) {
  const providers = useApp((s) => s.providers);
  const setView = useApp((s) => s.setView);
  const api = useApp((s) => s.api);
  const providerId = value?.providerId ?? null;
  const fallbackProvider = !allowNone && providers.length === 1 ? providers[0].id : null;
  const [pickedProvider, setPickedProvider] = useState<string | null>(providerId ?? fallbackProvider);
  useEffect(() => setPickedProvider(providerId ?? fallbackProvider), [providerId, fallbackProvider]);
  const { models, error, loading, reload } = useModels(pickedProvider);
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [showAll, setShowAll] = useState(false);
  const [manual, setManual] = useState("");

  const selectable = (m: ModelInfo) => (purpose === "image" ? m.imageOutput : purpose === "utility" ? true : m.reasoning);
  const filtered = useMemo(() => {
    const list = (models ?? []).filter((m) => !q || m.id.toLowerCase().includes(q.toLowerCase()) || m.name.toLowerCase().includes(q.toLowerCase()));
    return showAll || purpose === "utility" ? list : list.filter(selectable);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [models, q, showAll, purpose]);
  const hiddenCount = (models ?? []).filter((m) => !selectable(m)).length;
  const current = models?.find((m) => m.id === value?.modelId);

  if (providers.length === 0) {
    return (
      <div className="flex items-center justify-between gap-3 rounded-md border border-dashed border-line-strong px-3 py-2.5 text-[12.5px] text-muted">
        No providers yet.
        <Button size="sm" onClick={() => setView("settings", "providers")}>Add a provider</Button>
      </div>
    );
  }

  const pick = (m: ModelInfo) => {
    if (!pickedProvider) return;
    onChange({ providerId: pickedProvider, modelId: m.id, effort: purpose === "image" ? null : defaultEffort(m.effort), vision: m.vision, tools: m.tools, contextWindow: m.contextWindow });
    setOpen(false);
  };

  return (
    <div className="space-y-2.5">
      <div className="grid grid-cols-[minmax(0,0.9fr)_minmax(0,1.6fr)] gap-2">
        <Select
          value={pickedProvider ?? ""}
          onChange={(e) => {
            const id = e.target.value || null;
            setPickedProvider(id);
            if (!id) onChange(null);
          }}
        >
          <option value="">{allowNone ? "None" : "Choose provider"}</option>
          {providers.map((p) => (
            <option key={p.id} value={p.id} disabled={!p.enabled}>
              {p.name}
            </option>
          ))}
        </Select>

        <Popover.Root open={open} onOpenChange={setOpen}>
          <Popover.Trigger asChild>
            <button
              disabled={!pickedProvider}
              className="no-drag flex h-8.5 w-full items-center justify-between gap-2 rounded-sm border border-line bg-input px-3 text-left text-[13px] transition-ui hover:border-line-strong disabled:opacity-50"
            >
              <span className={cn("truncate", value?.providerId === pickedProvider && value?.modelId ? "text-fg font-mono text-[12.5px]" : "text-faint")}>
                {value?.providerId === pickedProvider && value?.modelId ? value.modelId : "Choose model"}
              </span>
              {loading ? <Spinner className="size-3.5" /> : <ChevronDown className="size-3.5 text-faint shrink-0" />}
            </button>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content align="end" sideOffset={6} className="glass-float z-50 w-[400px] rounded-lg p-2 outline-none">
              <div className="flex items-center gap-2 px-1 pb-2">
                <div className="relative flex-1">
                  <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-faint" />
                  <Input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search models" className="pl-8 h-8" />
                </div>
                <button className="size-8 inline-flex items-center justify-center rounded-sm text-faint hover:bg-hover hover:text-fg" onClick={reload} title="Refresh models">
                  <RefreshCw className={cn("size-3.5", loading && "animate-spin")} />
                </button>
              </div>
              {error && (
                <div className="mx-1 mb-2 flex items-start gap-2 rounded-sm bg-danger/10 px-2.5 py-2 text-[12px] text-danger">
                  <AlertCircle className="size-3.5 mt-px shrink-0" /> {error}
                </div>
              )}
              <div className="max-h-72 overflow-y-auto">
                {filtered.map((m) => {
                  const ok = selectable(m);
                  return (
                    <button
                      key={m.id}
                      disabled={!ok}
                      onClick={() => pick(m)}
                      className={cn("flex w-full items-start gap-2.5 rounded-sm px-2.5 py-2 text-left transition-ui", ok ? "hover:bg-hover" : "opacity-55 cursor-not-allowed")}
                    >
                      <span className="mt-0.5 size-4 shrink-0 text-accent">{value?.modelId === m.id && value?.providerId === pickedProvider ? <Check className="size-4" /> : !ok ? <Lock className="size-3.5 text-faint" /> : null}</span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate font-mono text-[12.5px] text-fg">{m.id}</span>
                        {!ok && m.reason && <span className="block text-[11px] text-faint leading-snug">{purpose === "image" ? "Not an image-generation model." : m.reason}</span>}
                        <span className="mt-1 flex flex-wrap gap-1">
                          {m.reasoning && <Badge tone="accent">reasoning</Badge>}
                          {m.vision && <Badge><Eye className="size-3" />vision</Badge>}
                          {m.tools && purpose !== "image" && <Badge><Wrench className="size-3" />tools</Badge>}
                          {m.imageOutput && <Badge><ImageIcon className="size-3" />image</Badge>}
                          {m.free && <Badge tone="success">free</Badge>}
                          {m.contextWindow && <Badge>{Math.round(m.contextWindow / 1000)}k ctx</Badge>}
                        </span>
                      </span>
                    </button>
                  );
                })}
                {models && filtered.length === 0 && <div className="px-3 py-6 text-center text-[12.5px] text-faint">No matching models.</div>}
              </div>
              <div className="mt-2 border-t border-line pt-2 px-1 space-y-2">
                {purpose !== "utility" && hiddenCount > 0 && (
                  <label className="flex items-center justify-between text-[12px] text-muted">
                    Show {hiddenCount} unavailable model{hiddenCount === 1 ? "" : "s"}
                    <Switch checked={showAll} onChange={setShowAll} />
                  </label>
                )}
                <form
                  className="flex gap-2"
                  onSubmit={async (e) => {
                    e.preventDefault();
                    if (!api || !manual.trim()) return;
                    const info = await api.inspectModel(manual.trim());
                    if (purpose === "reasoning" && !info.reasoning) {
                      useApp.getState().toast("warning", `${manual.trim()}: ${info.reason ?? "not a reasoning model"}`);
                      return;
                    }
                    pick(info);
                    setManual("");
                  }}
                >
                  <Input value={manual} onChange={(e) => setManual(e.target.value)} placeholder="Other model id (e.g. an Azure deployment)" className="h-8 text-[12px]" />
                  <Button size="sm" type="submit">Use</Button>
                </form>
              </div>
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
      </div>
      {purpose !== "image" && value && current && <EffortControlView control={current.effort} value={value.effort} onChange={(effort) => onChange({ ...value, effort })} />}
    </div>
  );
}

/** Shows only the effort controls the selected model supports; hidden when it has none. */
export function EffortControlView({ control, value, onChange }: { control: EffortControl; value: EffortValue | null; onChange: (v: EffortValue | null) => void }) {
  if (control.type === "none") return null;
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border border-line bg-hover/40 px-3 py-2">
      <span className="text-[12px] font-medium text-muted">Reasoning effort</span>
      {control.type === "levels" && (
        <Segmented
          value={value?.type === "level" ? value.value : control.default}
          options={control.levels.map((l) => ({ value: l, label: l }))}
          onChange={(l) => onChange({ type: "level", value: l })}
        />
      )}
      {control.type === "budget" && (
        <div className="flex items-center gap-2">
          <input
            type="range"
            min={control.min}
            max={control.max}
            step={256}
            value={value?.type === "budget" ? value.value : control.default}
            onChange={(e) => onChange({ type: "budget", value: Number(e.target.value) })}
            className="w-40 accent-[var(--accent)]"
            aria-label="Thinking budget"
          />
          <span className="w-20 text-right font-mono text-[11.5px] text-faint">{(value?.type === "budget" ? value.value : control.default).toLocaleString()} tok</span>
        </div>
      )}
      {control.type === "toggle" && <Switch checked={value?.type === "toggle" ? value.value : true} onChange={(v) => onChange({ type: "toggle", value: v })} label="Thinking" />}
    </div>
  );
}
