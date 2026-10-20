import { AnimatePresence, motion } from "motion/react";
import { Brush, FolderOpen, PanelRightClose, PanelRightOpen, ScrollText, Settings2, Upload } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ActionLogEntry, Chat } from "@dolphin/shared";
import { AgentAvatar, AgentHoverCard } from "@/components/agent";
import { Badge, Dialog, EmptyState, IconButton } from "@/components/ui";
import { chatIcon } from "@/lib/chatIcons";
import { formatTime } from "@/lib/format";
import { safeGet, safeSet, useApp } from "@/store/app";
import { ChatSettingsDialog, CleanChatDialog } from "./ChatDialogs";
import { Composer, type ComposerHandle } from "./Composer";
import { MembersPanel } from "./MembersPanel";
import { MessageList } from "./MessageList";

export function ChatView({ chat }: { chat: Chat }) {
  const api = useApp((s) => s.api);
  const agents = useApp((s) => s.agents);
  const busy = useApp(
    (s) => Object.values(s.activity[chat.id] ?? {}).some((a) => a.activity !== "idle") || (s.messages[chat.id] ?? []).some((m) => m.status === "streaming"),
  );
  const composer = useRef<ComposerHandle>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [cleanOpen, setCleanOpen] = useState(false);
  const [logOpen, setLogOpen] = useState(false);
  const [showMembers, setShowMembers] = useState(() => safeGet("dp.members") !== "0");
  const [dragging, setDragging] = useState(false);
  const Icon = chatIcon(chat.icon);
  const members = agents.filter((a) => chat.agentIds.includes(a.id) && a.role !== "orchestrator");

  // Native file drops (Tauri) arrive as paths.
  useEffect(() => {
    if (!api?.isTauri) return;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
      getCurrentWebview()
        .onDragDropEvent((e) => {
          if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
          else if (e.payload.type === "leave") setDragging(false);
          else if (e.payload.type === "drop") {
            setDragging(false);
            composer.current?.addPaths(e.payload.paths);
          }
        })
        .then((u) => (unlisten = u)),
    );
    return () => unlisten?.();
  }, [api, chat.id]);

  return (
    <div
      className="relative flex h-full min-w-0 flex-1 gap-2"
      onDragOver={(e) => {
        if (api?.isTauri) return;
        e.preventDefault();
        setDragging(true);
      }}
      onDragLeave={() => !api?.isTauri && setDragging(false)}
      onDrop={(e) => {
        if (api?.isTauri) return;
        e.preventDefault();
        setDragging(false);
        composer.current?.addFiles([...e.dataTransfer.files]);
      }}
    >
      <section className="glass relative flex min-w-0 flex-1 flex-col overflow-hidden rounded-xl">
        <header className="drag-region flex h-[60px] shrink-0 items-center gap-3 border-b border-line px-5">
          <Icon className="size-[18px] text-muted" />
          <h1 className="text-[15px] font-semibold tracking-tight text-fg truncate">{chat.name}</h1>
          <div className="no-drag ml-1 flex -space-x-1.5">
            {members.slice(0, 7).map((a) => (
              <AgentHoverCard key={a.id} agent={a} side="bottom">
                <span>
                  <AgentAvatar agent={a} size={22} className="ring-2 ring-[var(--surface-solid)] rounded-full" />
                </span>
              </AgentHoverCard>
            ))}
          </div>
          {chat.workspacePath && (
            <button className="no-drag" onClick={() => api?.revealPath(chat.workspacePath!)} title={chat.workspacePath}>
              <Badge className="hover:border-line-strong">
                <FolderOpen className="size-3" />
                {chat.workspacePath.split(/[\\/]/).pop()}
              </Badge>
            </button>
          )}
          <div className="no-drag ml-auto flex items-center gap-0.5">
            <IconButton label="Clean chat" onClick={() => setCleanOpen(true)}>
              <Brush className="size-[17px]" />
            </IconButton>
            <IconButton label="Action log" onClick={() => setLogOpen(true)}>
              <ScrollText className="size-[17px]" />
            </IconButton>
            <IconButton label="Chat settings" onClick={() => setSettingsOpen(true)}>
              <Settings2 className="size-[17px]" />
            </IconButton>
            <IconButton
              label={showMembers ? "Hide members" : "Show members"}
              onClick={() => {
                setShowMembers((v) => !v);
                safeSet("dp.members", showMembers ? "0" : "1");
              }}
            >
              {showMembers ? <PanelRightClose className="size-[17px]" /> : <PanelRightOpen className="size-[17px]" />}
            </IconButton>
          </div>
        </header>

        <MessageList chat={chat} onSuggest={(t) => composer.current?.setText(t)} />
        <Composer ref={composer} chat={chat} busy={busy} />

        <AnimatePresence>
          {dragging && (
            <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="pointer-events-none absolute inset-2 z-40 flex items-center justify-center rounded-xl border-2 border-dashed border-[var(--focus-ring)] bg-accent-soft/40 backdrop-blur-sm">
              <div className="flex flex-col items-center gap-2 text-fg">
                <Upload className="size-7" />
                <span className="text-[14px] font-medium">Drop files to attach</span>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </section>

      <AnimatePresence initial={false}>
        {showMembers && (
          <motion.div initial={{ width: 0, opacity: 0 }} animate={{ width: 240, opacity: 1 }} exit={{ width: 0, opacity: 0 }} transition={{ type: "spring", stiffness: 380, damping: 38 }} className="shrink-0 overflow-hidden">
            <MembersPanel chat={chat} />
          </motion.div>
        )}
      </AnimatePresence>

      <ChatSettingsDialog chat={chat} open={settingsOpen} onOpenChange={setSettingsOpen} />
      <CleanChatDialog chat={chat} open={cleanOpen} onOpenChange={setCleanOpen} />
      <ActionLogDialog chat={chat} open={logOpen} onOpenChange={setLogOpen} />
    </div>
  );
}

function ActionLogDialog({ chat, open, onOpenChange }: { chat: Chat; open: boolean; onOpenChange: (o: boolean) => void }) {
  const api = useApp((s) => s.api);
  const agents = useApp((s) => s.agents);
  const [entries, setEntries] = useState<ActionLogEntry[] | null>(null);
  useEffect(() => {
    if (open && api) void api.actionLog(chat.id).then(setEntries);
  }, [open, api, chat.id]);
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Action log" description="Every file and command action agents took in this chat, including blocked attempts." width={640}>
      {!entries || entries.length === 0 ? (
        <EmptyState icon={<ScrollText />} title="No actions yet">
          {chat.workspacePath ? "Agents haven't touched the workspace in this chat." : "Set a workspace folder in the chat settings to let agents read and work on files."}
        </EmptyState>
      ) : (
        <div className="divide-y divide-[var(--border)]">
          {entries.map((e) => {
            const a = agents.find((x) => x.id === e.agentId);
            return (
              <div key={e.id} className="flex items-center gap-3 py-2">
                {a && <AgentAvatar agent={a} size={22} />}
                <div className="min-w-0 flex-1">
                  <div className="truncate font-mono text-[12px] text-fg">{e.summary}</div>
                  <div className="text-[11px] text-faint">
                    {a?.name} · {formatTime(e.createdAt)}
                  </div>
                </div>
                {e.allowed ? <Badge tone="success">allowed</Badge> : <Badge tone="warning">blocked</Badge>}
              </div>
            );
          })}
        </div>
      )}
    </Dialog>
  );
}
