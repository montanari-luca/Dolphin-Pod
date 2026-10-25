import { AnimatePresence, motion } from "motion/react";
import { ArrowUp, MessageCircleQuestion, PenLine } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { AgentAvatar } from "@/components/agent";
import { Button, Kbd } from "@/components/ui";
import { cn } from "@/lib/cn";
import { useApp, type PendingQuestion } from "@/store/app";

/** An agent's question: up to four choices it wrote, plus an "Other" field the user types into in place. */
export function QuestionPanel({ chatId }: { chatId: string }) {
  const question = useApp((s) => s.questions.find((q) => q.chatId === chatId));
  return (
    <AnimatePresence initial={false} mode="wait">
      {question && <Question key={question.id} q={question} />}
    </AnimatePresence>
  );
}

function Question({ q }: { q: PendingQuestion }) {
  const api = useApp((s) => s.api);
  const agent = useApp((s) => s.agents.find((a) => a.id === q.agentId));
  const [typing, setTyping] = useState(false);
  const [custom, setCustom] = useState("");
  const input = useRef<HTMLInputElement>(null);

  const choose = (label: string) => void api?.answerQuestion(q.id, label, false);
  const submitCustom = () => {
    const text = custom.trim();
    if (text) void api?.answerQuestion(q.id, text, true);
  };

  useEffect(() => {
    if (typing) input.current?.focus();
  }, [typing]);

  // 1-4 pick an option, unless the user is typing somewhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = document.activeElement;
      if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || e.metaKey || e.ctrlKey || e.altKey) return;
      const i = Number(e.key) - 1;
      if (Number.isInteger(i) && i >= 0 && i < q.options.length) {
        e.preventDefault();
        choose(q.options[i].label);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [q.id, api]);

  return (
    <motion.div
      role="group"
      aria-label="Question from an agent"
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 12 }}
      transition={{ type: "spring", stiffness: 420, damping: 36 }}
      className="mx-4 mb-1 shrink-0"
    >
      <div className="glass-float rounded-xl p-3.5">
        <div className="flex items-start gap-2.5">
          {agent ? <AgentAvatar agent={agent} size={24} /> : <MessageCircleQuestion className="mt-0.5 size-5 text-muted" />}
          <div className="selectable min-w-0 flex-1">
            <div className="text-[11.5px] text-faint">{agent ? `${agent.name} is asking` : "Question"}</div>
            <div className="text-[14px] font-medium leading-snug text-fg">{q.question}</div>
          </div>
          <Button size="sm" variant="ghost" onClick={() => void api?.skipQuestion(q.id)}>
            Skip
          </Button>
        </div>

        <div className="mt-3 flex flex-col gap-1.5">
          {q.options.map((o, i) => (
            <button
              key={`${i}-${o.label}`}
              onClick={() => choose(o.label)}
              className="no-drag group flex items-start gap-2.5 rounded-md border border-line px-3 py-2 text-left transition-ui hover:border-line-strong hover:bg-hover focus-visible:border-[var(--focus-ring)] focus-visible:outline-none"
            >
              <Kbd>{i + 1}</Kbd>
              <span className="min-w-0 flex-1">
                <span className="block text-[13px] font-medium text-fg">{o.label}</span>
                {o.description && <span className="block text-[12px] leading-snug text-muted">{o.description}</span>}
              </span>
            </button>
          ))}

          {typing ? (
            <div className="flex items-center gap-2 rounded-md border border-[var(--focus-ring)] bg-input py-1 pl-3 pr-1">
              <PenLine className="size-3.5 shrink-0 text-muted" />
              <input
                ref={input}
                value={custom}
                placeholder="Type your own answer"
                maxLength={2000}
                className="h-7 min-w-0 flex-1 bg-transparent text-[13px] text-fg placeholder:text-faint outline-none"
                onChange={(e) => setCustom(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.nativeEvent.isComposing) {
                    e.preventDefault();
                    submitCustom();
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    e.stopPropagation();
                    setTyping(false);
                  }
                }}
              />
              <button
                onClick={submitCustom}
                disabled={!custom.trim()}
                aria-label="Send answer"
                className={cn(
                  "no-drag inline-flex size-7 shrink-0 items-center justify-center rounded-md text-white transition-ui disabled:opacity-35",
                  "[background:var(--brand-gradient)] hover:brightness-110",
                )}
              >
                <ArrowUp className="size-4" strokeWidth={2.4} />
              </button>
            </div>
          ) : (
            <button
              onClick={() => setTyping(true)}
              className="no-drag flex items-center gap-2.5 rounded-md border border-dashed border-line px-3 py-2 text-left text-[13px] text-muted transition-ui hover:border-line-strong hover:bg-hover hover:text-fg"
            >
              <PenLine className="size-3.5" />
              Other…
            </button>
          )}
        </div>
      </div>
    </motion.div>
  );
}
