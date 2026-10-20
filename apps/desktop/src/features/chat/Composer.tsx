import { AnimatePresence, motion } from "motion/react";
import { ArrowUp, FileText, Image as ImageIcon, Loader2, Paperclip, Square, X } from "lucide-react";
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import type { Attachment, Chat } from "@dolphin/shared";
import { AgentAvatar, AgentHoverCard, AgentNameText, SPECIALTY_LABEL } from "@/components/agent";
import { IconButton, Kbd } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { cn } from "@/lib/cn";
import { formatBytes } from "@/lib/format";
import { filterAgents, insertMention, mentionAt } from "@/lib/mentions";
import { safeGet, safeSet, useApp } from "@/store/app";

export interface ComposerHandle {
  setText(t: string): void;
  addFiles(files: File[]): void;
  addPaths(paths: string[]): void;
  focus(): void;
}

type Staged = { key: string; name: string; size: number; kind: string; attachment?: Attachment; error?: string };

export const Composer = forwardRef<ComposerHandle, { chat: Chat; busy: boolean }>(function Composer({ chat, busy }, ref) {
  const api = useApp((s) => s.api);
  const agents = useApp((s) => s.agents);
  const toast = useApp((s) => s.toast);
  const threshold = Number(useApp((s) => s.settings["chat.paste_threshold"]) ?? 6000) || 6000;
  const [text, setText] = useState(() => safeGetDraft(chat.id));
  const [staged, setStaged] = useState<Staged[]>([]);
  const [caret, setCaret] = useState(0);
  const [menuIndex, setMenuIndex] = useState(0);
  const [menuDismissed, setMenuDismissed] = useState(false);
  const [sending, setSending] = useState(false);
  const ta = useRef<HTMLTextAreaElement>(null);
  const pasteSeq = useRef(1);

  useEffect(() => {
    setText(safeGetDraft(chat.id));
    setStaged([]);
    ta.current?.focus();
  }, [chat.id]);
  useEffect(() => safeSetDraft(chat.id, text), [chat.id, text]);

  // Esc stops a running chat even when the input isn't focused (unless a dialog owns Esc).
  useEffect(() => {
    if (!busy) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.target === ta.current || document.querySelector("[role='dialog']")) return;
      void api?.stopChat(chat.id);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, api, chat.id]);

  // Auto-grow.
  useEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, window.innerHeight * 0.4)}px`;
  }, [text]);

  const members = useMemo(() => agents.filter((a) => chat.agentIds.includes(a.id)), [agents, chat.agentIds]);
  const mention = menuDismissed ? null : mentionAt(text, caret);
  const options = mention ? filterAgents(members, mention.query) : [];
  useEffect(() => setMenuIndex(0), [mention?.query]);

  const stageBytes = useCallback(
    async (name: string, mime: string, bytes: Uint8Array, kind: string) => {
      if (!api) return;
      const key = crypto.randomUUID();
      setStaged((s) => [...s, { key, name, size: bytes.length, kind }]);
      try {
        const attachment = await api.stageAttachment(name, mime, bytes);
        setStaged((s) => s.map((x) => (x.key === key ? { ...x, attachment } : x)));
      } catch (e) {
        setStaged((s) => s.map((x) => (x.key === key ? { ...x, error: errorText(e) } : x)));
      }
    },
    [api],
  );

  const addFiles = useCallback(
    (files: File[]) => {
      for (const f of files) {
        void f.arrayBuffer().then((b) => stageBytes(f.name || "file", f.type, new Uint8Array(b), f.type.startsWith("image/") ? "image" : "file"));
      }
    },
    [stageBytes],
  );

  const addPaths = useCallback(
    (paths: string[]) => {
      if (!api) return;
      for (const p of paths) {
        const key = crypto.randomUUID();
        const name = p.split(/[\\/]/).pop() ?? p;
        setStaged((s) => [...s, { key, name, size: 0, kind: /\.(png|jpe?g|gif|webp|heic)$/i.test(name) ? "image" : "file" }]);
        api.stageAttachmentPath(p).then(
          (attachment) => setStaged((s) => s.map((x) => (x.key === key ? { ...x, attachment, size: attachment.size } : x))),
          (e) => setStaged((s) => s.map((x) => (x.key === key ? { ...x, error: errorText(e) } : x))),
        );
      }
    },
    [api],
  );

  useImperativeHandle(ref, () => ({
    setText: (t) => {
      setText(t);
      requestAnimationFrame(() => {
        ta.current?.focus();
        ta.current?.setSelectionRange(t.length, t.length);
        setCaret(t.length);
      });
    },
    addFiles,
    addPaths,
    focus: () => ta.current?.focus(),
  }));

  const pending = staged.some((s) => !s.attachment && !s.error);
  const canSend = (text.trim().length > 0 || staged.some((s) => s.attachment)) && !pending && !sending;

  const send = async () => {
    if (!api || !canSend) return;
    setSending(true);
    const ids = staged.filter((s) => s.attachment).map((s) => s.attachment!.id);
    try {
      await api.sendMessage(chat.id, text, ids);
      setText("");
      setStaged([]);
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setSending(false);
      ta.current?.focus();
    }
  };

  const choose = (i: number) => {
    const a = options[i];
    if (!a || !mention) return;
    const r = insertMention(text, mention.start, caret, a.name);
    setText(r.text);
    requestAnimationFrame(() => {
      ta.current?.setSelectionRange(r.caret, r.caret);
      setCaret(r.caret);
    });
  };

  return (
    <div className="relative px-4 pb-5 pt-1">
      <AnimatePresence>
        {options.length > 0 && (
          <motion.div
            initial={{ opacity: 0, y: 6, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 4 }}
            transition={{ duration: 0.14 }}
            className="glass-float absolute bottom-full left-4 z-30 mb-2 w-80 overflow-hidden rounded-lg p-1"
            role="listbox"
          >
            <div className="px-2.5 pt-1.5 pb-1 text-[10.5px] font-semibold uppercase tracking-wider text-faint">Mention an agent</div>
            {options.map((a, i) => (
              <AgentHoverCard key={a.id} agent={a} side="right">
                <button
                  role="option"
                  aria-selected={i === menuIndex}
                  onMouseEnter={() => setMenuIndex(i)}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    choose(i);
                  }}
                  className={cn("flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left", i === menuIndex ? "bg-active" : "")}
                >
                  <AgentAvatar agent={a} size={24} />
                  <AgentNameText agent={a} className="text-[13px]" />
                  <span className="ml-auto text-[11px] text-faint">{SPECIALTY_LABEL[a.specialty]}</span>
                </button>
              </AgentHoverCard>
            ))}
          </motion.div>
        )}
      </AnimatePresence>

      <div className="glass rounded-xl transition-ui focus-within:border-line-strong">
        {staged.length > 0 && (
          <div className="flex flex-wrap gap-2 px-3 pt-3">
            {staged.map((s) => (
              <div key={s.key} className={cn("flex items-center gap-2 rounded-md border bg-hover/60 py-1 pl-2 pr-1 text-[12px]", s.error ? "border-danger/40" : "border-line")}>
                {!s.attachment && !s.error ? <Loader2 className="size-3.5 animate-spin text-faint" /> : s.kind === "image" ? <ImageIcon className="size-3.5 text-muted" /> : <FileText className="size-3.5 text-muted" />}
                <span className="max-w-44 truncate text-fg" title={s.error ?? s.name}>{s.name}</span>
                <span className="text-faint">{s.error ? "failed" : s.size ? formatBytes(s.size) : ""}</span>
                <button className="size-5 inline-flex items-center justify-center rounded text-faint hover:bg-active hover:text-fg" onClick={() => setStaged((x) => x.filter((y) => y.key !== s.key))} aria-label={`Remove ${s.name}`}>
                  <X className="size-3" />
                </button>
              </div>
            ))}
          </div>
        )}
        <div className="flex items-end gap-1.5 p-2">
          <IconButton
            label="Attach files"
            onClick={async () => {
              if (!api) return;
              if (api.isTauri) addPaths(await api.pickFiles());
              else {
                const input = document.createElement("input");
                input.type = "file";
                input.multiple = true;
                input.onchange = () => addFiles([...(input.files ?? [])]);
                input.click();
              }
            }}
          >
            <Paperclip className="size-[17px]" />
          </IconButton>
          <textarea
            ref={ta}
            rows={1}
            value={text}
            placeholder={`Message ${chat.name}  ·  @ to mention an agent`}
            className="max-h-[40vh] min-h-[34px] flex-1 resize-none bg-transparent px-1.5 py-[7px] text-[14px] leading-[20px] text-fg placeholder:text-faint outline-none"
            onChange={(e) => {
              setText(e.target.value);
              setCaret(e.target.selectionStart);
              setMenuDismissed(false);
            }}
            onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
            onKeyDown={(e) => {
              if (options.length > 0) {
                if (e.key === "ArrowDown") return (e.preventDefault(), setMenuIndex((i) => (i + 1) % options.length));
                if (e.key === "ArrowUp") return (e.preventDefault(), setMenuIndex((i) => (i - 1 + options.length) % options.length));
                if (e.key === "Enter" || e.key === "Tab") return (e.preventDefault(), choose(menuIndex));
                if (e.key === "Escape") return (e.preventDefault(), setMenuDismissed(true));
              }
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void send();
              }
              if (e.key === "Escape" && busy) {
                e.preventDefault();
                void api?.stopChat(chat.id);
              }
            }}
            onPaste={(e) => {
              const files = [...e.clipboardData.files];
              if (files.length) {
                e.preventDefault();
                addFiles(files);
                return;
              }
              const pasted = e.clipboardData.getData("text/plain");
              if (pasted.length > threshold) {
                // Long pastes become a .txt attachment instead of flooding the input.
                e.preventDefault();
                const n = pasteSeq.current++;
                void stageBytes(`pasted-text-${n}.txt`, "text/plain", new TextEncoder().encode(pasted), "file");
                toast("info", `Pasted ${pasted.length.toLocaleString()} characters as a text attachment.`);
              }
            }}
          />
          {busy ? (
            <button
              onClick={() => api?.stopChat(chat.id)}
              className="no-drag inline-flex size-[34px] shrink-0 items-center justify-center rounded-md bg-active text-fg transition-ui hover:bg-danger/20 hover:text-danger"
              title="Stop (Esc)"
              aria-label="Stop"
            >
              <Square className="size-3.5 fill-current" />
            </button>
          ) : (
            <button
              onClick={() => void send()}
              disabled={!canSend}
              className={cn(
                "no-drag inline-flex size-[34px] shrink-0 items-center justify-center rounded-md text-white transition-ui disabled:opacity-35",
                "[background:var(--brand-gradient)] shadow-[0_6px_18px_-8px_rgba(80,120,255,0.7)] hover:brightness-110 disabled:shadow-none",
              )}
              aria-label="Send"
              title="Send (Enter)"
            >
              {sending ? <Loader2 className="size-4 animate-spin" /> : <ArrowUp className="size-[18px]" strokeWidth={2.4} />}
            </button>
          )}
        </div>
      </div>
      <div className="mt-2.5 flex justify-between px-1 text-[10.5px] text-faint">
        <span>
          <Kbd>Enter</Kbd> send · <Kbd>Shift</Kbd>+<Kbd>Enter</Kbd> new line · <Kbd>@</Kbd> mention
        </span>
        {busy && <span>Press <Kbd>Esc</Kbd> to stop</span>}
      </div>
    </div>
  );
});

function safeGetDraft(chatId: string) {
  return safeGet(`dp.draft.${chatId}`) ?? "";
}
function safeSetDraft(chatId: string, v: string) {
  safeSet(`dp.draft.${chatId}`, v);
}
