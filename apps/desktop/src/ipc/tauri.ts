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
    listAgentMemories: (agentId) => invoke("list_agent_memories", { agentId }),
    addAgentMemory: (agentId, kind, text, project) => invoke("add_agent_memory", { agentId, kind, text, project }),
    updateAgentMemory: (memory) => invoke("update_agent_memory", { memory }),
    deleteAgentMemory: (id) => invoke("delete_agent_memory", { id }),
    forgetAgentMemories: (agentId) => invoke("forget_agent_memories", { agentId }),
    rateMessage: (messageId, rating) => invoke("rate_message", { messageId, rating }),
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
    runDiff: (messageId) => invoke("run_diff", { messageId }),
    revertRun: (messageId) => invoke("revert_run", { messageId }),
    shellPolicy: (chatId) => invoke("shell_policy", { chatId }),
    setShellPolicy: (chatId, policy) => invoke("set_shell_policy", { chatId, policy }),
    actionLog: (chatId) => invoke("action_log", { chatId }),
    usageAll: () => invoke("usage_all"),
    resolveConfirmation: (id, accepted) => invoke("resolve_confirmation", { id, accepted }),
    answerQuestion: (id, answer, custom) => invoke("answer_question", { id, answer, custom }),
    skipQuestion: (id) => invoke("skip_question", { id }),
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
    syncStatus: () => invoke("sync_status"),
    syncSetup: (url, passphrase) => invoke("sync_setup", { url, passphrase }),
    syncRecover: (url, recoveryKey, newPassphrase) => invoke("sync_recover", { url, recoveryKey, newPassphrase }),
    syncUnlock: (passphrase) => invoke("sync_unlock", { passphrase }),
    syncDisable: () => invoke("sync_disable"),
    syncNow: () => invoke("sync_now"),
    syncChangePassphrase: (oldPassphrase, newPassphrase) => invoke("sync_change_passphrase", { old: oldPassphrase, new: newPassphrase }),
    syncNewRecoveryKey: (passphrase) => invoke("sync_new_recovery_key", { passphrase }),
    syncDevices: () => invoke("sync_devices"),
    syncRevokeDevice: (deviceId) => invoke("sync_revoke_device", { deviceId }),
    setDeviceName: (name) => invoke("set_device_name", { name }),
    fetchAttachment: (attachmentId) => invoke("sync_fetch_attachment", { attachmentId }),
    remoteSettings: () => invoke("remote_settings"),
    setRemoteEnabled: (enabled) => invoke("set_remote_enabled", { enabled }),
    remoteAuthorize: (deviceId, allow) => invoke("remote_authorize", { deviceId, allow }),
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
