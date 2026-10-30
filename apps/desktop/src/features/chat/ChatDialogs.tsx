import { Brush, Check, FolderOpen, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { Agent, Chat, ShellPolicy } from "@dolphin/shared";
import { AgentAvatar, DEV_TEAM_SPECIALTIES } from "@/components/agent";
import { Button, ConfirmDialog, Dialog, Field, Input, Row, Switch } from "@/components/ui";
import { CHAT_ICONS } from "@/lib/chatIcons";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";

export function AgentChecklist({ selected, onChange }: { selected: string[]; onChange: (ids: string[]) => void }) {
  const agents = useApp((s) => s.agents);
  const specialists = agents.filter((a) => a.role === "specialist");
  const fixed = agents.filter((a) => a.role !== "specialist");
  const toggle = (a: Agent) => onChange(selected.includes(a.id) ? selected.filter((x) => x !== a.id) : [...selected, a.id]);
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-[12px] font-medium text-muted">Agents in this chat</span>
        <div className="flex gap-2 text-[11.5px]">
          <button className="text-accent hover:underline" onClick={() => onChange(specialists.map((a) => a.id))}>All</button>
          <button className="text-muted hover:underline" onClick={() => onChange([])}>None</button>
        </div>
      </div>
      <div className="grid grid-cols-2 gap-1.5">
        {specialists.map((a) => {
          const on = selected.includes(a.id);
          return (
            <button
              key={a.id}
              onClick={() => toggle(a)}
              className={cn("flex items-center gap-2.5 rounded-md border px-2.5 py-2 text-left transition-ui", on ? "border-[var(--focus-ring)] bg-accent-soft/60" : "border-line hover:bg-hover")}
            >
              <AgentAvatar agent={a} size={26} />
              <span className="min-w-0 flex-1">
                <span className="block text-[12.5px] font-semibold text-fg truncate">{a.name}</span>
                <span className="block text-[11px] text-faint truncate">{a.job.split(".")[0]}</span>
              </span>
              <span className={cn("size-4 rounded-[5px] border flex items-center justify-center", on ? "border-transparent [background:var(--brand-gradient)] text-white" : "border-line-strong")}>
                {on && <Check className="size-3" strokeWidth={3} />}
              </span>
            </button>
          );
        })}
      </div>
      <p className="text-[11.5px] text-faint">{fixed.map((a) => a.name).join(" and ")} are always present so routing and moderation keep working.</p>
    </div>
  );
}

function IconPicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="flex flex-wrap gap-1">
      {Object.entries(CHAT_ICONS).map(([k, Icon]) => (
        <button
          key={k}
          onClick={() => onChange(k)}
          className={cn("size-8 inline-flex items-center justify-center rounded-sm transition-ui", value === k ? "bg-accent-soft text-accent ring-1 ring-[var(--focus-ring)]" : "text-muted hover:bg-hover hover:text-fg")}
          aria-label={k}
        >
          <Icon className="size-4" />
        </button>
      ))}
    </div>
  );
}

export function NewChatDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const agents = useApp((s) => s.agents);
  const api = useApp((s) => s.api);
  const run = useApp((s) => s.run);
  const openChat = useApp((s) => s.openChat);
  const [name, setName] = useState("");
  const [icon, setIcon] = useState("hash");
  const [selected, setSelected] = useState<string[]>([]);
  useEffect(() => {
    if (open) {
      // Every agent is selected by default, except the development team; the user confirms or trims the list.
      setSelected(agents.filter((a) => a.role === "specialist" && !DEV_TEAM_SPECIALTIES.has(a.specialty)).map((a) => a.id));
      setName("");
      setIcon("hash");
    }
  }, [open, agents]);
  const create = async () => {
    if (!api) return;
    const chat = await run(api.createChat(name.trim(), icon, selected));
    if (chat) {
      onOpenChange(false);
      await openChat(chat.id);
    }
  };
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="New chat"
      description="Choose who takes part. Every agent is selected to start with, except the development team (add the director to make the chat a development chat)."
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" disabled={!name.trim()} onClick={create}>Create chat</Button>
        </>
      }
    >
      <div className="space-y-4">
        <Field label="Name">
          <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Side project" onKeyDown={(e) => e.key === "Enter" && name.trim() && create()} />
        </Field>
        <Field label="Icon">
          <IconPicker value={icon} onChange={setIcon} />
        </Field>
        <AgentChecklist selected={selected} onChange={setSelected} />
      </div>
    </Dialog>
  );
}

export function ChatSettingsDialog({ chat, open, onOpenChange }: { chat: Chat; open: boolean; onOpenChange: (o: boolean) => void }) {
  const agents = useApp((s) => s.agents);
  const api = useApp((s) => s.api);
  const run = useApp((s) => s.run);
  const globalCleanup = useApp((s) => s.settings["cleanup.enabled"] !== false);
  const [draft, setDraft] = useState<Chat>(chat);
  const [confirming, setConfirming] = useState<"clean" | "delete" | null>(null);
  const [policy, setPolicy] = useState<ShellPolicy | null>(null);
  const [newCommand, setNewCommand] = useState("");
  useEffect(() => {
    if (open) {
      setDraft(chat);
      setConfirming(null);
      setNewCommand("");
      setPolicy(null);
      void api?.shellPolicy(chat.id).then(setPolicy).catch(() => setPolicy({ network: false, allowCommands: [] }));
    }
  }, [open, chat, api]);
  const specialists = draft.agentIds.filter((id) => agents.find((a) => a.id === id)?.role === "specialist");
  const save = async () => {
    if (!api) return;
    let next = policy;
    const pending = newCommand.trim();
    if (next && pending && !next.allowCommands.includes(pending)) next = { ...next, allowCommands: [...next.allowCommands, pending] };
    const ok = await run(api.updateChat(draft), "Chat updated");
    if (ok && next) await run(api.setShellPolicy(chat.id, next));
    if (ok) onOpenChange(false);
  };
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      width={600}
      title={`${chat.name} settings`}
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" onClick={save}>Save</Button>
        </>
      }
    >
      <div className="space-y-5">
        <div className="grid grid-cols-[1fr_auto] gap-3">
          <Field label="Name">
            <Input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
          </Field>
        </div>
        <Field label="Icon">
          <IconPicker value={draft.icon} onChange={(icon) => setDraft({ ...draft, icon })} />
        </Field>
        <AgentChecklist
          selected={specialists}
          onChange={(ids) => setDraft({ ...draft, agentIds: [...agents.filter((a) => a.role !== "specialist").map((a) => a.id), ...ids] })}
        />

        <div className="rounded-lg border border-line divide-y divide-[var(--border)] px-3.5">
          <Row title="Clean this chat automatically" description={globalCleanup ? "Messages older than the age below are removed on a rolling basis." : "Periodic cleaning is turned off globally in Settings → General."}>
            <Switch checked={draft.cleanupEnabled} onChange={(v) => setDraft({ ...draft, cleanupEnabled: v })} />
          </Row>
          <Row title="Cleanup age" description="Hours a message is kept.">
            <div className="flex items-center gap-2">
              <Input type="number" min={1} max={2160} value={draft.cleanupTtlHours} onChange={(e) => setDraft({ ...draft, cleanupTtlHours: Math.max(1, Number(e.target.value) || 24) })} className="w-20 text-right" disabled={!draft.cleanupEnabled} />
              <span className="text-[12px] text-faint">hours</span>
            </div>
          </Row>
          <Row
            title="Workspace folder"
            description={draft.workspacePath ? <span className="font-mono">{draft.workspacePath}</span> : "Agents can read (and, with permission, change) files in this folder."}
          >
            <div className="flex gap-1.5">
              {draft.workspacePath && (
                <Button size="sm" variant="ghost" onClick={() => setDraft({ ...draft, workspacePath: null })}>
                  <X className="size-3.5" /> Remove
                </Button>
              )}
              <Button
                size="sm"
                onClick={async () => {
                  const p = await api?.pickFolder();
                  if (p) setDraft({ ...draft, workspacePath: p });
                }}
              >
                <FolderOpen className="size-3.5" /> Choose…
              </Button>
            </div>
          </Row>
        </div>

        {draft.workspacePath && policy && (
          <div className="rounded-lg border border-line divide-y divide-[var(--border)] px-3.5">
            <Row
              title="Commands can use the network"
              description="Off by default: commands agents run here cannot reach the internet, so a prompt injected through a web page cannot send your files out. Turn it on for installs and downloads."
            >
              <Switch checked={policy.network} onChange={(network) => setPolicy({ ...policy, network })} />
            </Row>
            <div className="py-3 space-y-2">
              <div className="text-[13px] font-medium text-fg">Pre-approved commands</div>
              <div className="text-[12px] text-muted">
                For agents set to ask first. A command starting with one of these runs without asking, unless it chains or pipes something else.
              </div>
              <div className="flex flex-wrap gap-1.5">
                {policy.allowCommands.length === 0 && <span className="text-[12px] text-faint">None yet. “Always allow” in an approval question adds one.</span>}
                {policy.allowCommands.map((c) => (
                  <span key={c} className="inline-flex items-center gap-1 rounded-md border border-line bg-hover px-2 py-0.5 font-mono text-[11.5px]">
                    {c}
                    <button className="text-faint hover:text-fg" title="Remove" onClick={() => setPolicy({ ...policy, allowCommands: policy.allowCommands.filter((x) => x !== c) })}>
                      <X className="size-3" />
                    </button>
                  </span>
                ))}
              </div>
              <Input
                value={newCommand}
                placeholder="Add a command prefix, e.g. cargo test"
                onChange={(e) => setNewCommand(e.target.value)}
                onKeyDown={(e) => {
                  const c = newCommand.trim();
                  if (e.key === "Enter" && c) {
                    e.preventDefault();
                    if (!policy.allowCommands.includes(c)) setPolicy({ ...policy, allowCommands: [...policy.allowCommands, c] });
                    setNewCommand("");
                  }
                }}
              />
            </div>
          </div>
        )}

        <div className="flex items-center justify-between gap-3 rounded-lg border border-danger/25 bg-danger/5 px-3.5 py-3">
          <div className="text-[12.5px] text-muted">Clean all messages now, or delete this chat.</div>
          <div className="flex gap-2">
            <Button size="sm" onClick={() => setConfirming("clean")}>
              <Brush className="size-3.5" /> Clean now
            </Button>
            <Button size="sm" variant="danger" onClick={() => setConfirming("delete")}>
              <Trash2 className="size-3.5" /> Delete
            </Button>
          </div>
        </div>
      </div>
      <CleanChatDialog chat={chat} open={confirming === "clean"} onOpenChange={(o) => !o && setConfirming(null)} />
      <ConfirmDialog
        open={confirming === "delete"}
        onOpenChange={(o) => !o && setConfirming(null)}
        title={`Delete ${chat.name}?`}
        description="This permanently deletes the chat, its messages and attachments. This can't be undone."
        confirmLabel="Delete chat"
        onConfirm={async () => {
          try {
            await api?.deleteChat(chat.id);
            onOpenChange(false);
            useApp.getState().toast("success", "Chat deleted");
          } catch (e) {
            useApp.getState().toast("error", String(e));
          }
        }}
      />
    </Dialog>
  );
}

/** Confirmation before wiping every message in a chat. */
export function CleanChatDialog({ chat, open, onOpenChange }: { chat: Chat; open: boolean; onOpenChange: (o: boolean) => void }) {
  const api = useApp((s) => s.api);
  const run = useApp((s) => s.run);
  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={`Clean ${chat.name}?`}
      description="This removes every message in this chat. This can't be undone."
      confirmLabel="Clean chat"
      onConfirm={async () => {
        if (api) await run(api.cleanChat(chat.id), "Chat cleaned");
      }}
    />
  );
}
