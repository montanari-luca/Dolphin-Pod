import { motion } from "motion/react";
import { useEffect } from "react";
import { Background, ConfirmRequests, Toasts, UpdateBanner } from "@/components/Overlays";
import { AutomationsView } from "@/features/automations/AutomationsView";
import { ChatView } from "@/features/chat/ChatView";
import { Welcome } from "@/features/onboarding/Welcome";
import { SettingsView } from "@/features/settings/SettingsView";
import { Sidebar } from "@/features/sidebar/Sidebar";
import { useApplyAppearance } from "@/lib/theme";
import { useApp } from "@/store/app";

export default function App() {
  const boot = useApp((s) => s.boot);
  const ready = useApp((s) => s.ready);
  const bootError = useApp((s) => s.bootError);
  const view = useApp((s) => s.view);
  const info = useApp((s) => s.info);
  const chat = useApp((s) => s.chats.find((c) => c.id === s.activeChatId));
  useApplyAppearance();

  useEffect(() => {
    void boot();
  }, [boot]);

  // Native window vibrancy (macOS / Windows Mica) shows through a translucent base.
  useEffect(() => {
    const vib = info?.platform === "macos" || info?.platform === "windows";
    document.documentElement.classList.toggle("vibrancy", vib);
    if (info) document.documentElement.dataset.platform = info.platform;
    document.documentElement.style.setProperty("--bg-opacity", vib ? "0.62" : "1");
  }, [info]);

  if (bootError) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <Background />
        <div className="glass max-w-md rounded-xl p-6 text-center">
          <h1 className="text-[16px] font-semibold text-fg">Dolphin Pod couldn't start</h1>
          <p className="mt-2 text-[13px] text-muted selectable">{bootError}</p>
        </div>
      </div>
    );
  }

  if (!ready) {
    return (
      <div className="flex h-full items-center justify-center">
        <Background />
        <motion.img src="/brand/dolphin-icon.png" alt="" className="size-16" animate={{ y: [0, -6, 0], rotate: [0, -3, 0] }} transition={{ duration: 1.8, repeat: Infinity, ease: "easeInOut" }} />
      </div>
    );
  }

  return (
    <div className="flex h-full gap-2 p-2">
      <Background />
      <Sidebar />
      <main className="flex min-w-0 flex-1">
        {view === "settings" ? <SettingsView /> : view === "automations" ? <AutomationsView /> : chat ? <ChatView key={chat.id} chat={chat} /> : null}
      </main>
      <Toasts />
      <ConfirmRequests />
      <UpdateBanner />
      <Welcome />
    </div>
  );
}
