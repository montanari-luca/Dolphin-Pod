// Typed facade over the Rust core. In the Tauri webview it calls commands; in a plain browser
// (UI development) it falls back to an in-memory mock with simulated streaming.
import type {
  ActionLogEntry,
  Agent,
  AgentMemory,
  Attachment,
  Automation,
  Chat,
  CoreEvent,
  DeviceInfo,
  MemoryKind,
  Message,
  ModelInfo,
  PermissionGrant,
  PermissionLevel,
  PersonaProposal,
  Rating,
  ShellPolicy,
  RevertReport,
  RemoteSettings,
  ProviderConfig,
  ProviderKindInfo,
  SettingChange,
  SettingDef,
  SyncSetupResult,
  SyncStatus,
  UpdateInfo,
  UsageStats,
  UserRole,
} from "@dolphin/shared";

export interface AppInfo {
  version: string;
  platform: string;
  dataDir: string;
  attachmentsDir: string;
  manifestUrl: string;
}

export type Settings = Record<string, unknown>;

export interface Backend {
  appInfo(): Promise<AppInfo>;
  subscribe(onEvent: (e: CoreEvent) => void): Promise<void>;
  listAgents(): Promise<Agent[]>;
  updateAgent(agent: Agent): Promise<Agent>;
  listAgentMemories(agentId: string): Promise<AgentMemory[]>;
  /** Notes other agents shared with this one. Read-only here: their owners change or take them back. */
  listSharedMemories(agentId: string): Promise<AgentMemory[]>;
  /** Try the chosen embedding model once. The size of the vectors it returns, or why it does not work. */
  testEmbeddingModel(): Promise<number>;
  addAgentMemory(agentId: string, kind: MemoryKind, text: string, project: string | null): Promise<AgentMemory>;
  updateAgentMemory(memory: AgentMemory): Promise<AgentMemory>;
  deleteAgentMemory(id: string): Promise<void>;
  /** Wipe everything one agent remembers. How many entries went. */
  forgetAgentMemories(agentId: string): Promise<number>;
  /** Thumbs up or down on an agent's message; `null` takes the rating back. Agents learn from it (when learning is on). */
  rateMessage(messageId: string, rating: Rating | null): Promise<void>;
  /** A persona change the agent suggested for itself, waiting for you. */
  personaProposal(agentId: string): Promise<PersonaProposal | null>;
  /** Accepting is the only way a suggestion becomes the persona. */
  acceptPersonaProposal(id: string): Promise<Agent>;
  rejectPersonaProposal(id: string): Promise<void>;
  listChats(): Promise<Chat[]>;
  createChat(name: string, icon: string, agentIds: string[]): Promise<Chat>;
  updateChat(chat: Chat): Promise<Chat>;
  deleteChat(chatId: string): Promise<void>;
  cleanChat(chatId: string): Promise<number>;
  listMessages(chatId: string, limit?: number): Promise<Message[]>;
  sendMessage(chatId: string, text: string, attachmentIds: string[]): Promise<Message>;
  stopChat(chatId: string): Promise<void>;
  approvePlan(messageId: string): Promise<void>;
  grantPermission(chatId: string, agentId: string, level: PermissionLevel): Promise<PermissionGrant>;
  grants(chatId: string): Promise<PermissionGrant[]>;
  /** The unified diff of everything an agent's message changed in the workspace. */
  runDiff(messageId: string): Promise<string>;
  /** Put the files that message changed back the way they were. */
  revertRun(messageId: string): Promise<RevertReport>;
  /** How commands behave in a chat: network access and pre-approved command prefixes. Stays on this device. */
  shellPolicy(chatId: string): Promise<ShellPolicy>;
  setShellPolicy(chatId: string, policy: ShellPolicy): Promise<ShellPolicy>;
  actionLog(chatId: string): Promise<ActionLogEntry[]>;
  usageAll(): Promise<UsageStats[]>;
  resolveConfirmation(id: string, accepted: boolean): Promise<void>;
  /** Answer an agent's question with one of its option labels, or `custom` text the user wrote. */
  answerQuestion(id: string, answer: string, custom: boolean): Promise<void>;
  skipQuestion(id: string): Promise<void>;
  listRoles(): Promise<UserRole[]>;
  saveRole(role: UserRole): Promise<UserRole>;
  deleteRole(roleId: string): Promise<void>;
  reorderRoles(roleIds: string[]): Promise<UserRole[]>;
  stageAttachment(name: string, mime: string, bytes: Uint8Array): Promise<Attachment>;
  stageAttachmentPath(path: string): Promise<Attachment>;
  providerKinds(): Promise<ProviderKindInfo[]>;
  listProviders(): Promise<ProviderConfig[]>;
  saveProvider(config: ProviderConfig, apiKey: string | null): Promise<ProviderConfig>;
  deleteProvider(providerId: string): Promise<void>;
  listModels(providerId: string): Promise<ModelInfo[]>;
  inspectModel(modelId: string): Promise<ModelInfo>;
  settingsRegistry(): Promise<SettingDef[]>;
  globalSettings(): Promise<Settings>;
  setSetting(key: string, target: string | null, value: unknown): Promise<SettingChange>;
  listAutomations(): Promise<Automation[]>;
  saveAutomation(a: Automation): Promise<Automation>;
  deleteAutomation(id: string): Promise<void>;
  checkUpdate(): Promise<UpdateInfo>;
  syncStatus(): Promise<SyncStatus>;
  syncSetup(url: string, passphrase: string): Promise<SyncSetupResult>;
  syncRecover(url: string, recoveryKey: string, newPassphrase: string): Promise<void>;
  syncUnlock(passphrase: string): Promise<void>;
  syncDisable(): Promise<void>;
  syncNow(): Promise<void>;
  syncChangePassphrase(oldPassphrase: string, newPassphrase: string): Promise<void>;
  syncNewRecoveryKey(passphrase: string): Promise<string>;
  syncDevices(): Promise<DeviceInfo[]>;
  syncRevokeDevice(deviceId: string): Promise<void>;
  setDeviceName(name: string): Promise<void>;
  /** Download an attachment another device uploaded; resolves to the local file path. */
  fetchAttachment(attachmentId: string): Promise<string>;
  remoteSettings(): Promise<RemoteSettings>;
  setRemoteEnabled(enabled: boolean): Promise<RemoteSettings>;
  remoteAuthorize(deviceId: string, allow: boolean): Promise<void>;
  pickFolder(): Promise<string | null>;
  pickFiles(): Promise<string[]>;
  openUrl(url: string): Promise<void>;
  revealPath(path: string): Promise<void>;
  fileUrl(path: string): string;
  isTauri: boolean;
}

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let backendPromise: Promise<Backend> | null = null;

export function backend(): Promise<Backend> {
  if (!backendPromise) {
    backendPromise = isTauri
      ? import("./tauri").then((m) => m.tauriBackend())
      : import("./mock").then((m) => m.mockBackend());
  }
  return backendPromise;
}

/** Errors from the core arrive as plain strings. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
