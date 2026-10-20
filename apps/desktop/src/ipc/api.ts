// Typed facade over the Rust core. In the Tauri webview it calls commands; in a plain browser
// (UI development) it falls back to an in-memory mock with simulated streaming.
import type {
  ActionLogEntry,
  Agent,
  Attachment,
  Automation,
  Chat,
  CoreEvent,
  DeviceInfo,
  Message,
  ModelInfo,
  PermissionGrant,
  PermissionLevel,
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
  actionLog(chatId: string): Promise<ActionLogEntry[]>;
  usageAll(): Promise<UsageStats[]>;
  resolveConfirmation(id: string, accepted: boolean): Promise<void>;
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
