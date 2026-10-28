import { Sparkles } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import type { Agent, PersonaProposal } from "@dolphin/shared";
import { Button } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { useApp } from "@/store/app";

/**
 * A persona change the agent suggested for itself. It only takes effect if you accept it: nothing the agent learns
 * rewrites its persona behind your back.
 */
export function PersonaProposalCard({ agent, blocked }: { agent: Agent; blocked: boolean }) {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const version = useApp((s) => s.proposalVersion[agent.id] ?? 0);
  const [proposal, setProposal] = useState<PersonaProposal | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!api) return;
    try {
      setProposal(await api.personaProposal(agent.id));
    } catch (e) {
      toast("error", errorText(e));
    }
  }, [api, agent.id, toast]);

  // Reload when the agent changes and whenever a suggestion appears or is dealt with.
  useEffect(() => {
    void load();
  }, [load, version]);

  if (!proposal) return null;

  const decide = async (accept: boolean) => {
    if (!api) return;
    setBusy(true);
    try {
      if (accept) {
        await api.acceptPersonaProposal(proposal.id);
        toast("success", `${agent.name}'s persona updated`);
      } else {
        await api.rejectPersonaProposal(proposal.id);
      }
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setBusy(false);
      await load();
    }
  };

  return (
    <section className="space-y-2.5 rounded-md border border-accent/40 bg-accent-soft/40 p-3.5" aria-label={`${agent.name}'s persona suggestion`}>
      <div className="flex items-start gap-2.5">
        <Sparkles className="size-4 shrink-0 mt-0.5 text-accent" />
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium text-fg">{agent.name} suggests a new persona</div>
          {proposal.reason && <p className="text-[12px] leading-relaxed text-muted">{proposal.reason}.</p>}
        </div>
      </div>
      <blockquote className="selectable whitespace-pre-wrap rounded-sm border border-line bg-surface-0 px-3 py-2 text-[12.5px] leading-relaxed text-fg">{proposal.text}</blockquote>
      <p className="text-[11.5px] text-faint">{agent.persona ? "It would replace the current persona." : "This agent has no persona yet."} Nothing changes unless you accept.</p>
      {blocked && <p className="text-[11.5px] text-warning">Save or revert your edits to this agent first, so accepting doesn't discard them.</p>}
      <div className="flex gap-2">
        <Button variant="primary" size="sm" loading={busy} disabled={blocked} onClick={() => void decide(true)}>Accept</Button>
        <Button variant="ghost" size="sm" disabled={busy} onClick={() => void decide(false)}>Dismiss</Button>
      </div>
    </section>
  );
}
