import { Brain, Pin, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { Agent, AgentMemory, MemoryKind } from "@dolphin/shared";
import { Badge, Button, ConfirmDialog, IconButton, Input, Select, Switch } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { MEMORY_KINDS, MEMORY_MAX_ENTRIES, MEMORY_MAX_TEXT, projectOf } from "@/lib/memory";
import { useApp } from "@/store/app";

/**
 * What one agent has learned about you. Changes to the list save as you go; only this agent ever sees them.
 * The learning switch belongs to the agent itself, so it goes through the editor's draft and Save like its other settings.
 */
export function AgentMemoryPanel({ agent, learning, onLearningChange, suggestPersona, onSuggestPersonaChange }: {
  agent: Agent;
  learning: boolean;
  onLearningChange: (on: boolean) => void;
  suggestPersona: boolean;
  onSuggestPersonaChange: (on: boolean) => void;
}) {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const version = useApp((s) => s.memoryVersion[agent.id] ?? 0);
  const chats = useApp((s) => s.chats);
  const [items, setItems] = useState<AgentMemory[] | null>(null);
  const [confirmForget, setConfirmForget] = useState(false);

  const projects = useMemo(() => [...new Set(chats.map((c) => projectOf(c.workspacePath)).filter((p): p is string => !!p))].sort(), [chats]);

  const reload = useCallback(async () => {
    if (!api) return;
    try {
      setItems(await api.listAgentMemories(agent.id));
    } catch (e) {
      toast("error", errorText(e));
    }
  }, [api, agent.id, toast]);

  // Reload when the agent changes and whenever its memory changed elsewhere (its learner, another device).
  useEffect(() => {
    void reload();
  }, [reload, version]);

  const run = async (op: () => Promise<unknown>) => {
    try {
      await op();
      await reload();
    } catch (e) {
      toast("error", errorText(e));
      await reload();
    }
  };

  const full = (items?.length ?? 0) >= MEMORY_MAX_ENTRIES;

  return (
    <section className="space-y-3 rounded-md border border-line bg-hover/30 p-3.5" aria-label={`${agent.name}'s memory`}>
      <div className="flex items-start gap-2.5">
        <Brain className="size-4 shrink-0 mt-0.5 text-muted" />
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium text-fg">Memory</div>
          <p className="text-[12px] leading-relaxed text-muted">
            What {agent.name} knows about you. Only {agent.name} sees it, it is sent along with their requests, and changes here are saved right away.
          </p>
        </div>
        <span className="text-[11px] tabular-nums text-faint">{items?.length ?? 0} / {MEMORY_MAX_ENTRIES}</span>
      </div>

      <div className="flex items-start gap-2.5 rounded-md bg-hover/60 px-2.5 py-2">
        <Switch checked={learning} onChange={onLearningChange} label={`${agent.name} learns from you`} />
        <div className="min-w-0 flex-1 text-[12px] leading-relaxed text-muted">
          <span className="font-medium text-fg">Learn from my corrections</span>
          <br />
          {learning
            ? `${agent.name} notes lasting preferences when you correct them, undo their changes, decline an action or give a thumbs down, and can save notes themself. Notes it writes are marked “learned”.`
            : `${agent.name} won't add notes on their own. Saying “remember that…” and the list below still work.`}
          {learning !== agent.learning && <span className="text-warning"> Press Save to apply.</span>}
        </div>
      </div>

      <div className="flex items-start gap-2.5 rounded-md bg-hover/60 px-2.5 py-2">
        <Switch checked={suggestPersona && learning} disabled={!learning} onChange={onSuggestPersonaChange} label={`${agent.name} may suggest persona changes`} />
        <div className="min-w-0 flex-1 text-[12px] leading-relaxed text-muted">
          <span className="font-medium text-fg">Let it suggest persona changes</span>
          <br />
          {`${agent.name} may propose a rewrite of its own persona when you have shown it, repeatedly, how you want it to sound. You see the suggestion first; it only takes effect if you accept it.`}
          {!learning && " Needs learning to be on."}
          {learning && suggestPersona !== agent.suggestPersona && <span className="text-warning"> Press Save to apply.</span>}
        </div>
      </div>

      {items && items.length === 0 && <p className="text-[12.5px] text-faint">{agent.name} hasn't learned anything yet.</p>}

      <ul className="space-y-2">
        {items?.map((m) => (
          <MemoryRow
            key={m.id + m.updatedAt}
            memory={m}
            projects={projects}
            onChange={(next) => run(() => api!.updateAgentMemory(next))}
            onDelete={() => run(() => api!.deleteAgentMemory(m.id))}
          />
        ))}
      </ul>

      <AddMemory
        projects={projects}
        disabled={full}
        disabledReason={full ? `${agent.name} already remembers ${MEMORY_MAX_ENTRIES} things. Forget some first.` : undefined}
        onAdd={(kind, text, project) => run(() => api!.addAgentMemory(agent.id, kind, text, project))}
      />

      {(items?.length ?? 0) > 0 && (
        <div className="flex justify-end">
          <Button variant="ghost" onClick={() => setConfirmForget(true)}>Forget everything</Button>
        </div>
      )}
      <ConfirmDialog
        open={confirmForget}
        onOpenChange={setConfirmForget}
        title={`Forget everything ${agent.name} knows about you?`}
        description={`This removes ${items?.length === 1 ? "its one entry" : `all ${items?.length ?? 0} entries`}, pinned ones included. It can't be undone.`}
        confirmLabel="Forget everything"
        onConfirm={() => run(() => api!.forgetAgentMemories(agent.id))}
      />
    </section>
  );
}

const SOURCE_LABEL = { auto: "learned", explicit: "you added", edited: "you edited" } as const;

function ScopeSelect({ value, projects, onChange, label }: { value: string | null; projects: string[]; onChange: (v: string | null) => void; label: string }) {
  // A project this device has no chat for (it came from another device) must still show up as selected.
  const options = value && !projects.includes(value) ? [...projects, value] : projects;
  return (
    <Select aria-label={label} className="h-7 w-auto! max-w-[170px] text-[12px]" value={value ?? ""} onChange={(e) => onChange(e.target.value || null)}>
      <option value="">Everywhere</option>
      {options.map((p) => (
        <option key={p} value={p}>Only in {p}</option>
      ))}
    </Select>
  );
}

function KindSelect({ value, onChange, label }: { value: MemoryKind; onChange: (v: MemoryKind) => void; label: string }) {
  return (
    <Select aria-label={label} className="h-7 w-auto! text-[12px]" value={value} onChange={(e) => onChange(e.target.value as MemoryKind)} title={MEMORY_KINDS.find((k) => k.value === value)?.hint}>
      {MEMORY_KINDS.map((k) => (
        <option key={k.value} value={k.value}>{k.label}</option>
      ))}
    </Select>
  );
}

function MemoryRow({ memory, projects, onChange, onDelete }: { memory: AgentMemory; projects: string[]; onChange: (m: AgentMemory) => void; onDelete: () => void }) {
  const [text, setText] = useState(memory.text);
  const commitText = () => {
    if (text.trim() && text.trim() !== memory.text) onChange({ ...memory, text });
    else setText(memory.text);
  };
  return (
    <li className="space-y-1.5 rounded-md border border-line bg-surface-0 p-2.5">
      <div className="flex items-center gap-1">
        <Input
          aria-label="Memory text"
          className="min-w-0 flex-1"
          value={text}
          maxLength={MEMORY_MAX_TEXT}
          onChange={(e) => setText(e.target.value)}
          onBlur={commitText}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
            if (e.key === "Escape") setText(memory.text);
          }}
        />
        <IconButton size="sm" label={memory.pinned ? "Unpin (it may be dropped when memory is full)" : "Pin (always send it, never drop it)"} active={memory.pinned} onClick={() => onChange({ ...memory, pinned: !memory.pinned })}>
          <Pin className="size-3.5" />
        </IconButton>
        <IconButton size="sm" label="Forget this" onClick={onDelete}>
          <Trash2 className="size-3.5" />
        </IconButton>
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        <KindSelect label="Kind" value={memory.kind} onChange={(kind) => onChange({ ...memory, kind })} />
        <ScopeSelect label="Where it applies" value={memory.project} projects={projects} onChange={(project) => onChange({ ...memory, project })} />
        <Badge tone={memory.source === "auto" ? "accent" : "neutral"}>{SOURCE_LABEL[memory.source]}</Badge>
      </div>
    </li>
  );
}

function AddMemory({ projects, disabled, disabledReason, onAdd }: { projects: string[]; disabled: boolean; disabledReason?: string; onAdd: (kind: MemoryKind, text: string, project: string | null) => void }) {
  const [kind, setKind] = useState<MemoryKind>("style");
  const [project, setProject] = useState<string | null>(null);
  const [text, setText] = useState("");
  const submit = () => {
    if (!text.trim() || disabled) return;
    onAdd(kind, text, project);
    setText("");
  };
  return (
    <div className="space-y-1.5 border-t border-line pt-3">
      <Input
        aria-label="Add a memory"
        placeholder={disabledReason ?? "Teach it something, e.g. “Keep answers under three sentences”"}
        value={text}
        maxLength={MEMORY_MAX_TEXT}
        disabled={disabled}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && submit()}
      />
      <div className="flex flex-wrap items-center gap-1.5">
        <KindSelect label="Kind of new memory" value={kind} onChange={setKind} />
        <ScopeSelect label="Where the new memory applies" value={project} projects={projects} onChange={setProject} />
        <span className="flex-1" />
        <Button variant="primary" size="sm" disabled={disabled || !text.trim()} onClick={submit}>
          <Plus className="size-3.5" />Add
        </Button>
      </div>
    </div>
  );
}
