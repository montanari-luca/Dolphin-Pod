import Foundation

/// The small reasoning models a phone can run itself. Sizes are the 4-bit MLX downloads.
struct OnDeviceCatalogEntry: Codable, Identifiable {
    var id: String
    var name: String
    var sizeMb: Int
    var contextWindow: Int
    var repo: String
}

enum OnDeviceCatalog {
    static let curated: [OnDeviceCatalogEntry] = [
        .init(id: "qwen3.5-0.8b-4bit", name: "Qwen3.5 0.8B", sizeMb: 652, contextWindow: 8192, repo: "mlx-community/Qwen3.5-0.8B-MLX-4bit"),
        .init(id: "deepseek-r1-distill-qwen-1.5b-4bit", name: "DeepSeek R1 Distill 1.5B", sizeMb: 1011, contextWindow: 8192, repo: "mlx-community/DeepSeek-R1-Distill-Qwen-1.5B-4bit"),
        .init(id: "qwen3.5-2b-4bit", name: "Qwen3.5 2B", sizeMb: 1749, contextWindow: 8192, repo: "mlx-community/Qwen3.5-2B-MLX-4bit"),
    ]
}

#if canImport(MLXLLM) && !targetEnvironment(simulator)
import MLXLLM
import MLXLMCommon

/// Runs the curated models with MLX Swift. Weights download from Hugging Face on first use and are cached.
///
/// Enable by adding the `mlx-swift-examples` package (products MLXLLM, MLXLMCommon) to the app target; the
/// default project leaves it out because MLX needs a physical device (no Metal GPU in the simulator).
final class OnDeviceEngine: OnDeviceBackend, @unchecked Sendable {
    private var containers: [String: ModelContainer] = [:]
    private let lock = NSLock()
    private var downloaded: Set<String> {
        get { Set(UserDefaults.standard.stringArray(forKey: "ondevice.downloaded") ?? []) }
        set { UserDefaults.standard.set(Array(newValue), forKey: "ondevice.downloaded") }
    }

    func modelsJson() -> String {
        let have = downloaded
        let rows = OnDeviceCatalog.curated.map {
            ["id": $0.id as Any, "name": $0.name, "sizeMb": $0.sizeMb, "contextWindow": $0.contextWindow, "downloaded": have.contains($0.id)] as [String: Any]
        }
        return (try? String(data: JSONSerialization.data(withJSONObject: rows), encoding: .utf8)) ?? "[]"
    }

    func generate(requestJson: String, sink: GenerationSink) async {
        struct Msg: Decodable { var role: String; var text: String }
        struct Req: Decodable { var model: String; var system: String; var messages: [Msg]; var maxTokens: Int? }
        guard let req = try? JSONDecoder().decode(Req.self, from: Data(requestJson.utf8)),
              let entry = OnDeviceCatalog.curated.first(where: { $0.id == req.model }) else {
            sink.fail(message: "Unknown on-device model")
            return
        }
        do {
            let container = try await load(entry)
            let history = req.messages.dropLast().map { Chat.Message(role: $0.role == "assistant" ? .assistant : .user, content: $0.text) }
            let prompt = req.messages.last?.text ?? ""
            let session = ChatSession(container, instructions: req.system, history: Array(history))
            var out = 0
            var inThink = false
            for try await piece in session.streamResponse(to: prompt) {
                if sink.isCancelled() { break }
                out += 1
                // Reasoning models write their thoughts between <think> tags; route them to the thinking stream.
                var rest = Substring(piece)
                while !rest.isEmpty {
                    if inThink, let end = rest.range(of: "</think>") {
                        sink.reasoning(delta: String(rest[..<end.lowerBound])); rest = rest[end.upperBound...]; inThink = false
                    } else if !inThink, let start = rest.range(of: "<think>") {
                        let before = String(rest[..<start.lowerBound]); if !before.isEmpty { sink.text(delta: before) }
                        rest = rest[start.upperBound...]; inThink = true
                    } else {
                        if inThink { sink.reasoning(delta: String(rest)) } else { sink.text(delta: String(rest)) }
                        rest = ""
                    }
                }
            }
            sink.finish(inputTokens: Int64(prompt.count / 4), outputTokens: Int64(out))
        } catch {
            sink.fail(message: "On-device model failed: \(error)")
        }
    }

    private func load(_ entry: OnDeviceCatalogEntry) async throws -> ModelContainer {
        lock.lock(); let cached = containers[entry.id]; lock.unlock()
        if let cached { return cached }
        let container = try await LLMModelFactory.shared.loadContainer(configuration: ModelConfiguration(id: entry.repo))
        lock.lock(); containers[entry.id] = container; lock.unlock()
        downloaded.insert(entry.id)
        return container
    }
}
#else
/// Without MLX (simulator, or the package not added) the phone offers no on-device models.
final class OnDeviceEngine: OnDeviceBackend, @unchecked Sendable {
    func modelsJson() -> String { "[]" }
    func generate(requestJson: String, sink: GenerationSink) async {
        sink.fail(message: "On-device models are not available in this build.")
    }
}
#endif
