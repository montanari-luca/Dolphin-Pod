import { motion } from "motion/react";
import { Bot, Info, Palette, Plug, ScrollText, SlidersHorizontal, UserRound } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";
import { useApp, type SettingsTab } from "@/store/app";
import { AboutPage } from "./AboutPage";
import { AgentsPage } from "./AgentsPage";
import { AppearancePage, GeneralPage, PreferencesPage } from "./GeneralPages";
import { ProfilePage } from "./ProfilePage";
import { ProvidersPage } from "./ProvidersPage";

const TABS: { id: SettingsTab; label: string; icon: ReactNode }[] = [
  { id: "profile", label: "Profile", icon: <UserRound className="size-4" /> },
  { id: "general", label: "General", icon: <SlidersHorizontal className="size-4" /> },
  { id: "providers", label: "Providers", icon: <Plug className="size-4" /> },
  { id: "agents", label: "Agents", icon: <Bot className="size-4" /> },
  { id: "preferences", label: "Preferences", icon: <ScrollText className="size-4" /> },
  { id: "appearance", label: "Appearance", icon: <Palette className="size-4" /> },
  { id: "about", label: "About", icon: <Info className="size-4" /> },
];

export function SettingsView() {
  const tab = useApp((s) => s.settingsTab);
  const setView = useApp((s) => s.setView);
  return (
    <section className="glass flex h-full min-w-0 flex-1 overflow-hidden rounded-xl">
      <nav className="w-[200px] shrink-0 border-r border-line p-2">
        <div className="drag-region flex h-[52px] items-end px-2.5 pb-2 text-[15px] font-semibold text-fg">Settings</div>
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setView("settings", t.id)}
            className={cn("no-drag relative flex h-9 w-full items-center gap-2.5 rounded-sm px-2.5 text-[13px] font-medium transition-ui", tab === t.id ? "text-fg" : "text-muted hover:bg-hover hover:text-fg")}
          >
            {tab === t.id && <motion.span layoutId="settings-tab" className="absolute inset-0 rounded-sm bg-active border border-line" transition={{ type: "spring", stiffness: 500, damping: 38 }} />}
            <span className="relative text-faint">{t.icon}</span>
            <span className="relative">{t.label}</span>
          </button>
        ))}
      </nav>
      <div className="min-w-0 flex-1 overflow-y-auto">
        <div className="drag-region h-4" />
        <motion.div key={tab} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }} className="mx-auto max-w-[760px] px-8 pb-10">
          {tab === "profile" && <ProfilePage />}
          {tab === "general" && <GeneralPage />}
          {tab === "providers" && <ProvidersPage />}
          {tab === "agents" && <AgentsPage />}
          {tab === "preferences" && <PreferencesPage />}
          {tab === "appearance" && <AppearancePage />}
          {tab === "about" && <AboutPage />}
        </motion.div>
      </div>
    </section>
  );
}

export function PageHeader({ title, description, actions }: { title: string; description?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-5 mt-6 flex items-end justify-between gap-4">
      <div>
        <h2 className="text-[19px] font-semibold tracking-tight text-fg">{title}</h2>
        {description && <p className="mt-1 max-w-xl text-[12.5px] leading-relaxed text-muted">{description}</p>}
      </div>
      {actions}
    </div>
  );
}

export function Card({ children, className, title }: { children: ReactNode; className?: string; title?: ReactNode }) {
  return (
    <div className={cn("mb-4 rounded-lg border border-line bg-surface-0 px-4", className)}>
      {title && <div className="border-b border-line py-2.5 text-[12px] font-semibold uppercase tracking-wider text-faint">{title}</div>}
      <div className="divide-y divide-[var(--border)]">{children}</div>
    </div>
  );
}
