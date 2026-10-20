import { AnimatePresence, motion } from "motion/react";
import { AlertTriangle, ArrowRight, CheckCircle2, ChevronRight, CircleSlash, ClipboardList, Copy, FileText, Loader2, Settings2, ShieldAlert, Sparkles, Wrench } from "lucide-react";
import { memo, useState } from "react";
import type { Agent, Attachment, Message } from "@dolphin/shared";
import { AgentAvatar, AgentHoverCard, AgentName } from "@/components/agent";
import { UserAvatar, UserHoverCard, UserNameText } from "@/components/user";
import { Markdown } from "@/components/Markdown";
import { ThinkingBlock } from "@/components/ThinkingBlock";
import { Badge, Button, Dialog } from "@/components/ui";
import { cn } from "@/lib/cn";
import { formatBytes, formatDuration, formatTime, formatTokens } from "@/lib/format";
import { useApp } from "@/store/app";

type Meta = {
  routing?: { status?: string; mode?: string; intent?: string; targets?: string[]; source?: string; needsSetup?: boolean; error?: string | null; via?: string; brief?: string; rewrite?: string };
  tools?: { name: string; summary: string; ok: boolean; allowed: boolean; error?: string | null }[];
  plan?: { approved?: boolean };
  needsSetup?: boolean;
  notice?: string;
  context?: { window?: number; peakTokens?: number; savedTokens?: number; compactions?: number; compactedMessages?: number; cacheRead?: number; cacheWritten?: number };
};

const MODE_LABEL: Record<string, string> = { quick: "Quick", learn: "Learn", plan: "Plan", implement: "Implement", answer: "Answer", internal: "Settings", clarify: "Clarify" };

export const MessageItem = memo(function MessageItem({ message, agent, grouped }: { message: Message; agent?: Agent; grouped: boolean }) {
  const meta = (message.meta ?? {}) as Meta;
  if (message.kind === "notice") return <Notice message={message} agent={agent} />;
  if (message.kind === "ack") return <Ack message={message} agent={agent} meta={meta} />;
  const streaming = message.status === "streaming" || message.status === "pending";
  const isUser = !message.agentId;

  return (
    <div className={cn("group relative flex gap-3 px-5 transition-colors hover:bg-hover/40", grouped ? "pt-0.5 pb-1" : "pt-3 pb-1")}>
      <div className="w-9 shrink-0 pt-0.5">
        {!grouped && (isUser ? (
          <UserHoverCard side="right">
            <span><UserAvatar size={36} /></span>
          </UserHoverCard>
        ) : agent && (
          <AgentHoverCard agent={agent} side="right">
            <span><AgentAvatar agent={agent} size={36} busy={streaming} /></span>
          </AgentHoverCard>
        ))}
        {grouped && <span className="block pt-1 text-right text-[10px] text-faint opacity-0 group-hover:opacity-100 tabular-nums">{formatTime(message.createdAt).replace(/^.* /, "")}</span>}
      </div>
      <div className="min-w-0 flex-1">
        {!grouped && (
          <div className="flex items-baseline gap-2 leading-tight mb-0.5">
            {isUser ? (
              <UserHoverCard>
                <span className="cursor-default"><UserNameText className="text-[14px]" /></span>
              </UserHoverCard>
            ) : agent && <AgentName agent={agent} className="text-[14px]" />}
            {!isUser && meta.routing?.mode && meta.routing.mode !== "answer" && <Badge tone={meta.routing.mode === "plan" ? "warning" : "accent"}>{MODE_LABEL[meta.routing.mode] ?? meta.routing.mode}</Badge>}
            {meta.routing?.via === "mention" && <Badge>direct</Badge>}
            <span className="text-[11px] text-faint tabular-nums">{formatTime(message.createdAt)}</span>
          </div>
        )}

        {meta.routing?.brief && <Brief text={meta.routing.brief} />}
        {message.reasoning && <ThinkingBlock text={message.reasoning} live={streaming && !message.content} thinkingMs={message.thinkingMs} startedAt={message.createdAt} />}
        {streaming && !message.content && !message.reasoning && <Waiting />}

        {message.content && (
          <div className={cn("text-[14px] text-fg", streaming && "is-streaming")}>
            <Markdown text={message.content} streaming={streaming} />
          </div>
        )}

        {meta.tools && meta.tools.length > 0 && <ToolActivity tools={meta.tools} />}
        {message.attachments.length > 0 && <Attachments items={message.attachments} />}
        {message.status === "error" && <ErrorBox message={message} agent={agent} needsSetup={meta.needsSetup} />}
        {message.status === "cancelled" && message.content !== "_Stopped._" && <div className="mt-1 inline-flex items-center gap-1 text-[11.5px] text-faint"><CircleSlash className="size-3" /> Stopped</div>}
        {message.kind === "plan" && message.status === "done" && <PlanBar message={message} approved={!!meta.plan?.approved} agent={agent} />}
        {!isUser && message.status === "done" && <Footer message={message} />}
      </div>
    </div>
  );
});

function Waiting() {
  return (
    <div className="flex items-center gap-1 py-1.5" aria-label="Working">
      {[0, 1, 2].map((i) => (
        <motion.span key={i} className="size-1.5 rounded-full bg-faint" animate={{ opacity: [0.3, 1, 0.3], y: [0, -2, 0] }} transition={{ duration: 1, repeat: Infinity, delay: i * 0.15 }} />
      ))}
    </div>
  );
}

function Ack({ message, agent, meta }: { message: Message; agent?: Agent; meta: Meta }) {
  const r = meta.routing ?? {};
  const setView = useApp((s) => s.setView);
  const routing = r.status === "routing";
  return (
    <div className="flex items-center gap-3 px-5 pt-2.5 pb-0.5">
      <div className="w-9 flex justify-end">{agent && <AgentAvatar agent={agent} size={22} busy={routing} />}</div>
      <div className="min-w-0 flex flex-wrap items-center gap-x-2 gap-y-1 text-[12.5px] text-muted">
        {agent && <AgentName agent={agent} className="text-[12.5px]" />}
        <span className="italic">{message.content}</span>
        <AnimatePresence mode="wait">
          {routing ? (
            <motion.span key="r" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="inline-flex items-center gap-1 text-faint">
              <Loader2 className="size-3 animate-spin" /> routing
            </motion.span>
          ) : (
            <motion.span key="d" initial={{ opacity: 0, x: -4 }} animate={{ opacity: 1, x: 0 }} className="inline-flex items-center gap-1.5">
              {r.targets && r.targets.length > 0 && (
                <>
                  <ArrowRight className="size-3 text-faint" />
                  <span className="font-medium text-fg">{r.targets.join(", ")}</span>
                </>
              )}
              {r.mode && <Badge tone={r.mode === "plan" ? "warning" : "neutral"}>{MODE_LABEL[r.mode] ?? r.mode}</Badge>}
              {r.source === "fallback" && (
                <button className="inline-flex items-center gap-1 text-[11px] text-warning hover:underline" onClick={() => setView("settings", "agents", message.agentId)} title={r.error ?? "Roger has no model, so keyword routing was used."}>
                  <AlertTriangle className="size-3" /> {r.needsSetup ? "keyword routing · set Roger's model" : "keyword routing"}
                </button>
              )}
            </motion.span>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

function Notice({ message, agent }: { message: Message; agent?: Agent }) {
  return (
    <div className="flex justify-center px-5 py-2">
      <div className="inline-flex max-w-[80%] items-center gap-2 rounded-full border border-line bg-surface-0 px-3 py-1 text-[12px] text-muted">
        {agent && <AgentAvatar agent={agent} size={16} />}
        <Markdown text={message.content} className="[&_p]:m-0 leading-snug" />
      </div>
    </div>
  );
}

function Brief({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  const lincoln = useApp((s) => s.agents.find((a) => a.slug === "lincoln"));
  return (
    <div className="mb-1">
      <button onClick={() => setOpen((o) => !o)} className="inline-flex items-center gap-1 text-[11.5px] text-faint hover:text-muted transition-ui">
        <Sparkles className="size-3" /> Brief from {lincoln?.name ?? "Lincoln"}
        <ChevronRight className={cn("size-3 transition-transform", open && "rotate-90")} />
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden">
            <div className="selectable mt-1 rounded-md border border-line bg-hover/50 px-3 py-2 text-[12.5px] text-muted whitespace-pre-wrap">{text}</div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function ToolActivity({ tools }: { tools: NonNullable<Meta["tools"]> }) {
  const [open, setOpen] = useState(false);
  const shown = open ? tools : tools.slice(-3);
  return (
    <div className="mt-2 rounded-md border border-line bg-hover/40 px-2.5 py-1.5">
      {shown.map((t, i) => (
        <div key={i} className="flex items-center gap-2 py-0.5 text-[12px]">
          {!t.allowed ? <ShieldAlert className="size-3.5 text-warning" /> : t.ok ? <CheckCircle2 className="size-3.5 text-success" /> : <AlertTriangle className="size-3.5 text-danger" />}
          <span className="font-mono text-muted truncate">{t.summary}</span>
          {!t.allowed && <span className="text-warning text-[11px]">blocked: read-only</span>}
          {t.allowed && !t.ok && t.error && <span className="text-danger text-[11px] truncate">{t.error}</span>}
        </div>
      ))}
      {tools.length > 3 && (
        <button onClick={() => setOpen((o) => !o)} className="mt-0.5 inline-flex items-center gap-1 text-[11px] text-faint hover:text-muted">
          <Wrench className="size-3" /> {open ? "Show less" : `Show all ${tools.length} actions`}
        </button>
      )}
    </div>
  );
}

function Attachments({ items }: { items: Attachment[] }) {
  const api = useApp((s) => s.api);
  const [zoom, setZoom] = useState<Attachment | null>(null);
  return (
    <div className="mt-2 flex flex-wrap gap-2">
      {items.map((a) =>
        a.kind === "image" ? (
          <button key={a.id} onClick={() => setZoom(a)} className="overflow-hidden rounded-md border border-line hover:border-line-strong transition-ui">
            <img src={api?.fileUrl(a.path)} alt={a.name} className="max-h-72 max-w-[420px] object-cover" loading="lazy" />
          </button>
        ) : (
          <button key={a.id} onClick={() => api?.revealPath(a.path)} className="flex items-center gap-2 rounded-md border border-line bg-hover/50 px-2.5 py-1.5 text-left hover:bg-hover transition-ui">
            <FileText className="size-4 text-muted" />
            <span className="min-w-0">
              <span className="block max-w-56 truncate text-[12.5px] text-fg">{a.name}</span>
              <span className="block text-[11px] text-faint">{formatBytes(a.size)}</span>
            </span>
          </button>
        ),
      )}
      <Dialog open={!!zoom} onOpenChange={(o) => !o && setZoom(null)} title={zoom?.name ?? ""} width={980}>
        {zoom && <img src={api?.fileUrl(zoom.path)} alt={zoom.name} className="w-full rounded-md" />}
      </Dialog>
    </div>
  );
}

function ErrorBox({ message, agent, needsSetup }: { message: Message; agent?: Agent; needsSetup?: boolean }) {
  const setView = useApp((s) => s.setView);
  return (
    <div className="mt-1.5 flex items-start gap-2.5 rounded-md border border-danger/25 bg-danger/8 px-3 py-2 text-[12.5px]">
      <AlertTriangle className="size-4 shrink-0 text-danger mt-px" />
      <div className="min-w-0 flex-1 text-fg/90 selectable">{message.error}</div>
      {needsSetup && agent && (
        <Button size="sm" onClick={() => setView("settings", "agents", agent.id)}>
          <Settings2 className="size-3.5" /> Set up {agent.name}
        </Button>
      )}
    </div>
  );
}

function PlanBar({ message, approved, agent }: { message: Message; approved: boolean; agent?: Agent }) {
  const api = useApp((s) => s.api);
  const run = useApp((s) => s.run);
  const [busy, setBusy] = useState(false);
  return (
    <div className={cn("mt-2.5 flex items-center gap-3 rounded-lg border px-3.5 py-2.5", approved ? "border-success/30 bg-success/8" : "border-warning/30 bg-warning/8")}>
      <ClipboardList className={cn("size-4", approved ? "text-success" : "text-warning")} />
      <div className="flex-1 text-[12.5px] text-muted">
        {approved ? (
          <span><span className="font-medium text-success">Approved.</span> {agent?.name ?? "The agent"} has full agentic permissions in this chat.</span>
        ) : (
          <span>This is a plan. {agent?.name ?? "The agent"} is read-only until you approve it.</span>
        )}
      </div>
      {!approved && (
        <Button
          variant="primary"
          size="sm"
          loading={busy}
          onClick={async () => {
            if (!api) return;
            setBusy(true);
            await run(api.approvePlan(message.id));
            setBusy(false);
          }}
        >
          <CheckCircle2 className="size-3.5" /> Approve plan
        </Button>
      )}
    </div>
  );
}

function Footer({ message }: { message: Message }) {
  const [copied, setCopied] = useState(false);
  const tokens = (message.tokensIn ?? 0) + (message.tokensOut ?? 0);
  const ctx = (message.meta as Meta | undefined)?.context;
  return (
    <div className="mt-1 flex h-5 items-center gap-3 text-[11px] text-faint opacity-0 transition-opacity group-hover:opacity-100">
      {message.workingMs != null && <span>{formatDuration(message.workingMs)}</span>}
      {message.thinkingMs != null && <span>thought {formatDuration(message.thinkingMs)}</span>}
      {tokens > 0 && <span>{formatTokens(tokens)} tokens</span>}
      {(ctx?.cacheRead ?? 0) > 0 && <span title="Input tokens served from the provider's prompt cache">{formatTokens(ctx!.cacheRead!)} cached</span>}
      {(ctx?.savedTokens ?? 0) > 0 && <span title="Tool output compressed before the model saw it. The full text stays retrievable.">−{formatTokens(ctx!.savedTokens!)} compressed</span>}
      {(ctx?.compactions ?? 0) > 0 && <span title="Older context was summarized to stay within the model's window">compacted context</span>}
      <button
        className="inline-flex items-center gap-1 hover:text-fg"
        onClick={() => {
          void navigator.clipboard.writeText(message.content);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        }}
      >
        <Copy className="size-3" /> {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}
