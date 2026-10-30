import Foundation
import Observation
import SwiftUI

/// Where a message runs: on this phone with its own agents, or on one of the pod's desktops.
enum RunTarget: Hashable {
    case phone
    case desktop(id: String, name: String)

    var label: String {
        switch self {
        case .phone: return "This iPhone"
        case .desktop(_, let name): return name
        }
    }
}

struct Toast: Identifiable, Equatable {
    let id = UUID()
    var level: String
    var text: String
}

@MainActor
@Observable
final class AppModel {
    // MARK: state
    var ready = false
    var bootError: String?

    var agents: [Agent] = []
    var chats: [Chat] = []
    var roles: [UserRole] = []
    var messages: [String: [Message]] = [:]
    var settings: [String: JSON] = [:]
    var usage: [String: UsageStats] = [:]
    var activity: [String: [String: ActivityState]] = [:]
    var providers: [ProviderConfig] = []
    var providerKinds: [ProviderKindInfo] = []
    var agentModels: [String: AgentModelState] = [:]
    var sync: SyncStatus = .off
    var devices: [DeviceInfo] = []
    var remoteTargets: [DeviceInfo] = []
    var toasts: [Toast] = []
    var confirmation: (id: String, title: String, body: String)?
    var questions: [PendingQuestion] = []
    var update: UpdateInfo?
    var lastRoute: (path: String, target: String)?

    var runTarget: RunTarget = .phone
    var selectedChatId: String?
    var version = ""

    @ObservationIgnored private(set) var client: CoreClient?
    @ObservationIgnored private let bonjour = BonjourBrowser()
    @ObservationIgnored private var started = false
    @ObservationIgnored private var refreshTask: Task<Void, Never>?

    // MARK: boot

    func boot() async {
        guard !started else { return }
        started = true
        do {
            let dir = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true).appendingPathComponent("DolphinPod", isDirectory: true)
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let client = try CoreClient(dataDir: dir.path, deviceName: UIDeviceName.current)
            self.client = client
            version = client.version
            await client.subscribe(EventBridge { [weak self] ev in
                Task { @MainActor in self?.apply(ev) }
            })
            client.setOnDeviceBackend(OnDeviceEngine())
            await client.start()
            try reloadAll()
            ready = true
            bonjour.onChange = { [weak self] in self?.objectWillChangeHack() }
            bonjour.start()
            startRefreshLoop()
            #if DEBUG
            Task { await DebugAutomation.run(self) }
            #endif
        } catch {
            bootError = "\(error)"
        }
    }

    private func objectWillChangeHack() {
        // Bonjour changes only matter when a message is sent; nothing to re-render.
    }

    func reloadAll() throws {
        guard let c = client else { return }
        agents = try c.agents()
        chats = try c.chats()
        roles = try c.roles()
        settings = try c.settings()
        usage = Dictionary(uniqueKeysWithValues: try c.usage().map { ($0.agentId, $0) })
        providers = try c.providers()
        providerKinds = try c.providerKinds()
        agentModels = Dictionary(uniqueKeysWithValues: try c.agentModels().map { ($0.agentId, $0) })
        sync = try c.syncStatus()
    }

    /// Foreground housekeeping: look for desktops and refresh the device list while the app is open.
    private func startRefreshLoop() {
        refreshTask?.cancel()
        refreshTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refreshRemote()
                try? await Task.sleep(for: .seconds(20))
            }
        }
    }

    func refreshRemote() async {
        guard let c = client, sync.state != .off else { remoteTargets = []; return }
        if let t = try? await c.remoteTargets() {
            remoteTargets = t
            // Keep the chosen desktop valid; fall back to the phone if it went away.
            if case .desktop(let id, _) = runTarget, !t.contains(where: { $0.id == id }) { runTarget = .phone }
        }
        if let d = try? await c.syncDevices() { devices = d }
    }

    func foreground() {
        client?.syncWake()
        bonjour.start()
        Task { await refreshRemote() }
    }

    func background() { bonjour.stop() }

    // MARK: derived

    var displayName: String { settings["profile.display_name"]?.string?.nilIfEmpty ?? "You" }
    func agent(_ id: String?) -> Agent? { id.flatMap { id in agents.first { $0.id == id } } }
    func chat(_ id: String) -> Chat? { chats.first { $0.id == id } }
    func members(of chat: Chat) -> [Agent] { chat.agentIds.compactMap { agent($0) }.filter { $0.role == .specialist || true } }
    var hasRemoteDesktop: Bool { !remoteTargets.isEmpty }

    func unavailableOnPhone(_ agent: Agent) -> Bool { agentModels[agent.id]?.desktopModelUnavailable ?? false }
    func effectiveModel(_ agent: Agent) -> ModelRef? { agentModels[agent.id]?.effective }

    func streamingAgents(in chatId: String) -> [(Agent, ActivityState)] {
        (activity[chatId] ?? [:]).compactMap { id, st in
            guard st.activity != .idle, let a = agent(id) else { return nil }
            return (a, st)
        }.sorted { $0.0.sort < $1.0.sort }
    }

    // MARK: chat actions

    func openChat(_ id: String) {
        guard let c = client else { return }
        if let m = try? c.messages(chatId: id) { messages[id] = m }
    }

    func send(chatId: String, text: String, attachments: [Attachment]) async {
        guard let c = client else { return }
        do {
            switch runTarget {
            case .phone:
                _ = try await c.sendMessage(chatId: chatId, text: text, attachmentIds: attachments.map(\.id))
            case .desktop(let id, _):
                let ep = bonjour.endpoints[id]
                let receipt = try await c.remoteSend(target: id, chatId: chatId, text: text, lanHost: ep?.host, lanPort: ep?.port)
                lastRoute = (receipt.path, receipt.targetName)
                openChat(chatId)
            }
        } catch {
            toast("error", "\(error)")
            openChat(chatId)
        }
    }

    func stop(chatId: String) { client?.stop(chatId) }

    func approvePlan(_ messageId: String) {
        Task { try? await client?.approvePlan(messageId) }
    }

    /// Answer a design card: "a" or "b" picks that idea, "merge" has a designer write one design from both.
    func chooseDesign(_ messageId: String, choice: String, note: String? = nil) {
        Task {
            do { try await client?.chooseDesign(messageId, choice: choice, note: note) } catch { toast("error", "\(error)") }
        }
    }

    func createChat(name: String) {
        guard let c = client else { return }
        do {
            let chat = try c.createChat(name: name, icon: "hash", agentIds: [])
            if !chats.contains(where: { $0.id == chat.id }) { chats.append(chat) }
        } catch { toast("error", "\(error)") }
    }

    func deleteChat(_ id: String) {
        do { try client?.deleteChat(id); chats.removeAll { $0.id == id }; messages[id] = nil } catch { toast("error", "\(error)") }
    }

    func cleanChat(_ id: String) {
        do { try client?.cleanChat(id); messages[id] = [] } catch { toast("error", "\(error)") }
    }

    func toast(_ level: String, _ text: String) {
        let t = Toast(level: level, text: text)
        toasts.append(t)
        Task { try? await Task.sleep(for: .seconds(4)); toasts.removeAll { $0.id == t.id } }
    }

    // MARK: events

    func apply(_ e: CoreEvent) {
        switch e {
        case .messageCreated(let m):
            guard var list = messages[m.chatId], !list.contains(where: { $0.id == m.id }) else { return }
            list.append(m)
            messages[m.chatId] = list
        case .messageUpdated(let m):
            guard var list = messages[m.chatId] else { return }
            if let i = list.firstIndex(where: { $0.id == m.id }) { list[i] = m } else { list.append(m) }
            messages[m.chatId] = list
        case .messageDelta(let chatId, let messageId, let text, let reasoning):
            guard var list = messages[chatId], let i = list.firstIndex(where: { $0.id == messageId }) else { return }
            if let t = text { list[i].content += t }
            if let r = reasoning { list[i].reasoning = (list[i].reasoning ?? "") + r }
            messages[chatId] = list
        case .messagesRemoved(let chatId, let ids, let all):
            guard var list = messages[chatId] else { return }
            list = all ? [] : list.filter { !ids.contains($0.id) }
            messages[chatId] = list
        case .agentActivity(let chatId, let agentId, let a, let detail):
            activity[chatId, default: [:]][agentId] = ActivityState(activity: a, detail: detail)
        case .chatChanged(let c):
            if let i = chats.firstIndex(where: { $0.id == c.id }) { chats[i] = c } else { chats.append(c) }
            chats.sort { $0.sort < $1.sort }
        case .chatDeleted(let id):
            chats.removeAll { $0.id == id }
        case .agentChanged(let a):
            if let i = agents.firstIndex(where: { $0.id == a.id }) { agents[i] = a } else { agents.append(a); agents.sort { $0.sort < $1.sort } }
            if let c = client, let s = try? c.agentModels() { agentModels = Dictionary(uniqueKeysWithValues: s.map { ($0.agentId, $0) }) }
        case .rolesChanged(let r): roles = r
        case .settingChanged(let key, let value): settings[key] = value
        case .usageUpdated(let u): usage[u.agentId] = u
        case .confirmRequest(let id, let title, let body): confirmation = (id, title, body)
        case .questionRequest(let q): questions.removeAll { $0.id == q.id }; questions.append(q)
        case .questionClosed(let id): questions.removeAll { $0.id == id }
        case .updateAvailable(let version, let url): update = UpdateInfo(current: self.version, latest: version, available: true, url: url)
        case .toast(let level, let text): toast(level, text)
        case .syncStatusChanged(let s):
            let wasOff = sync.state == .off
            sync = s
            if wasOff != (s.state == .off) { Task { await refreshRemote() } }
        case .syncApplied:
            if let c = client { providers = (try? c.providers()) ?? providers }
        case .other: break
        }
    }

    func refreshProviders() {
        guard let c = client else { return }
        providers = (try? c.providers()) ?? providers
        if let s = try? c.agentModels() { agentModels = Dictionary(uniqueKeysWithValues: s.map { ($0.agentId, $0) }) }
    }
}

extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}

enum UIDeviceName {
    static var current: String {
        #if targetEnvironment(simulator)
        return "iPhone Simulator"
        #else
        return UIDevice.current.name
        #endif
    }
}
