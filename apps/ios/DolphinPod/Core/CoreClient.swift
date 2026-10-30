import Foundation

/// Typed wrapper over the UniFFI object: every call returns decoded models instead of JSON strings.
final class CoreClient: @unchecked Sendable {
    let core: DolphinCore
    private let decoder = JSONDecoder()
    private let encoder = JSONEncoder()

    init(dataDir: String, deviceName: String) throws {
        core = try DolphinCore.open(dataDir: dataDir, deviceName: deviceName)
    }

    private func decode<T: Decodable>(_ json: String) throws -> T {
        try decoder.decode(T.self, from: Data(json.utf8))
    }

    private func encode<T: Encodable>(_ value: T) throws -> String {
        String(decoding: try encoder.encode(value), as: UTF8.self)
    }

    // MARK: lifecycle

    func setOnDeviceBackend(_ backend: OnDeviceBackend) { core.setOnDeviceBackend(backend: backend) }

    func start() async { await core.start() }

    func subscribe(_ listener: EventListener) async { await core.subscribe(listener: listener) }

    var version: String { core.version() }

    // MARK: reading

    func chats() throws -> [Chat] { try decode(core.listChats()) }
    func agents() throws -> [Agent] { try decode(core.listAgents()) }
    func roles() throws -> [UserRole] { try decode(core.listRoles()) }
    func messages(chatId: String, limit: UInt32 = 300) throws -> [Message] { try decode(core.listMessages(chatId: chatId, limit: limit)) }
    func usage() throws -> [UsageStats] { try decode(core.usageAll()) }
    func settings() throws -> [String: JSON] { try decode(core.globalSettings()) }
    func providers() throws -> [ProviderConfig] { try decode(core.listProviders()) }
    func providerKinds() throws -> [ProviderKindInfo] { try decode(core.providerKinds()) }
    func agentModels() throws -> [AgentModelState] { try decode(core.agentModels()) }

    // MARK: chats and messages

    func createChat(name: String, icon: String, agentIds: [String]) throws -> Chat {
        try decode(core.createChat(name: name, icon: icon, agentIds: agentIds))
    }
    func deleteChat(_ id: String) throws { try core.deleteChat(chatId: id) }
    func cleanChat(_ id: String) throws { _ = try core.cleanChat(chatId: id) }
    func sendMessage(chatId: String, text: String, attachmentIds: [String]) async throws -> Message {
        try decode(await core.sendMessage(chatId: chatId, text: text, attachmentIds: attachmentIds))
    }
    func stop(_ chatId: String) { core.stopChat(chatId: chatId) }
    func approvePlan(_ messageId: String) async throws { try await core.approvePlan(messageId: messageId) }
    func chooseDesign(_ messageId: String, choice: String, note: String?) async throws { try await core.chooseDesign(messageId: messageId, choice: choice, note: note) }
    func stageAttachment(name: String, mime: String, data: Data) throws -> Attachment {
        try decode(core.stageAttachment(name: name, mime: mime, bytes: data))
    }
    func resolveConfirmation(id: String, accepted: Bool) { core.resolveConfirmation(id: id, accepted: accepted) }
    func answerQuestion(id: String, answer: String, custom: Bool) { core.answerQuestion(id: id, answer: answer, custom: custom) }
    func skipQuestion(id: String) { core.skipQuestion(id: id) }

    // MARK: agents, providers, settings

    func updateAgent(_ agent: Agent) throws -> Agent { try decode(core.updateAgent(agentJson: try encode(agent))) }
    func saveProvider(_ cfg: ProviderConfig, apiKey: String?) throws -> ProviderConfig {
        try decode(core.saveProvider(providerJson: try encode(cfg), apiKey: apiKey))
    }
    func deleteProvider(_ id: String) throws { try core.deleteProvider(providerId: id) }
    func listModels(providerId: String) async throws -> [ModelInfo] { try decode(await core.listModels(providerId: providerId)) }
    func setSetting(key: String, value: JSON) throws { _ = try core.setSetting(key: key, target: nil, valueJson: try encode(value)) }
    func checkUpdate() async throws -> UpdateInfo { try decode(await core.checkUpdate()) }

    // MARK: sync

    func syncStatus() throws -> SyncStatus { try decode(core.syncStatus()) }
    func syncSetup(url: String, passphrase: String) async throws -> SyncSetupResult { try decode(await core.syncSetup(url: url, passphrase: passphrase)) }
    func syncRecover(url: String, recoveryKey: String, newPassphrase: String) async throws {
        try await core.syncRecover(url: url, recoveryKey: recoveryKey, newPassphrase: newPassphrase)
    }
    func syncUnlock(_ passphrase: String) async throws { try await core.syncUnlock(passphrase: passphrase) }
    func syncDisable() throws { try core.syncDisable() }
    func syncNow() async throws { try await core.syncNow() }
    func syncWake() { core.syncWake() }
    func syncChangePassphrase(old: String, new: String) async throws { try await core.syncChangePassphrase(old: old, new: new) }
    func syncNewRecoveryKey(passphrase: String) async throws -> String { try await core.syncNewRecoveryKey(passphrase: passphrase) }
    func syncDevices() async throws -> [DeviceInfo] { try decode(await core.syncDevices()) }
    func syncRevokeDevice(_ id: String) async throws { try await core.syncRevokeDevice(deviceId: id) }
    func setDeviceName(_ name: String) throws { try core.setDeviceName(name: name) }
    func fetchAttachment(_ id: String) async throws -> String { try await core.syncFetchAttachment(attachmentId: id) }

    // MARK: remote control

    func remoteTargets() async throws -> [DeviceInfo] { try decode(await core.remoteTargets()) }
    func remoteSend(target: String?, chatId: String, text: String, lanHost: String?, lanPort: UInt16?) async throws -> RemoteReceipt {
        try decode(await core.remoteSend(targetDeviceId: target, chatId: chatId, text: text, lanHost: lanHost, lanPort: lanPort))
    }
}

/// Receives core events on a Rust thread and hands them to the main actor.
final class EventBridge: EventListener, @unchecked Sendable {
    private let handler: @Sendable (CoreEvent) -> Void
    init(_ handler: @escaping @Sendable (CoreEvent) -> Void) { self.handler = handler }
    func onEvent(eventJson: String) { handler(CoreEvent.decode(eventJson)) }
}
