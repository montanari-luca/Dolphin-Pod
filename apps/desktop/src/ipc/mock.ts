// In-browser mock of the Rust core, for UI work outside the Tauri webview (vite dev in a browser).
// It mirrors the seeds and simulates routing + streaming so every screen can be exercised.
import type {
  Agent,
  AgentColor,
  Automation,
  Chat,
  CoreEvent,
  DeviceInfo,
  Message,
  ModelInfo,
  PermissionGrant,
  ProviderConfig,
  ProviderKind,
  ProviderKindInfo,
  SettingDef,
  SyncStatus,
  UsageStats,
  UserRole,
} from "@dolphin/shared";
import type { Backend, Settings } from "./api";

const uid = () => crypto.randomUUID();
const now = () => Date.now();
const g = (a: string, b: string): AgentColor => ({
  type: "gradient",
  angle: 135,
  stops: [
    { color: a, position: 0 },
    { color: b, position: 100 },
  ],
});

const SEED: [string, string, Agent["role"], string, string, AgentColor][] = [
  ["roger", "Roger", "moderator", "moderator", "Moderator. Keeps order in the pod, answers general questions first, routes your requests to the right specialist and explains what is going on.", g("#3d8bfd", "#a77bdb")],
  ["lincoln", "Lincoln", "orchestrator", "orchestrator", "Orchestrator. Works behind the scenes: turns Roger's decisions into precise briefs, directs the specialists and enforces permissions.", g("#4f46e5", "#0ea5e9")],
  ["rick", "Rick", "specialist", "coder", "Coding specialist. Writes, refactors and reviews code in any language.", g("#2563eb", "#22d3ee")],
  ["micheal", "Micheal", "specialist", "long_horizon", "Long-horizon agent. Plans and carries out big, multi-step projects end to end.", g("#7c3aed", "#f59e0b")],
  ["viktor", "Viktor", "specialist", "security", "Security and cyber specialist. Threat models, reviews code for vulnerabilities and hardens systems.", g("#dc2626", "#7f1d1d")],
  ["jordan", "Jordan", "specialist", "debug", "Debug specialist. Reads stack traces and logs, reproduces bugs and finds root causes.", g("#f97316", "#facc15")],
  ["emma", "Emma", "specialist", "creative", "Creative writing specialist. Stories, poems, scripts, copy and brainstorming.", g("#ec4899", "#f472b6")],
  ["ducky", "Ducky", "specialist", "docs", "Docs specialist. Writes READMEs, API references, guides and clear explanations of code.", g("#eab308", "#fde047")],
  ["sarah", "Sarah", "specialist", "image", "Image specialist. Turns your ideas into detailed image prompts and generates the images.", g("#f7826f", "#fbb36a")],
  ["prof", "Prof", "specialist", "teacher", "Professor. Helps you learn anything step by step, with examples and checks for understanding.", g("#10b981", "#14b8a6")],
  ["ruby", "Ruby", "specialist", "fun_facts", "Fun facts agent. Tells you surprising, true fun facts whenever you ask for one.", g("#e11d48", "#fb7185")],
  ["amelia", "Amelia", "specialist", "verifier", "Verification agent, called only when you explicitly ask her to double-check work, with evidence for every verdict.", g("#0d9488", "#2dd4bf")],
  ["luna", "Luna", "specialist", "researcher", "Research agent. Runs many web searches in parallel, validates and cross-checks the sources, checks dates and reports the final findings with citations.", g("#4338ca", "#818cf8")],
  ["robert", "Robert", "specialist", "git", "Git manager. Tracks history and handles status, commit, push, pull and fetch, usually on behalf of the coding agents.", g("#ea580c", "#f59e0b")],
];

const KINDS: ProviderKindInfo[] = (
  [
    ["ollama", "Ollama", true, "http://localhost:11434"],
    ["lm_studio", "LM Studio", true, "http://localhost:1234/v1"],
    ["custom_openai", "Custom endpoint", true, "http://localhost:8000/v1"],
    ["openrouter", "OpenRouter", false, "https://openrouter.ai/api/v1"],
    ["openai", "OpenAI", false, "https://api.openai.com/v1"],
    ["anthropic", "Anthropic", false, "https://api.anthropic.com"],
    ["gemini", "Google Gemini", false, "https://generativelanguage.googleapis.com"],
    ["xai", "xAI", false, "https://api.x.ai/v1"],
    ["mistral", "Mistral", false, "https://api.mistral.ai/v1"],
    ["deepseek", "DeepSeek", false, "https://api.deepseek.com/v1"],
    ["groq", "Groq", false, "https://api.groq.com/openai/v1"],
    ["together", "Together AI", false, "https://api.together.xyz/v1"],
    ["fireworks", "Fireworks AI", false, "https://api.fireworks.ai/inference/v1"],
    ["azure_openai", "Azure OpenAI", false, "https://YOUR-RESOURCE.openai.azure.com/openai/v1"],
  ] as const
).map(([kind, label, isLocal, url]) => ({ kind: kind as ProviderKind, label, isLocal, needsKey: !isLocal, defaultBaseUrl: url }));

const SAMPLE = `Here's a small example in **Java**:

\`\`\`java
public static int add(int a, int b) {
    return a + b;
}
\`\`\`

1. \`public static\` makes it callable without an object.
2. \`int\` before the name is the **return type**.
3. The parameters go in parentheses.

| Part | Meaning |
| --- | --- |
| \`public\` | visible everywhere |
| \`static\` | belongs to the class |

Quick check: what would you change to return a \`double\`?`;

let syncState: SyncStatus = { state: "off", provider: null, lastSyncAt: null, pending: 0, error: null, deviceId: "this", deviceName: "This computer" };
let remoteOn = false;
const mockAuthorized: Record<string, boolean> = {};
let emitSync: (() => void) | null = null;
function setSync(patch: Partial<SyncStatus>) {
  syncState = { ...syncState, ...patch };
  emitSync?.();
}

export function mockBackend(): Backend {
  const listeners: ((e: CoreEvent) => void)[] = [];
  const emit = (e: CoreEvent) => listeners.forEach((l) => l(e));
  emitSync = () => emit({ type: "syncStatusChanged", status: { ...syncState } });
  const agents: Agent[] = SEED.map(([slug, name, role, specialty, job, color], i) => ({
    id: `a-${slug}`, slug, name, role, specialty, job, color, model: null, imageModel: null, iosModel: null, sort: i, updatedAt: now(),
  }));
  const id = (slug: string) => `a-${slug}`;
  const chats: Chat[] = [
    ["Coding", "code", ["rick", "jordan", "ducky", "viktor", "prof", "robert", "amelia"]],
    ["General", "message-circle", ["rick", "micheal", "viktor", "jordan", "emma", "ducky", "sarah", "prof", "ruby", "amelia", "luna", "robert"]],
    ["Long Horizon Agentic", "rocket", ["micheal", "rick", "jordan", "robert", "amelia", "luna"]],
    ["Learning", "graduation-cap", ["prof", "ducky", "ruby", "luna"]],
    ["Creative", "palette", ["emma", "sarah", "ruby"]],
  ].map(([name, icon, slugs], i) => ({
    id: `c-${i}`, name: name as string, icon: icon as string, sort: i,
    agentIds: ["roger", "lincoln", ...(slugs as string[])].map(id),
    cleanupEnabled: true, cleanupTtlHours: 24, workspacePath: null, createdAt: now(), updatedAt: now(),
  }));
  const messages = new Map<string, Message[]>();
  const usage = new Map<string, UsageStats>();
  const providers: ProviderConfig[] = [];
  const grants: PermissionGrant[] = [];
  const automations: Automation[] = [];
  let roles: UserRole[] = [
    {
      id: "r-master", name: "Master", icon: "crown", sort: 0, updatedAt: now(),
      color: { type: "gradient", angle: 90, stops: [["#ff3b5c", 0], ["#ff9f1c", 20], ["#ffd60a", 38], ["#2ee59d", 56], ["#3d8bfd", 76], ["#b15cff", 100]].map(([color, position]) => ({ color: color as string, position: position as number })) },
    },
  ];
  const emitRoles = () => emit({ type: "rolesChanged", roles: structuredClone(roles) });
  const settings: Settings = {
    "appearance.theme": "system", "appearance.reduce_motion": false, "appearance.reduce_transparency": false,
    "cleanup.enabled": true, "cleanup.default_ttl_hours": 24, "preferences.custom_instructions": "", "utility.model": null,
    "supervisor.stall_seconds": 120, "updates.auto_check": true, "context.history_messages": 40, "context.auto_compact": true, "context.compact_at": 85, "context.compress_tools": true, "context.local_window": 32768, "chat.paste_threshold": 6000,
    "routing.show_briefs": true, "web.access": true, "app.onboarded": false, "profile.display_name": "You",
  };

  const msg = (chatId: string, agentId: string | null, kind: Message["kind"], content: string, status: Message["status"] = "done"): Message => ({
    id: uid(), chatId, agentId, kind, status, content, reasoning: null, error: null, mentions: [], attachments: [], meta: {},
    thinkingMs: null, workingMs: null, tokensIn: null, tokensOut: null, createdAt: now(), updatedAt: now(),
  });
  const push = (m: Message) => {
    const list = messages.get(m.chatId) ?? [];
    list.push(m);
    messages.set(m.chatId, list);
    emit({ type: "messageCreated", message: structuredClone(m) });
  };
  const update = (m: Message) => emit({ type: "messageUpdated", message: structuredClone(m) });
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  async function stream(chatId: string, agentId: string, kind: Message["kind"], routing: object) {
    const m = msg(chatId, agentId, kind, "", "streaming");
    m.meta = { routing, tools: [], ...(kind === "plan" ? { plan: { approved: false } } : {}) };
    push(m);
    const started = now();
    emit({ type: "agentActivity", chatId, agentId, activity: "thinking", detail: null });
    const thoughts = ["Let me think about what the user already knows. ", "A tiny example first, ", "then the structure, then a question."];
    for (const t of thoughts) {
      await sleep(220);
      m.reasoning = (m.reasoning ?? "") + t;
      emit({ type: "messageDelta", chatId, messageId: m.id, text: null, reasoning: t });
    }
    const thinkingMs = now() - started;
    emit({ type: "agentActivity", chatId, agentId, activity: "writing", detail: null });
    const text = kind === "plan" ? "## Goal\nAdd OAuth sign-in.\n\n## Phases\n1. Pick a provider\n2. Add callback route\n3. Store sessions\n\n## Risks\n- Token storage\n\nPress **Approve** to start." : SAMPLE;
    for (const chunk of text.match(/.{1,14}/gs) ?? []) {
      await sleep(18);
      m.content += chunk;
      emit({ type: "messageDelta", chatId, messageId: m.id, text: chunk, reasoning: null });
    }
    Object.assign(m, { status: "done", thinkingMs, workingMs: now() - started, tokensIn: 812, tokensOut: 260, updatedAt: now() });
    update(m);
    emit({ type: "agentActivity", chatId, agentId, activity: "idle", detail: null });
    const u = usage.get(agentId) ?? { agentId, tokensIn: 0, tokensOut: 0, thinkingMs: 0, workingMs: 0, requests: 0, lastQueryMs: null, lastQueryAt: null };
    Object.assign(u, { tokensIn: u.tokensIn + 812, tokensOut: u.tokensOut + 260, thinkingMs: u.thinkingMs + thinkingMs, workingMs: u.workingMs + (m.workingMs ?? 0), requests: u.requests + 1, lastQueryMs: m.workingMs, lastQueryAt: now() });
    usage.set(agentId, u);
    emit({ type: "usageUpdated", usage: { ...u } });
  }

  const registry: SettingDef[] = [];

  return {
    isTauri: false,
    appInfo: async () => ({ version: "0.1.0", platform: "browser", dataDir: "(mock)", attachmentsDir: "(mock)", manifestUrl: "https://raw.githubusercontent.com/AstroDevCompany/MintFlow-Releases/refs/heads/main/dolphinpod.json" }),
    subscribe: async (fn) => void listeners.push(fn),
    listAgents: async () => structuredClone(agents),
    async updateAgent(a) {
      const i = agents.findIndex((x) => x.id === a.id);
      agents[i] = { ...a, updatedAt: now() };
      emit({ type: "agentChanged", agent: structuredClone(agents[i]) });
      return structuredClone(agents[i]);
    },
    listChats: async () => structuredClone(chats),
    async createChat(name, icon, agentIds) {
      const c: Chat = { id: uid(), name, icon: icon || "hash", sort: chats.length, agentIds: [id("roger"), id("lincoln"), ...agentIds], cleanupEnabled: true, cleanupTtlHours: 24, workspacePath: null, createdAt: now(), updatedAt: now() };
      chats.push(c);
      emit({ type: "chatChanged", chat: structuredClone(c) });
      return c;
    },
    async updateChat(c) {
      const i = chats.findIndex((x) => x.id === c.id);
      chats[i] = { ...c, updatedAt: now() };
      emit({ type: "chatChanged", chat: structuredClone(chats[i]) });
      return chats[i];
    },
    async deleteChat(chatId) {
      chats.splice(chats.findIndex((c) => c.id === chatId), 1);
      emit({ type: "chatDeleted", chatId });
    },
    async cleanChat(chatId) {
      const n = messages.get(chatId)?.length ?? 0;
      messages.set(chatId, []);
      emit({ type: "messagesRemoved", chatId, messageIds: [], all: true });
      return n;
    },
    listMessages: async (chatId) => structuredClone(messages.get(chatId) ?? []),
    async sendMessage(chatId, text) {
      const user = msg(chatId, null, "text", text);
      const mention = agents.find((a) => new RegExp(`@${a.name}\\b`, "i").test(text));
      if (mention) user.mentions = [mention.id];
      push(user);
      (async () => {
        if (mention) return stream(chatId, mention.id, "text", { via: "mention" });
        const ack = msg(chatId, id("roger"), "ack", "Give me a sec, I'll find the right person for this.");
        ack.meta = { routing: { status: "routing" } };
        push(ack);
        emit({ type: "agentActivity", chatId, agentId: id("roger"), activity: "routing", detail: null });
        await sleep(700);
        const plan = /\bplan\b/i.test(text);
        const target = plan ? "micheal" : /java|learn|how/i.test(text) ? "prof" : "rick";
        ack.meta = { routing: { status: "done", mode: plan ? "plan" : "learn", intent: "mock routing", targets: [agents.find((a) => a.slug === target)!.name], source: "model" } };
        update(ack);
        emit({ type: "agentActivity", chatId, agentId: id("roger"), activity: "idle", detail: null });
        emit({ type: "agentActivity", chatId, agentId: id("lincoln"), activity: "routing", detail: "Briefing" });
        await sleep(400);
        emit({ type: "agentActivity", chatId, agentId: id("lincoln"), activity: "idle", detail: null });
        await stream(chatId, id(target), plan ? "plan" : "text", { via: "lincoln", mode: plan ? "plan" : "learn", rewrite: "light", brief: `Teach the user: ${text}` });
      })();
      return user;
    },
    stopChat: async () => {},
    async approvePlan(messageId) {
      for (const list of messages.values()) {
        const m = list.find((x) => x.id === messageId);
        if (!m) continue;
        m.meta = { ...(m.meta as object), plan: { approved: true } };
        update(m);
        push(Object.assign(msg(m.chatId, id("lincoln"), "notice", "🔓 Micheal now has **full agentic permissions** in this chat (plan approved)."), { meta: { notice: "permission" } }));
        void stream(m.chatId, m.agentId!, "text", { via: "lincoln", mode: "implement" });
      }
    },
    async grantPermission(chatId, agentId, level) {
      const gr = { chatId, agentId, level, reason: "granted by you", updatedAt: now() };
      grants.push(gr);
      emit({ type: "grantChanged", grant: gr });
      return gr;
    },
    grants: async (chatId) => grants.filter((x) => x.chatId === chatId),
    actionLog: async () => [],
    usageAll: async () => [...usage.values()],
    resolveConfirmation: async () => {},
    listRoles: async () => structuredClone(roles),
    async saveRole(r) {
      if (roles.some((x) => x.id !== r.id && x.name.toLowerCase() === r.name.trim().toLowerCase())) throw `you already have a role called ${r.name}`;
      const saved = { ...r, name: r.name.trim(), id: r.id || uid(), sort: r.id ? r.sort : roles.length, updatedAt: now() };
      roles = r.id ? roles.map((x) => (x.id === r.id ? saved : x)) : [...roles, saved];
      emitRoles();
      return saved;
    },
    async deleteRole(id) {
      roles = roles.filter((r) => r.id !== id);
      emitRoles();
    },
    async reorderRoles(ids) {
      roles = ids.map((id, i) => ({ ...roles.find((r) => r.id === id)!, sort: i }));
      emitRoles();
      return structuredClone(roles);
    },
    stageAttachment: async (name, mime, bytes) => ({ id: uid(), name, mime, kind: mime.startsWith("image/") ? "image" : "text", size: bytes.length, path: URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime })) }),
    stageAttachmentPath: async (path) => ({ id: uid(), name: path.split("/").pop() ?? path, mime: "text/plain", kind: "text", size: 0, path }),
    providerKinds: async () => KINDS,
    listProviders: async () => structuredClone(providers),
    async saveProvider(c, key) {
      const saved = { ...c, id: c.id || uid(), hasKey: Boolean(key) || c.hasKey, updatedAt: now() };
      const i = providers.findIndex((p) => p.id === saved.id);
      if (i >= 0) providers[i] = saved;
      else providers.push(saved);
      return saved;
    },
    deleteProvider: async (pid) => void providers.splice(providers.findIndex((p) => p.id === pid), 1),
    listModels: async (): Promise<ModelInfo[]> => [
      { id: "qwen3.5:2b", name: "qwen3.5:2b", reasoning: true, reason: null, effort: { type: "toggle" }, vision: false, tools: true, imageOutput: false, contextWindow: 40960, free: true },
      { id: "gpt-oss:20b", name: "gpt-oss:20b", reasoning: true, reason: null, effort: { type: "levels", levels: ["low", "medium", "high"], default: "medium" }, vision: false, tools: true, imageOutput: false, contextWindow: 131072, free: true },
      { id: "llama3.1:8b", name: "llama3.1:8b", reasoning: false, reason: "Not a reasoning model. Dolphin Pod agents need models that think before answering.", effort: { type: "none" }, vision: false, tools: true, imageOutput: false, contextWindow: null, free: true },
    ],
    inspectModel: async (modelId) => ({ id: modelId, name: modelId, reasoning: false, reason: "Not recognized as a reasoning model.", effort: { type: "none" }, vision: false, tools: true, imageOutput: false, contextWindow: null, free: false }),
    settingsRegistry: async () => registry,
    globalSettings: async () => ({ ...settings }),
    async setSetting(key, _target, value) {
      settings[key] = value;
      const change = { type: "global" as const, key, value: value as never };
      emit({ type: "settingChanged", change });
      return change;
    },
    listAutomations: async () => structuredClone(automations),
    async saveAutomation(a) {
      const saved = { ...a, id: a.id || uid(), updatedAt: now() };
      const i = automations.findIndex((x) => x.id === saved.id);
      if (i >= 0) automations[i] = saved;
      else automations.push(saved);
      return saved;
    },
    deleteAutomation: async (aid) => void automations.splice(automations.findIndex((a) => a.id === aid), 1),
    syncStatus: async () => ({ ...syncState }),
    async syncSetup(url, passphrase) {
      if (passphrase.length < 8) throw "use a passphrase of at least 8 characters";
      if (!url.startsWith("postgres")) throw "sync needs a Postgres connection URL (postgres://…). NeonDB gives you one for free.";
      setSync({ state: "idle", provider: new URL(url.replace("postgresql://", "postgres://")).hostname, lastSyncAt: now() });
      return { recoveryKey: "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress", created: true };
    },
    async syncRecover(url) {
      setSync({ state: "idle", provider: new URL(url.replace("postgresql://", "postgres://")).hostname, lastSyncAt: now() });
    },
    async syncUnlock() {
      setSync({ state: "idle" });
    },
    async syncDisable() {
      setSync({ state: "off", provider: null, lastSyncAt: null });
    },
    async syncNow() {
      setSync({ lastSyncAt: now() });
    },
    syncChangePassphrase: async () => {},
    syncNewRecoveryKey: async () => "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title",
    syncDevices: async (): Promise<DeviceInfo[]> => [
      { id: "this", name: "This computer", platform: "macos", kind: "desktop", remote: remoteOn, authorized: false, lastSeen: now(), revoked: false, thisDevice: true },
      { id: "phone", name: "Luca's iPhone", platform: "ios", kind: "phone", remote: false, authorized: !!mockAuthorized.phone, lastSeen: now() - 120_000, revoked: false, thisDevice: false },
    ],
    syncRevokeDevice: async () => {},
    setDeviceName: async (name) => setSync({ deviceName: name }),
    fetchAttachment: async () => "",
    remoteSettings: async () => ({ enabled: remoteOn, lanPort: remoteOn ? 49152 : null }),
    async setRemoteEnabled(enabled) {
      remoteOn = enabled;
      return { enabled, lanPort: enabled ? 49152 : null };
    },
    async remoteAuthorize(id, allow) {
      mockAuthorized[id] = allow;
    },
    checkUpdate: async () => ({ current: "0.1.0", latest: "0.1.0", available: false, url: null }),
    pickFolder: async () => "/Users/you/Projects/demo",
    pickFiles: async () => [],
    openUrl: async (url) => void window.open(url, "_blank"),
    revealPath: async () => {},
    fileUrl: (path) => path,
  };
}
