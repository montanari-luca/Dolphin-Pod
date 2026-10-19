import { AnimatePresence, motion } from "motion/react";
import { Brain, ChevronRight } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/cn";
import { formatDuration } from "@/lib/format";

export function ThinkingBlock({ text, live, thinkingMs, startedAt }: { text: string; live: boolean; thinkingMs: number | null; startedAt: number }) {
  const [open, setOpen] = useState(live);
  const [now, setNow] = useState(Date.now());
  const body = useRef<HTMLDivElement>(null);
  const wasLive = useRef(live);
  const [localMs, setLocalMs] = useState<number | null>(null);

  useEffect(() => {
    if (!live) return;
    const t = setInterval(() => setNow(Date.now()), 200);
    return () => clearInterval(t);
  }, [live]);
  // Expanded while thinking; folds away once the answer starts.
  useEffect(() => {
    if (wasLive.current && !live) {
      setOpen(false);
      setLocalMs(Date.now() - startedAt);
    }
    if (!wasLive.current && live) setOpen(true);
    wasLive.current = live;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [live]);
  useEffect(() => {
    if (live && open && body.current) body.current.scrollTop = body.current.scrollHeight;
  }, [text, live, open]);

  const label = live ? `Thinking… ${formatDuration(now - startedAt)}` : thinkingMs != null || localMs != null ? `Thought for ${formatDuration(thinkingMs ?? localMs)}` : "Thoughts";
  return (
    <div className="my-1.5">
      <button
        onClick={() => setOpen((o) => !o)}
        className="group inline-flex items-center gap-1.5 rounded-full border border-line bg-hover/60 pl-2 pr-2.5 h-6 text-[11.5px] text-muted hover:text-fg transition-ui"
      >
        <Brain className="size-3.5" />
        <span className={cn(live && "shimmer-text font-medium")}>{label}</span>
        <ChevronRight className={cn("size-3 transition-transform duration-200", open && "rotate-90")} />
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
            className="overflow-hidden"
          >
            <div ref={body} className="selectable mt-1.5 max-h-60 overflow-y-auto whitespace-pre-wrap border-l-2 border-line-strong pl-3 text-[12.5px] leading-relaxed text-faint italic">
              {text.trim() || "…"}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
