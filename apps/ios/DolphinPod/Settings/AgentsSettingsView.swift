import SwiftUI

/// Per-agent iPhone model overrides. The computer's model keeps working on the computer; this one is used here.
struct AgentsSettingsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        List {
            Section {
                ForEach(model.agents.filter { $0.role != .orchestrator || true }) { agent in
                    NavigationLink { AgentModelPicker(agent: agent) } label: {
                        HStack(spacing: 12) {
                            AgentAvatar(agent: agent, size: 34)
                            VStack(alignment: .leading, spacing: 2) {
                                AgentNameText(agent: agent, font: .system(size: 16, weight: .semibold))
                                Text(subtitle(agent)).font(.system(size: 12.5)).foregroundStyle(model.unavailableOnPhone(agent) && agent.iosModel == nil ? Color.orange : Color.secondary).lineLimit(1)
                            }
                        }
                    }
                }
            } footer: {
                Text("Each agent can use a different model on this iPhone. Models that run on your computer (Ollama, LM Studio) can't be reached from here, so pick a cloud model or send the request to your desktop.")
            }
        }
        .scrollContentBackground(.hidden).background(PodBackground())
        .navigationTitle("Agents").navigationBarTitleDisplayMode(.inline)
    }

    private func subtitle(_ a: Agent) -> String {
        if let m = a.iosModel { return "iPhone: \(m.modelId)" }
        if let m = model.effectiveModel(a) { return m.modelId }
        if model.unavailableOnPhone(a) { return "Runs on your computer · unavailable here" }
        return "No model yet"
    }
}

struct AgentModelPicker: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let agent: Agent
    @State private var providerId: String?
    @State private var models: [ModelInfo] = []
    @State private var loading = false
    @State private var error: String?

    private var cloudProviders: [ProviderConfig] {
        model.providers.filter { p in
            guard p.enabled, let kind = model.providerKinds.first(where: { $0.kind == p.kind }) else { return false }
            return !kind.isLocal || (kind.kind == "custom_openai" && !p.baseUrl.contains("localhost") && !p.baseUrl.contains("127.0.0.1"))
        }
    }

    var body: some View {
        List {
            Section {
                if agent.iosModel != nil {
                    Button(role: .destructive) { save(nil) } label: { Label("Use the computer's model instead", systemImage: "arrow.uturn.backward") }
                }
                if let m = agent.iosModel { LabeledContent("Current", value: m.modelId) }
                else if let m = model.effectiveModel(agent) { LabeledContent("Currently using", value: m.modelId) }
            }
            Section("Provider") {
                if cloudProviders.isEmpty {
                    Text("No cloud provider yet. Add one in Settings → Providers, or let your computer's providers sync here.").font(.system(size: 13.5)).foregroundStyle(.secondary)
                }
                ForEach(cloudProviders) { p in
                    Button { Task { await load(p) } } label: {
                        HStack {
                            Text(p.name).foregroundStyle(.primary)
                            Spacer()
                            if providerId == p.id { Image(systemName: "checkmark").foregroundStyle(Brand.blue) }
                            if !p.hasKey { Tag(text: "no key", tint: .orange) }
                        }
                    }
                }
            }
            if loading { Section { HStack { ProgressView(); Text("Loading models…").foregroundStyle(.secondary) } } }
            if let error { Section { Text(error).font(.system(size: 13)).foregroundStyle(.red).textSelection(.enabled) } }
            if !models.isEmpty {
                Section {
                    ForEach(models) { m in
                        Button { pick(m) } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(m.name).foregroundStyle(m.reasoning ? Color.primary : Color.secondary)
                                    if let r = m.reason, !m.reasoning { Text(r).font(.system(size: 11.5)).foregroundStyle(.tertiary).lineLimit(2) }
                                }
                                Spacer()
                                if m.free { Tag(text: "free", tint: .green) }
                                if agent.iosModel?.modelId == m.id && agent.iosModel?.providerId == providerId { Image(systemName: "checkmark").foregroundStyle(Brand.blue) }
                            }
                        }
                        .disabled(!m.reasoning)
                    }
                } header: { Text("Reasoning models") } footer: { Text("Agents need models that think before answering, so only those can be picked.") }
            }
        }
        .scrollContentBackground(.hidden).background(PodBackground())
        .navigationTitle(agent.name).navigationBarTitleDisplayMode(.inline)
    }

    private func load(_ p: ProviderConfig) async {
        providerId = p.id; loading = true; error = nil; models = []
        defer { loading = false }
        do {
            let all = try await model.client?.listModels(providerId: p.id) ?? []
            models = all.sorted { ($0.reasoning ? 0 : 1, $0.name) < ($1.reasoning ? 0 : 1, $1.name) }
        } catch { self.error = "\(error)" }
    }

    private func pick(_ m: ModelInfo) {
        guard let providerId else { return }
        save(ModelRef(providerId: providerId, modelId: m.id, effort: m.effort.defaultValue, vision: m.vision, tools: m.tools, contextWindow: m.contextWindow))
    }

    private func save(_ ref: ModelRef?) {
        var a = agent
        a.iosModel = ref
        do {
            let saved = try model.client?.updateAgent(a) ?? a
            model.apply(.agentChanged(saved))
            model.toast("info", ref == nil ? "\(agent.name) uses its computer model again" : "\(agent.name) will use \(ref!.modelId) on this iPhone")
            dismiss()
        } catch { model.toast("error", "\(error)") }
    }
}
