import Foundation

// Mirrors the JSON the Rust core produces (camelCase, millisecond timestamps). See packages/shared/src/generated.

// MARK: - Arbitrary JSON

/// A question an agent asked; the user picks one of its options or writes their own answer.
struct QuestionOption: Equatable { let label: String; let description: String? }
struct PendingQuestion: Identifiable, Equatable {
    let id: String, chatId: String, agentId: String, question: String
    let options: [QuestionOption]
}

enum JSON: Codable, Equatable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSON])
    case object([String: JSON])

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let b = try? c.decode(Bool.self) { self = .bool(b) }
        else if let n = try? c.decode(Double.self) { self = .number(n) }
        else if let s = try? c.decode(String.self) { self = .string(s) }
        else if let a = try? c.decode([JSON].self) { self = .array(a) }
        else if let o = try? c.decode([String: JSON].self) { self = .object(o) }
        else { self = .null }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let b): try c.encode(b)
        case .number(let n): try c.encode(n)
        case .string(let s): try c.encode(s)
        case .array(let a): try c.encode(a)
        case .object(let o): try c.encode(o)
        }
    }

    subscript(key: String) -> JSON {
        if case .object(let o) = self { return o[key] ?? .null }
        return .null
    }

    var string: String? { if case .string(let s) = self { return s }; return nil }
    var bool: Bool? { if case .bool(let b) = self { return b }; return nil }
    var number: Double? { if case .number(let n) = self { return n }; return nil }
    var array: [JSON]? { if case .array(let a) = self { return a }; return nil }
}

// MARK: - Colors

struct GradientStop: Codable, Equatable {
    var color: String
    var position: Double
}

enum AgentColor: Codable, Equatable {
    case solid(String)
    case gradient(angle: Double, stops: [GradientStop])

    private enum Keys: String, CodingKey { case type, color, angle, stops }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "gradient":
            self = .gradient(angle: try c.decode(Double.self, forKey: .angle), stops: try c.decode([GradientStop].self, forKey: .stops))
        default:
            self = .solid(try c.decode(String.self, forKey: .color))
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        switch self {
        case .solid(let color):
            try c.encode("solid", forKey: .type)
            try c.encode(color, forKey: .color)
        case .gradient(let angle, let stops):
            try c.encode("gradient", forKey: .type)
            try c.encode(angle, forKey: .angle)
            try c.encode(stops, forKey: .stops)
        }
    }
}

// MARK: - Models and efforts

enum EffortValue: Codable, Equatable {
    case level(String)
    case budget(Double)
    case toggle(Bool)

    private enum Keys: String, CodingKey { case type, value }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "budget": self = .budget(try c.decode(Double.self, forKey: .value))
        case "toggle": self = .toggle(try c.decode(Bool.self, forKey: .value))
        default: self = .level(try c.decode(String.self, forKey: .value))
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Keys.self)
        switch self {
        case .level(let v): try c.encode("level", forKey: .type); try c.encode(v, forKey: .value)
        case .budget(let v): try c.encode("budget", forKey: .type); try c.encode(v, forKey: .value)
        case .toggle(let v): try c.encode("toggle", forKey: .type); try c.encode(v, forKey: .value)
        }
    }
}

struct ModelRef: Codable, Equatable, Hashable {
    var providerId: String
    var modelId: String
    var effort: EffortValue?
    var vision: Bool
    var tools: Bool
    var contextWindow: Int?

    func hash(into h: inout Hasher) { h.combine(providerId); h.combine(modelId) }
}

enum EffortControl: Codable, Equatable {
    case none
    case levels(levels: [String], defaultLevel: String)
    case budget(min: Double, max: Double, defaultValue: Double)
    case toggle

    private enum Keys: String, CodingKey { case type, levels, `default`, min, max }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "levels": self = .levels(levels: try c.decode([String].self, forKey: .levels), defaultLevel: try c.decode(String.self, forKey: .default))
        case "budget": self = .budget(min: try c.decode(Double.self, forKey: .min), max: try c.decode(Double.self, forKey: .max), defaultValue: try c.decode(Double.self, forKey: .default))
        case "toggle": self = .toggle
        default: self = .none
        }
    }

    func encode(to encoder: Encoder) throws {}

    /// The value a freshly picked model starts with.
    var defaultValue: EffortValue? {
        switch self {
        case .none: return nil
        case .levels(_, let d): return .level(d)
        case .budget(_, _, let d): return .budget(d)
        case .toggle: return .toggle(true)
        }
    }
}

struct ModelInfo: Codable, Identifiable, Equatable {
    var id: String
    var name: String
    var reasoning: Bool
    var reason: String?
    var effort: EffortControl
    var vision: Bool
    var tools: Bool
    var imageOutput: Bool
    var contextWindow: Int?
    var free: Bool
}

// MARK: - Agents and chats

enum AgentRole: String, Codable { case moderator, orchestrator, specialist }

struct Agent: Codable, Identifiable, Equatable {
    var id: String
    var slug: String
    var name: String
    var role: AgentRole
    var specialty: String
    var job: String
    var color: AgentColor
    var model: ModelRef?
    var imageModel: ModelRef?
    var iosModel: ModelRef?
    var sort: Int
    var updatedAt: Int64
}

struct Chat: Codable, Identifiable, Equatable {
    var id: String
    var name: String
    var icon: String
    var sort: Int
    var agentIds: [String]
    var cleanupEnabled: Bool
    var cleanupTtlHours: Int
    var workspacePath: String?
    var createdAt: Int64
    var updatedAt: Int64
}

struct UserRole: Codable, Identifiable, Equatable {
    var id: String
    var name: String
    var icon: String
    var color: AgentColor
    var sort: Int
    var updatedAt: Int64
}

struct UsageStats: Codable, Equatable {
    var agentId: String
    var tokensIn: Int64
    var tokensOut: Int64
    var thinkingMs: Int64
    var workingMs: Int64
    var requests: Int64
    var lastQueryMs: Int64?
    var lastQueryAt: Int64?
}

// MARK: - Messages

enum MessageKind: String, Codable { case text, ack, plan, notice, image }
enum MessageStatus: String, Codable { case pending, streaming, done, error, cancelled }
enum AttachmentKind: String, Codable { case image, text, pdf, other }

struct Attachment: Codable, Identifiable, Equatable {
    var id: String
    var name: String
    var mime: String
    var kind: AttachmentKind
    var size: Int64
    var path: String
}

struct Message: Codable, Identifiable, Equatable {
    var id: String
    var chatId: String
    var agentId: String?
    var kind: MessageKind
    var status: MessageStatus
    var content: String
    var reasoning: String?
    var error: String?
    var mentions: [String]
    var attachments: [Attachment]
    var meta: JSON
    var thinkingMs: Int64?
    var workingMs: Int64?
    var tokensIn: Int64?
    var tokensOut: Int64?
    var createdAt: Int64
    var updatedAt: Int64

    var isUser: Bool { agentId == nil }
    var isStreaming: Bool { status == .streaming || status == .pending }
    var routingMode: String? { meta["routing"]["mode"].string }
    var remoteDevice: String? { meta["remoteDevice"].string }
    var remoteTarget: String? { meta["remoteTarget"].string }
    var isRemote: Bool { meta["remote"].bool ?? false }
}

// MARK: - Sync, devices, remote control

enum SyncState: String, Codable { case off, locked, idle, syncing, offline, error, revoked }

struct SyncStatus: Codable, Equatable {
    var state: SyncState
    var provider: String?
    var lastSyncAt: Int64?
    var pending: Int64
    var error: String?
    var deviceId: String
    var deviceName: String

    static let off = SyncStatus(state: .off, provider: nil, lastSyncAt: nil, pending: 0, error: nil, deviceId: "", deviceName: "")
}

struct SyncSetupResult: Codable {
    var recoveryKey: String?
    var created: Bool
}

struct DeviceInfo: Codable, Identifiable, Equatable {
    var id: String
    var name: String
    var platform: String
    var kind: String
    var remote: Bool
    var authorized: Bool
    var lastSeen: Int64
    var revoked: Bool
    var thisDevice: Bool
}

struct RemoteReceipt: Codable {
    var commandId: String
    var messageId: String
    var path: String
    var targetDeviceId: String
    var targetName: String
}

struct ProviderConfig: Codable, Identifiable, Equatable {
    var id: String
    var kind: String
    var name: String
    var baseUrl: String
    var extra: JSON
    var enabled: Bool
    var hasKey: Bool
    var updatedAt: Int64
}

struct ProviderKindInfo: Codable, Identifiable, Equatable, Hashable {
    var kind: String
    var label: String
    var isLocal: Bool
    var needsKey: Bool
    var defaultBaseUrl: String
    var id: String { kind }
}

struct AgentModelState: Codable, Equatable {
    var agentId: String
    var effective: ModelRef?
    var desktopModelUnavailable: Bool
}

struct UpdateInfo: Codable {
    var current: String
    var latest: String
    var available: Bool
    var url: String?
}

// MARK: - Events

enum AgentActivity: String, Codable { case idle, routing, thinking, writing, usingTool = "using_tool", stuck }

struct ActivityState: Equatable {
    var activity: AgentActivity
    var detail: String?
}

enum CoreEvent {
    case messageCreated(Message)
    case messageUpdated(Message)
    case messageDelta(chatId: String, messageId: String, text: String?, reasoning: String?)
    case messagesRemoved(chatId: String, ids: [String], all: Bool)
    case agentActivity(chatId: String, agentId: String, activity: AgentActivity, detail: String?)
    case chatChanged(Chat)
    case chatDeleted(String)
    case agentChanged(Agent)
    case rolesChanged([UserRole])
    case settingChanged(key: String, value: JSON)
    case usageUpdated(UsageStats)
    case confirmRequest(id: String, title: String, body: String)
    case questionRequest(PendingQuestion)
    case questionClosed(id: String)
    case updateAvailable(version: String, url: String)
    case toast(level: String, text: String)
    case syncStatusChanged(SyncStatus)
    case syncApplied(providers: Bool, automations: Bool)
    case other

    private struct Envelope: Decodable { let type: String }

    static func decode(_ json: String) -> CoreEvent {
        guard let data = json.data(using: .utf8),
              let type = try? JSONDecoder().decode(Envelope.self, from: data).type,
              let raw = try? JSONDecoder().decode(JSON.self, from: data) else { return .other }
        let dec = JSONDecoder()
        func sub<T: Decodable>(_ key: String, as type: T.Type) -> T? {
            guard let d = try? JSONEncoder().encode(raw[key]) else { return nil }
            return try? dec.decode(type, from: d)
        }
        switch type {
        case "messageCreated": if let m = sub("message", as: Message.self) { return .messageCreated(m) }
        case "messageUpdated": if let m = sub("message", as: Message.self) { return .messageUpdated(m) }
        case "messageDelta":
            return .messageDelta(chatId: raw["chatId"].string ?? "", messageId: raw["messageId"].string ?? "", text: raw["text"].string, reasoning: raw["reasoning"].string)
        case "messagesRemoved":
            return .messagesRemoved(chatId: raw["chatId"].string ?? "", ids: (raw["messageIds"].array ?? []).compactMap(\.string), all: raw["all"].bool ?? false)
        case "agentActivity":
            return .agentActivity(chatId: raw["chatId"].string ?? "", agentId: raw["agentId"].string ?? "", activity: AgentActivity(rawValue: raw["activity"].string ?? "idle") ?? .idle, detail: raw["detail"].string)
        case "chatChanged": if let c = sub("chat", as: Chat.self) { return .chatChanged(c) }
        case "chatDeleted": return .chatDeleted(raw["chatId"].string ?? "")
        case "agentChanged": if let a = sub("agent", as: Agent.self) { return .agentChanged(a) }
        case "rolesChanged": if let r = sub("roles", as: [UserRole].self) { return .rolesChanged(r) }
        case "settingChanged":
            let change = raw["change"]
            if change["type"].string == "global" { return .settingChanged(key: change["key"].string ?? "", value: change["value"]) }
        case "usageUpdated": if let u = sub("usage", as: UsageStats.self) { return .usageUpdated(u) }
        case "confirmRequest": return .confirmRequest(id: raw["id"].string ?? "", title: raw["title"].string ?? "", body: raw["body"].string ?? "")
        case "questionRequest":
            let options = (raw["options"].array ?? []).map { QuestionOption(label: $0["label"].string ?? "", description: $0["description"].string) }
            return .questionRequest(PendingQuestion(id: raw["id"].string ?? "", chatId: raw["chatId"].string ?? "", agentId: raw["agentId"].string ?? "", question: raw["question"].string ?? "", options: options))
        case "questionClosed": return .questionClosed(id: raw["id"].string ?? "")
        case "updateAvailable": return .updateAvailable(version: raw["version"].string ?? "", url: raw["url"].string ?? "")
        case "toast": return .toast(level: raw["level"].string ?? "info", text: raw["text"].string ?? "")
        case "syncStatusChanged": if let s = sub("status", as: SyncStatus.self) { return .syncStatusChanged(s) }
        case "syncApplied": return .syncApplied(providers: raw["providers"].bool ?? false, automations: raw["automations"].bool ?? false)
        default: break
        }
        return .other
    }
}
