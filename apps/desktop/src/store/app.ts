import { create } from "zustand";
import type {
  Agent,
  AgentActivity,
  Chat,
  CoreEvent,
  Message,
  PermissionGrant,
  ProviderConfig,
  ProviderKindInfo,
  UsageStats,
  UserRole,
} from "@dolphin/shared";
import { backend, errorText, type AppInfo, type Backend, type Settings } from "@/ipc/api";

export type View = "chat" | "settings" | "automations";
export type SettingsTab = "profile" | "general" | "providers" | "agents" | "preferences" | "appearance" | "about";

export interface Toast {
  id: number;
  level: "info" | "success" | "warning" | "error";
  text: string;
}

export interface Activity {
  activity: AgentActivity;
  detail: string | null;
  since: number;
}

interface State {
  api: Backend | null;
  info: AppInfo | null;
  ready: boolean;
  bootError: string | null;

  agents: Agent[];
  roles: UserRole[];
  chats: Chat[];
  messages: Record<string, Message[]>;
  settings: Settings;
  usage: Record<string, UsageStats>;
  activity: Record<string, Record<string, Activity>>;
  grants: Record<string, PermissionGrant[]>;
  providers: ProviderConfig[];
  kinds: ProviderKindInfo[];
  confirms: { id: string; title: string; body: string }[];
  toasts: Toast[];
  update: { version: string; url: string } | null;

  activeChatId: string | null;
  view: View;
  settingsTab: SettingsTab;
  focusAgentId: string | null;

  boot(): Promise<void>;
  apply(e: CoreEvent): void;
  openChat(id: string): Promise<void>;
  setView(v: View, tab?: SettingsTab, focusAgentId?: string | null): void;
  refreshProviders(): Promise<void>;
  refreshSettings(): Promise<void>;
  setSetting(key: string, value: unknown, target?: string | null): Promise<boolean>;
  toast(level: Toast["level"], text: string): void;
  dismissToast(id: number): void;
  run<T>(p: Promise<T>, okText?: string): Promise<T | undefined>;
}

let toastSeq = 1;
let booted = false;

export const useApp = create<State>((set, get) => ({
  api: null,
  info: null,
  ready: false,
  bootError: null,
  agents: [],
  roles: [],
  chats: [],
  messages: {},
  settings: {},
  usage: {},
  activity: {},
  grants: {},
  providers: [],
  kinds: [],
  confirms: [],
  toasts: [],
  update: null,
  activeChatId: null,
  view: "chat",
  settingsTab: "general",
  focusAgentId: null,

  async boot() {
    // Boot once per page (React StrictMode mounts effects twice; a second subscription would double every event).
    if (booted) return;
    booted = true;
    try {
      const api = await backend();
      set({ api });
      await api.subscribe((e) => get().apply(e));
      const [info, agents, roles, chats, settings, usage, providers, kinds] = await Promise.all([
        api.appInfo(),
        api.listAgents(),
        api.listRoles(),
        api.listChats(),
        api.globalSettings(),
        api.usageAll(),
        api.listProviders(),
        api.providerKinds(),
      ]);
      const lastChat = safeGet("dp.activeChat");
      const active = chats.find((c) => c.id === lastChat)?.id ?? chats[0]?.id ?? null;
      set({
        info,
        agents,
        roles,
        chats,
        settings,
        usage: Object.fromEntries(usage.map((u) => [u.agentId, u])),
        providers,
        kinds,
        ready: true,
      });
      if (active) await get().openChat(active);
    } catch (e) {
      set({ bootError: errorText(e) });
    }
  },

  apply(e) {
    switch (e.type) {
      case "messageCreated":
        set((s) => {
          const list = s.messages[e.message.chatId];
          if (!list || list.some((m) => m.id === e.message.id)) return {};
          return { messages: { ...s.messages, [e.message.chatId]: [...list, e.message] } };
        });
        break;
      case "messageDelta":
        set((s) => {
          const list = s.messages[e.chatId];
          if (!list) return {};
          const i = list.findIndex((m) => m.id === e.messageId);
          if (i < 0) return {};
          const m = list[i];
          const next = list.slice();
          next[i] = {
            ...m,
            content: e.text ? m.content + e.text : m.content,
            reasoning: e.reasoning ? (m.reasoning ?? "") + e.reasoning : m.reasoning,
          };
          return { messages: { ...s.messages, [e.chatId]: next } };
        });
        break;
      case "messageUpdated":
        set((s) => {
          const list = s.messages[e.message.chatId];
          if (!list) return {};
          const i = list.findIndex((m) => m.id === e.message.id);
          const next = list.slice();
          if (i < 0) next.push(e.message);
          else next[i] = e.message;
          return { messages: { ...s.messages, [e.message.chatId]: next } };
        });
        break;
      case "messagesRemoved":
        set((s) => {
          const list = s.messages[e.chatId];
          if (!list) return {};
          const gone = new Set(e.messageIds);
          return { messages: { ...s.messages, [e.chatId]: e.all ? [] : list.filter((m) => !gone.has(m.id)) } };
        });
        break;
      case "agentActivity":
        set((s) => ({
          activity: {
            ...s.activity,
            [e.chatId]: { ...s.activity[e.chatId], [e.agentId]: { activity: e.activity, detail: e.detail, since: Date.now() } },
          },
        }));
        break;
      case "chatChanged":
        set((s) => {
          const i = s.chats.findIndex((c) => c.id === e.chat.id);
          const chats = s.chats.slice();
          if (i < 0) chats.push(e.chat);
          else chats[i] = e.chat;
          return { chats: chats.sort((a, b) => a.sort - b.sort) };
        });
        break;
      case "chatDeleted":
        set((s) => {
          const chats = s.chats.filter((c) => c.id !== e.chatId);
          const activeChatId = s.activeChatId === e.chatId ? (chats[0]?.id ?? null) : s.activeChatId;
          return { chats, activeChatId };
        });
        {
          const id = get().activeChatId;
          if (id) void get().openChat(id);
        }
        break;
      case "agentChanged":
        set((s) => ({ agents: s.agents.map((a) => (a.id === e.agent.id ? e.agent : a)) }));
        break;
      case "rolesChanged":
        set({ roles: e.roles });
        break;
      case "settingChanged":
        if (e.change.type === "global") {
          const { key, value } = e.change;
          set((s) => ({ settings: { ...s.settings, [key]: value } }));
        }
        break;
      case "usageUpdated":
        set((s) => ({ usage: { ...s.usage, [e.usage.agentId]: e.usage } }));
        break;
      case "grantChanged":
        set((s) => {
          const list = (s.grants[e.grant.chatId] ?? []).filter((g) => g.agentId !== e.grant.agentId);
          return { grants: { ...s.grants, [e.grant.chatId]: [...list, e.grant] } };
        });
        break;
      case "confirmRequest":
        set((s) => ({ confirms: [...s.confirms, { id: e.id, title: e.title, body: e.body }] }));
        break;
      case "updateAvailable":
        set({ update: { version: e.version, url: e.url } });
        break;
      case "toast":
        get().toast(e.level, e.text);
        break;
      case "actionLogged":
        break;
    }
  },

  async openChat(id) {
    const api = get().api;
    if (!api) return;
    set({ activeChatId: id, view: "chat" });
    safeSet("dp.activeChat", id);
    const [msgs, grants] = await Promise.all([api.listMessages(id, 300), api.grants(id)]);
    set((s) => ({ messages: { ...s.messages, [id]: msgs }, grants: { ...s.grants, [id]: grants } }));
  },

  setView(view, tab, focusAgentId = null) {
    set((s) => ({ view, settingsTab: tab ?? s.settingsTab, focusAgentId }));
  },

  async refreshProviders() {
    const api = get().api;
    if (api) set({ providers: await api.listProviders() });
  },

  async refreshSettings() {
    const api = get().api;
    if (api) set({ settings: await api.globalSettings() });
  },

  async setSetting(key, value, target = null) {
    const api = get().api;
    if (!api) return false;
    try {
      const change = await api.setSetting(key, target, value);
      if (change.type === "global") set((s) => ({ settings: { ...s.settings, [key]: change.value } }));
      return true;
    } catch (e) {
      get().toast("error", errorText(e));
      return false;
    }
  },

  toast(level, text) {
    const id = toastSeq++;
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, level, text }] }));
    setTimeout(() => get().dismissToast(id), level === "error" ? 7000 : 3800);
  },

  dismissToast(id) {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },

  async run(p, okText) {
    try {
      const r = await p;
      if (okText) get().toast("success", okText);
      return r;
    } catch (e) {
      get().toast("error", errorText(e));
      return undefined;
    }
  },
}));

// Remembered UI conveniences only; never data.
export function safeGet(k: string): string | null {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
}
export function safeSet(k: string, v: string) {
  try {
    localStorage.setItem(k, v);
  } catch {
    /* ignore */
  }
}

export const agentById = (agents: Agent[], id: string | null | undefined) => agents.find((a) => a.id === id);
