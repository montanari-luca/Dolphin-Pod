import { useVirtualizer } from "@tanstack/react-virtual";
import { AnimatePresence, motion } from "motion/react";
import { ArrowDown } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Chat, Message } from "@dolphin/shared";
import { AgentAvatar } from "@/components/agent";
import { chatIcon } from "@/lib/chatIcons";
import { useApp } from "@/store/app";
import { MessageItem } from "./MessageItem";

const EMPTY: Message[] = [];

const SUGGESTIONS: Record<string, string[]> = {
  Coding: ["How do I implement a function in Java?", "Review this function for edge cases", "@Jordan why does my build fail with a linker error?"],
  General: ["What can this pod do?", "Explain how routing works here", "Give me three ideas for a weekend project"],
  "Long Horizon Agentic": ["Make a plan to add user accounts to my app", "Plan a migration from REST to GraphQL"],
  Learning: ["Teach me how recursion works", "Help me understand Big-O notation"],
  Creative: ["Write a short poem about the sea", "@Sarah draw a dolphin made of glass"],
};

function isGrouped(prev: Message | undefined, m: Message) {
  if (!prev) return false;
  if (prev.kind === "ack" || prev.kind === "notice" || m.kind === "ack" || m.kind === "notice") return false;
  return prev.agentId === m.agentId && m.createdAt - prev.createdAt < 5 * 60_000;
}

export function MessageList({ chat, onSuggest }: { chat: Chat; onSuggest: (text: string) => void }) {
  const messages = useApp((s) => s.messages[chat.id] ?? EMPTY);
  const agents = useApp((s) => s.agents);
  const parent = useRef<HTMLDivElement>(null);
  const [atBottom, setAtBottom] = useState(true);
  const stick = useRef(true);

  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => parent.current,
    estimateSize: (i) => (messages[i]?.kind === "ack" || messages[i]?.kind === "notice" ? 36 : 96),
    overscan: 8,
    getItemKey: (i) => messages[i]?.id ?? i,
  });

  const lastContentLen = messages.length ? messages[messages.length - 1].content.length + (messages[messages.length - 1].reasoning?.length ?? 0) : 0;

  // Follow new content while the user is at the bottom.
  useLayoutEffect(() => {
    if (stick.current && messages.length > 0) virtualizer.scrollToIndex(messages.length - 1, { align: "end" });
  }, [messages.length, lastContentLen, virtualizer]);

  useEffect(() => {
    stick.current = true;
    setAtBottom(true);
  }, [chat.id]);

  const onScroll = () => {
    const el = parent.current;
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    stick.current = near;
    setAtBottom(near);
  };

  if (messages.length === 0) {
    const Icon = chatIcon(chat.icon);
    const members = agents.filter((a) => chat.agentIds.includes(a.id) && a.role !== "orchestrator");
    return (
      <div className="flex flex-1 flex-col items-center justify-center px-8 text-center">
        <motion.div initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, ease: [0.22, 1, 0.36, 1] }} className="flex flex-col items-center">
          <div className="mb-4 flex size-14 items-center justify-center rounded-xl border border-line bg-surface-2 shadow-sm">
            <Icon className="size-6 text-muted" />
          </div>
          <h2 className="text-[20px] font-semibold tracking-tight text-fg">Welcome to {chat.name}</h2>
          <p className="mt-1.5 max-w-md text-[13px] text-muted leading-relaxed">
            Ask anything. Roger routes your message to the right specialist, or type <span className="font-mono text-fg">@</span> to talk to one directly.
          </p>
          <div className="mt-4 flex -space-x-1.5">
            {members.map((a) => (
              <AgentAvatar key={a.id} agent={a} size={28} className="ring-2 ring-[var(--bg-base)] rounded-full" />
            ))}
          </div>
          <div className="mt-6 flex max-w-xl flex-wrap justify-center gap-2">
            {(SUGGESTIONS[chat.name] ?? SUGGESTIONS.General).map((s) => (
              <button key={s} onClick={() => onSuggest(s)} className="rounded-full border border-line bg-surface-0 px-3.5 py-1.5 text-[12.5px] text-muted transition-ui hover:border-line-strong hover:bg-hover hover:text-fg">
                {s}
              </button>
            ))}
          </div>
        </motion.div>
      </div>
    );
  }

  return (
    <div className="relative min-h-0 flex-1">
      <div ref={parent} onScroll={onScroll} className="h-full overflow-y-auto pt-3 pb-4" style={{ contain: "strict" }}>
        <div style={{ height: virtualizer.getTotalSize(), position: "relative", width: "100%" }}>
          {virtualizer.getVirtualItems().map((v) => {
            const m = messages[v.index];
            return (
              <div key={v.key} data-index={v.index} ref={virtualizer.measureElement} style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${v.start}px)` }}>
                <MessageItem message={m} agent={agents.find((a) => a.id === m.agentId)} grouped={isGrouped(messages[v.index - 1], m)} />
              </div>
            );
          })}
        </div>
      </div>
      <AnimatePresence>
        {!atBottom && (
          <motion.button
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 8 }}
            onClick={() => {
              stick.current = true;
              virtualizer.scrollToIndex(messages.length - 1, { align: "end", behavior: "smooth" });
            }}
            className="glass-float absolute bottom-3 left-1/2 -translate-x-1/2 inline-flex items-center gap-1.5 rounded-full px-3 h-8 text-[12px] text-fg"
          >
            <ArrowDown className="size-3.5" /> Jump to latest
          </motion.button>
        )}
      </AnimatePresence>
    </div>
  );
}
