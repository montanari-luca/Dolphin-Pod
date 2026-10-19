import { Channel, convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import type { CoreEvent } from "@dolphin/shared";
import type { Backend } from "./api";

export function tauriBackend(): Backend {
  return {
    isTauri: true,
    appInfo: () => invoke("app_info"),
    async subscribe(onEvent) {
      const channel = new Channel<CoreEvent>();
      channel.onmessage = onEvent;
      await invoke("subscribe", { channel });
    },
    listAgents: () => invoke("list_agents"),
    updateAgent: (agent) => invoke("update_agent", { agent }),
    listChats: () => invoke("list_chats"),
    createChat: (name, icon, agentIds) => invoke("create_chat", { name, icon, agentIds }),
    updateChat: (chat) => invoke("update_chat", { chat }),
    deleteChat: (chatId) => invoke("delete_chat", { chatId }),
    cleanChat: (chatId) => invoke("clean_chat", { chatId }),
    listMessages: (chatId, limit) => invoke("list_messages", { chatId, limit }),
    sendMessage: (chatId, text, attachmentIds) => invoke("send_message", { chatId, text, attachmentIds }),
    stopChat: (chatId) => invoke("stop_chat", { chatId }),
    approvePlan: (messageId) => invoke("approve_plan", { messageId }),
    grantPermission: (chatId, agentId, level) => invoke("grant_permission", { chatId, agentId, level }),
    grants: (chatId) => invoke("grants", { chatId }),
    actionLog: (chatId) => invoke("action_log", { chatId }),
    usageAll: () => invoke("usage_all"),
    resolveConfirmation: (id, accepted) => invoke("resolve_confirmation", { id, accepted }),
    listRoles: () => invoke("list_roles"),
    saveRole: (role) => invoke("save_role", { role }),
    deleteRole: (roleId) => invoke("delete_role", { roleId }),
    reorderRoles: (roleIds) => invoke("reorder_roles", { roleIds }),
    stageAttachment: (name, mime, bytes) =>
      invoke("stage_attachment", bytes, {
        headers: { "x-name": encodeURIComponent(name), "x-mime": encodeURIComponent(mime) },
      }),
    stageAttachmentPath: (path) => invoke("stage_attachment_path", { path }),
    providerKinds: () => invoke("provider_kinds"),
    listProviders: () => invoke("list_providers"),
    saveProvider: (config, apiKey) => invoke("save_provider", { config, apiKey }),
    deleteProvider: (providerId) => invoke("delete_provider", { providerId }),
    listModels: (providerId) => invoke("list_models", { providerId }),
    inspectModel: (modelId) => invoke("inspect_model", { modelId }),
    settingsRegistry: () => invoke("settings_registry"),
    globalSettings: () => invoke("global_settings"),
    setSetting: (key, target, value) => invoke("set_setting", { key, target, value }),
    listAutomations: () => invoke("list_automations"),
    saveAutomation: (automation) => invoke("save_automation", { automation }),
    deleteAutomation: (automationId) => invoke("delete_automation", { automationId }),
    checkUpdate: () => invoke("check_update"),
    async pickFolder() {
      const r = await open({ directory: true, multiple: false, title: "Choose a workspace folder" });
      return typeof r === "string" ? r : null;
    },
    async pickFiles() {
      const r = await open({ multiple: true, title: "Attach files" });
      if (!r) return [];
      return Array.isArray(r) ? r : [r];
    },
    openUrl: (url) => openUrl(url),
    revealPath: (path) => revealItemInDir(path),
    fileUrl: (path) => convertFileSrc(path),
  };
}
