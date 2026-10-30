import { Check, GitMerge, Loader2 } from "lucide-react";
import { useState } from "react";
import type { Agent, Message } from "@dolphin/shared";
import { AgentAvatar } from "@/components/agent";
import { Markdown } from "@/components/Markdown";
import { Badge, Button, Input } from "@/components/ui";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";

export type DesignChoice = {
  group: string;
  state: "pending" | "decided";
  options: { label: "A" | "B"; agentId: string; messageId: string }[];
  picked?: "a" | "b" | "merge";
  note?: string | null;
  mergedMessageId?: string;
};

/** The two design ideas side by side, with the buttons to pick one or have them merged. The run waits on this card. */
export function DesignChoiceCard({ message, choice }: { message: Message; choice: DesignChoice }) {
  const api = useApp((s) => s.api);
  const run = useApp((s) => s.run);
  const agents = useApp((s) => s.agents);
  const messages = useApp((s) => s.messages[message.chatId]);
  const [busy, setBusy] = useState(false);
  const [merging, setMerging] = useState(false);
  const [note, setNote] = useState("");

  const decided = choice.state === "decided";
  const answer = async (pick: "a" | "b" | "merge") => {
    if (!api) return;
    setBusy(true);
    await run(api.chooseDesign(message.id, pick, pick === "merge" ? note.trim() || undefined : undefined));
    setBusy(false);
    setMerging(false);
  };
  const merged = choice.mergedMessageId ? messages?.find((m) => m.id === choice.mergedMessageId) : undefined;

  return (
    <div className="mt-2.5 rounded-lg border border-line bg-surface-2/60 p-3">
      <div className="grid gap-3 sm:grid-cols-2">
        {choice.options.map((o) => {
          const idea = messages?.find((m) => m.id === o.messageId);
          const author: Agent | undefined = agents.find((a) => a.id === o.agentId);
          const chosen = decided && choice.picked === o.label.toLowerCase();
          return (
            <div key={o.label} className={cn("min-w-0 rounded-md border bg-bg p-2.5", chosen ? "border-success/50" : "border-line", decided && !chosen && choice.picked !== "merge" && "opacity-60")}>
              <div className="mb-1.5 flex items-center gap-2">
                {author && <AgentAvatar agent={author} size={20} />}
                <span className="text-[12.5px] font-medium">{author?.name ?? "Designer"}</span>
                <Badge tone={chosen ? "success" : "accent"}>Idea {o.label}</Badge>
                {chosen && <Check className="ml-auto size-3.5 text-success" />}
              </div>
              <div className="max-h-72 overflow-auto text-[13px] text-fg">{idea ? <Markdown text={idea.content} /> : <span className="text-faint">Not loaded</span>}</div>
            </div>
          );
        })}
      </div>

      {!decided && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {choice.options.map((o) => {
            const author = agents.find((a) => a.id === o.agentId);
            return (
              <Button key={o.label} variant="primary" size="sm" disabled={busy || merging} onClick={() => answer(o.label.toLowerCase() as "a" | "b")}>
                <Check className="size-3.5" /> Pick {o.label}{author ? ` (${author.name})` : ""}
              </Button>
            );
          })}
          <Button size="sm" disabled={busy} onClick={() => setMerging((v) => !v)}>
            <GitMerge className="size-3.5" /> Merge…
          </Button>
        </div>
      )}

      {!decided && merging && (
        <div className="mt-2 flex items-center gap-2">
          <Input
            autoFocus
            value={note}
            onChange={(e) => setNote(e.target.value)}
            placeholder="How to merge them, e.g. A's layout with B's colors"
            onKeyDown={(e) => e.key === "Enter" && !busy && answer("merge")}
          />
          <Button variant="primary" size="sm" loading={busy} onClick={() => answer("merge")}>Merge</Button>
        </div>
      )}

      {decided && (
        <div className="mt-2.5 flex items-center gap-1.5 text-[12.5px] text-muted">
          {choice.picked === "merge" ? (
            <>
              {merged ? <Check className="size-3.5 text-success" /> : <Loader2 className="size-3.5 animate-spin" />}
              <span>{merged ? "Merged into one design." : "Merging the two designs…"}{choice.note ? ` “${choice.note}”` : ""}</span>
            </>
          ) : (
            <>
              <Check className="size-3.5 text-success" />
              <span>You picked idea {choice.picked?.toUpperCase()}.</span>
            </>
          )}
        </div>
      )}
    </div>
  );
}
