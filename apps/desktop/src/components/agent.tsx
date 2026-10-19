import * as HoverCard from "@radix-ui/react-hover-card";
import { motion } from "motion/react";
import { BookOpen, Bug, Code2, Feather, GraduationCap, Image as ImageIcon, MessagesSquare, Network, Rocket, Settings2, ShieldCheck, Sparkles, type LucideIcon } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import type { Agent } from "@dolphin/shared";
import { cn } from "@/lib/cn";
import { cssBackground, onColor, readableTextBackground } from "@/lib/color";
import { formatDuration, formatTokens } from "@/lib/format";
import { useResolvedTheme } from "@/lib/theme";
import { useApp } from "@/store/app";

export const SPECIALTY_ICON: Record<string, LucideIcon> = {
  moderator: MessagesSquare,
  orchestrator: Network,
  coder: Code2,
  long_horizon: Rocket,
  security: ShieldCheck,
  debug: Bug,
  creative: Feather,
  docs: BookOpen,
  image: ImageIcon,
  teacher: GraduationCap,
};

export const SPECIALTY_LABEL: Record<string, string> = {
  moderator: "Moderator",
  orchestrator: "Orchestrator",
  coder: "Coding",
  long_horizon: "Long horizon",
  security: "Security",
  debug: "Debugging",
  creative: "Creative writing",
  docs: "Documentation",
  image: "Images",
  teacher: "Teaching",
};

export function AgentAvatar({ agent, size = 32, busy, className }: { agent: Agent; size?: number; busy?: boolean; className?: string }) {
  const Icon = SPECIALTY_ICON[agent.specialty] ?? Sparkles;
  return (
    <span className={cn("relative inline-flex shrink-0", className)} style={{ width: size, height: size }}>
      {busy && (
        <motion.span
          className="absolute -inset-[3px] rounded-full opacity-70"
          style={{ background: cssBackground(agent.color) }}
          animate={{ opacity: [0.25, 0.75, 0.25], scale: [1, 1.06, 1] }}
          transition={{ duration: 1.6, repeat: Infinity, ease: "easeInOut" }}
        />
      )}
      <span
        className="relative inline-flex size-full items-center justify-center rounded-full ring-1 ring-white/15 shadow-[inset_0_1px_0_rgba(255,255,255,0.35)]"
        style={{ background: cssBackground(agent.color), color: onColor(agent.color) }}
      >
        <Icon style={{ width: size * 0.5, height: size * 0.5 }} strokeWidth={2.1} />
      </span>
    </span>
  );
}

export function UserAvatar({ size = 32 }: { size?: number }) {
  return (
    <span className="inline-flex shrink-0 items-center justify-center rounded-full bg-surface-2 border border-line-strong text-muted font-semibold" style={{ width: size, height: size, fontSize: size * 0.42 }}>
      You
    </span>
  );
}

/** The agent's name in its colors, adjusted for contrast against the current theme. */
export function AgentNameText({ agent, className, at, children }: { agent: Agent; className?: string; at?: boolean; children?: ReactNode }) {
  const theme = useResolvedTheme();
  const bg = readableTextBackground(agent.color, theme);
  const style = bg.startsWith("linear")
    ? { backgroundImage: bg, color: "transparent", WebkitBackgroundClip: "text", backgroundClip: "text" }
    : { color: bg };
  return (
    <span className={cn("font-semibold", className)} style={style}>
      {children ?? (at ? `@${agent.name}` : agent.name)}
    </span>
  );
}

/** Name with the hover card. */
export function AgentName(props: { agent: Agent; className?: string; at?: boolean; children?: ReactNode }) {
  return (
    <AgentHoverCard agent={props.agent}>
      <span className="cursor-default">
        <AgentNameText {...props} />
      </span>
    </AgentHoverCard>
  );
}

export function AgentMention({ agent }: { agent: Agent }) {
  return (
    <AgentHoverCard agent={agent}>
      <span className="inline-flex items-center rounded-[6px] px-1 -mx-0.5 bg-accent-soft/80 hover:bg-accent-soft cursor-default">
        <AgentNameText agent={agent} at />
      </span>
    </AgentHoverCard>
  );
}

function useLiveElapsed(since: number | undefined, running: boolean) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, [running]);
  return running && since ? now - since : null;
}

export function AgentCardContent({ agent }: { agent: Agent }) {
  const usage = useApp((s) => s.usage[agent.id]);
  const providers = useApp((s) => s.providers);
  const activeChatId = useApp((s) => s.activeChatId);
  const act = useApp((s) => (activeChatId ? s.activity[activeChatId]?.[agent.id] : undefined));
  const setView = useApp((s) => s.setView);
  const running = !!act && act.activity !== "idle";
  const live = useLiveElapsed(act?.since, running);
  const provider = providers.find((p) => p.id === agent.model?.providerId);
  const Stat = ({ label, value, sub }: { label: string; value: string; sub?: string }) => (
    <div className="rounded-md bg-hover/70 border border-line px-2.5 py-2">
      <div className="text-[10.5px] uppercase tracking-wide text-faint font-medium">{label}</div>
      <div className="text-[14px] font-semibold text-fg tabular-nums">{value}</div>
      {sub && <div className="text-[10.5px] text-faint tabular-nums">{sub}</div>}
    </div>
  );
  return (
    <div className="w-[300px]">
      <div className="relative h-14 rounded-t-lg overflow-hidden" style={{ background: cssBackground(agent.color) }}>
        <div className="absolute inset-0 bg-gradient-to-b from-transparent to-black/25" />
      </div>
      <div className="px-4 pb-4 -mt-7">
        <AgentAvatar agent={agent} size={52} busy={running} className="ring-4 ring-[var(--surface-float)] rounded-full" />
        <div className="mt-2 flex items-baseline gap-2">
          <AgentNameText agent={agent} className="text-[16px] leading-tight" />
          <span className="text-[11.5px] text-muted">{SPECIALTY_LABEL[agent.specialty] ?? agent.specialty}</span>
        </div>
        <p className="mt-1.5 text-[12.5px] text-muted leading-relaxed">{agent.job}</p>
        <div className="mt-2 flex items-center gap-1.5 text-[11.5px] text-faint truncate">
          <span className="size-1.5 rounded-full shrink-0" style={{ background: agent.model ? "var(--success)" : "var(--warning)" }} />
          {agent.model ? (
            <span className="truncate">
              {provider?.name ?? "Provider"} · <span className="font-mono text-muted">{agent.model.modelId}</span>
            </span>
          ) : (
            <span>No model configured</span>
          )}
        </div>
        <div className="mt-3 grid grid-cols-2 gap-2">
          <Stat label="Tokens (all time)" value={formatTokens((usage?.tokensIn ?? 0) + (usage?.tokensOut ?? 0))} sub={`${formatTokens(usage?.tokensIn ?? 0)} in · ${formatTokens(usage?.tokensOut ?? 0)} out`} />
          <Stat label={running ? "Current query" : "Last query"} value={formatDuration(live ?? usage?.lastQueryMs)} sub={running ? act?.activity.replace("_", " ") : `${usage?.requests ?? 0} requests`} />
          <Stat label="Thinking time" value={formatDuration(usage?.thinkingMs ?? 0)} />
          <Stat label="Working time" value={formatDuration(usage?.workingMs ?? 0)} />
        </div>
        <button
          className="mt-3 inline-flex items-center gap-1.5 text-[12px] text-muted hover:text-fg transition-ui"
          onClick={() => setView("settings", "agents", agent.id)}
        >
          <Settings2 className="size-3.5" /> Configure {agent.name}
        </button>
      </div>
    </div>
  );
}

export function AgentHoverCard({ agent, children, side = "top" }: { agent: Agent; children: ReactNode; side?: "top" | "right" | "bottom" | "left" }) {
  return (
    <HoverCard.Root openDelay={280} closeDelay={120}>
      <HoverCard.Trigger asChild>{children}</HoverCard.Trigger>
      <HoverCard.Portal>
        <HoverCard.Content side={side} sideOffset={8} collisionPadding={12} className="z-50 outline-none">
          <motion.div
            className="glass-float rounded-lg overflow-hidden"
            initial={{ opacity: 0, y: side === "top" ? 6 : -6, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            transition={{ type: "spring", stiffness: 500, damping: 34 }}
          >
            <AgentCardContent agent={agent} />
          </motion.div>
        </HoverCard.Content>
      </HoverCard.Portal>
    </HoverCard.Root>
  );
}
