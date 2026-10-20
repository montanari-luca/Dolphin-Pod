import { motion } from "motion/react";
import { CalendarClock, Plus, Settings } from "lucide-react";
import { useState } from "react";
import { IconButton } from "@/components/ui";
import { NewChatDialog } from "@/features/chat/ChatDialogs";
import { chatIcon } from "@/lib/chatIcons";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";

export function Sidebar() {
  const chats = useApp((s) => s.chats);
  const activeChatId = useApp((s) => s.activeChatId);
  const view = useApp((s) => s.view);
  const openChat = useApp((s) => s.openChat);
  const setView = useApp((s) => s.setView);
  const activity = useApp((s) => s.activity);
  const [creating, setCreating] = useState(false);

  return (
    <aside className="drag-region glass relative z-10 flex h-full w-[248px] shrink-0 flex-col rounded-xl overflow-hidden">
      <div className="traffic-pad flex h-[60px] shrink-0 items-end gap-2.5 px-4 pb-2.5">
        <img src="/brand/dolphin-icon.png" alt="" className="size-6 drop-shadow" draggable={false} />
        <span className="text-[14px] font-semibold tracking-tight text-fg">Dolphin Pod</span>
      </div>

      <div className="flex items-center justify-between px-4 pt-3 pb-1.5">
        <span className="text-[11px] font-semibold uppercase tracking-[0.08em] text-faint">Chats</span>
        <IconButton label="New chat" size="sm" onClick={() => setCreating(true)}>
          <Plus className="size-4" />
        </IconButton>
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {chats.map((c) => {
          const Icon = chatIcon(c.icon);
          const active = view === "chat" && c.id === activeChatId;
          const busy = Object.values(activity[c.id] ?? {}).some((a) => a.activity !== "idle");
          return (
            <button
              key={c.id}
              onClick={() => void openChat(c.id)}
              className={cn(
                "no-drag group relative flex h-9 w-full items-center gap-2.5 rounded-sm px-2.5 text-left text-[13.5px] transition-ui",
                active ? "text-fg" : "text-muted hover:bg-hover hover:text-fg",
              )}
            >
              {active && (
                <motion.span layoutId="chat-active" className="absolute inset-0 rounded-sm bg-active border border-line" transition={{ type: "spring", stiffness: 500, damping: 38 }} />
              )}
              <Icon className={cn("relative size-[17px] shrink-0", active ? "text-fg" : "text-faint group-hover:text-muted")} />
              <span className="relative truncate font-medium">{c.name}</span>
              {busy && <span className="relative ml-auto size-1.5 rounded-full [background:var(--brand-gradient)] animate-pulse" />}
            </button>
          );
        })}
      </nav>

      <div className="border-t border-line p-2 space-y-0.5">
        <NavButton icon={<CalendarClock className="size-[17px]" />} label="Automations" active={view === "automations"} onClick={() => setView("automations")} />
        <NavButton icon={<Settings className="size-[17px]" />} label="Settings" active={view === "settings"} onClick={() => setView("settings")} />
      </div>
      <NewChatDialog open={creating} onOpenChange={setCreating} />
    </aside>
  );
}

function NavButton({ icon, label, active, onClick }: { icon: React.ReactNode; label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={cn("no-drag flex h-9 w-full items-center gap-2.5 rounded-sm px-2.5 text-[13.5px] font-medium transition-ui", active ? "bg-active text-fg" : "text-muted hover:bg-hover hover:text-fg")}
    >
      <span className={active ? "text-fg" : "text-faint"}>{icon}</span>
      {label}
    </button>
  );
}
