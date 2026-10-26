import { Lock, LockOpen, ShieldQuestion } from "lucide-react";
import type { Agent, Chat, PermissionLevel } from "@dolphin/shared";
import { AgentAvatar, AgentHoverCard, AgentNameText } from "@/components/agent";
import { RoleBadge, UserAvatar, UserHoverCard, UserNameText, useUser } from "@/components/user";
import { cn } from "@/lib/cn";
import { useApp, type Activity } from "@/store/app";

const ACTIVITY_TEXT: Record<string, string> = {
  routing: "routing…",
  thinking: "thinking…",
  writing: "writing…",
  using_tool: "using tools…",
  stuck: "seems stuck",
};

/** Read-only → asks before acting → full → read-only. */
const NEXT_LEVEL: Record<PermissionLevel, PermissionLevel> = { read_only: "ask", ask: "full", full: "read_only" };
const LEVEL_HINT: Record<PermissionLevel, string> = {
  read_only: "Read-only. Click to let this agent act after asking you first.",
  ask: "Asks before every file change and command. Click to grant full permissions.",
  full: "Full agentic permissions. Click to make read-only.",
};

function Member({ agent, chat, act }: { agent: Agent; chat: Chat; act?: Activity }) {
  const providers = useApp((s) => s.providers);
  const grant = useApp((s) => s.grants[chat.id]?.find((g) => g.agentId === agent.id));
  const api = useApp((s) => s.api);
  const run = useApp((s) => s.run);
  const busy = !!act && act.activity !== "idle";
  const provider = providers.find((p) => p.id === agent.model?.providerId);
  const level: PermissionLevel = grant?.level ?? "read_only";
  const full = level === "full";
  const status = busy ? (act?.detail && act.activity !== "thinking" ? act.detail : ACTIVITY_TEXT[act!.activity]) : agent.model ? `${provider?.name ?? ""} · ${agent.model.modelId}` : "no model";
  return (
    <AgentHoverCard agent={agent} side="left">
      <div className="group flex items-center gap-2.5 rounded-sm px-2 py-1.5 hover:bg-hover transition-ui">
        <AgentAvatar agent={agent} size={30} busy={busy} />
        <div className="min-w-0 flex-1">
          <AgentNameText agent={agent} className="block text-[13px] leading-tight" />
          <div className={cn("truncate text-[11px] leading-tight mt-0.5", busy ? "shimmer-text" : agent.model ? "text-faint" : "text-warning")}>{status}</div>
        </div>
        {chat.workspacePath && agent.role === "specialist" && (
          <button
            className={cn("size-6 inline-flex items-center justify-center rounded transition-ui", full ? "text-success hover:bg-success/15" : level === "ask" ? "text-warning hover:bg-warning/15" : "text-faint opacity-60 group-hover:opacity-100 hover:bg-active")}
            title={LEVEL_HINT[level]}
            onClick={(e) => {
              e.stopPropagation();
              if (api) void run(api.grantPermission(chat.id, agent.id, NEXT_LEVEL[level]));
            }}
          >
            {full ? <LockOpen className="size-3.5" /> : level === "ask" ? <ShieldQuestion className="size-3.5" /> : <Lock className="size-3.5" />}
          </button>
        )}
      </div>
    </AgentHoverCard>
  );
}

/** The user, pinned at the top of the member list with their roles. */
function Me() {
  const { roles } = useUser();
  const setView = useApp((s) => s.setView);
  return (
    <div className="mb-3">
      <div className="px-2 pb-1 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-faint">You</div>
      <UserHoverCard side="left">
        <button onClick={() => setView("settings", "profile")} className="flex w-full items-center gap-2.5 rounded-sm px-2 py-1.5 text-left hover:bg-hover transition-ui">
          <UserAvatar size={30} />
          <div className="min-w-0 flex-1">
            <UserNameText className="text-[13px] leading-tight" />
            <div className="mt-1 flex flex-wrap gap-1">
              {roles.slice(0, 3).map((r) => (
                <RoleBadge key={r.id} role={r} className="h-[18px] text-[10.5px]" />
              ))}
              {roles.length > 3 && <span className="text-[10.5px] text-faint">+{roles.length - 3}</span>}
            </div>
          </div>
        </button>
      </UserHoverCard>
    </div>
  );
}

export function MembersPanel({ chat }: { chat: Chat }) {
  const agents = useApp((s) => s.agents);
  const activity = useApp((s) => s.activity[chat.id]);
  const members = agents.filter((a) => chat.agentIds.includes(a.id));
  const crew = members.filter((a) => a.role !== "specialist");
  const specialists = members.filter((a) => a.role === "specialist");
  const Section = ({ title, list }: { title: string; list: Agent[] }) => (
    <div className="mb-3">
      <div className="px-2 pb-1 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-faint">
        {title} — {list.length}
      </div>
      {list.map((a) => (
        <Member key={a.id} agent={a} chat={chat} act={activity?.[a.id]} />
      ))}
    </div>
  );
  return (
    <aside className="glass flex h-full w-[240px] flex-col overflow-hidden rounded-xl">
      <div className="drag-region h-[60px] shrink-0 border-b border-line flex items-center px-4">
        <span className="text-[13px] font-semibold text-fg">Members</span>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        <Me />
        <Section title="Moderation" list={crew} />
        <Section title="Specialists" list={specialists} />
      </div>
    </aside>
  );
}
