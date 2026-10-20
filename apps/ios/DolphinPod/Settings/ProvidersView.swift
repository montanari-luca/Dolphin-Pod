import SwiftUI

struct ProvidersView: View {
    @Environment(AppModel.self) private var model
    @State private var adding = false
    @State private var editing: ProviderConfig?

    var body: some View {
        List {
            Section {
                ForEach(model.providers) { p in
                    Button { editing = p } label: {
                        HStack(spacing: 12) {
                            Image(systemName: isLocal(p) ? "desktopcomputer" : "cloud.fill").foregroundStyle(.secondary).frame(width: 24)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(p.name).foregroundStyle(.primary).font(.system(size: 16, weight: .medium))
                                Text(isLocal(p) ? "On a computer · not reachable from this iPhone" : p.baseUrl).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
                            }
                            Spacer()
                            if needsKey(p) { Image(systemName: p.hasKey ? "key.fill" : "key").foregroundStyle(p.hasKey ? Color.green : Color.orange) }
                        }
                    }
                }
                if model.providers.isEmpty { Text("No providers yet").foregroundStyle(.secondary) }
            } footer: { Text("API keys are stored in the iOS keychain and sync encrypted with your pod.") }
            Section { Button { adding = true } label: { Label("Add a cloud provider", systemImage: "plus") } }
        }
        .scrollContentBackground(.hidden).background(PodBackground())
        .navigationTitle("Providers").navigationBarTitleDisplayMode(.inline)
        .sheet(isPresented: $adding) { AddProviderSheet() }
        .sheet(item: $editing) { ProviderEditor(provider: $0) }
    }

    private func isLocal(_ p: ProviderConfig) -> Bool { model.providerKinds.first { $0.kind == p.kind }?.isLocal ?? false }
    private func needsKey(_ p: ProviderConfig) -> Bool { model.providerKinds.first { $0.kind == p.kind }?.needsKey ?? true }
}

struct AddProviderSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var kind: ProviderKindInfo?
    @State private var apiKey = ""
    @State private var baseUrl = ""

    var body: some View {
        NavigationStack {
            Form {
                Section("Provider") {
                    Picker("Kind", selection: $kind) {
                        Text("Choose…").tag(ProviderKindInfo?.none)
                        ForEach(model.providerKinds.filter { !$0.isLocal || $0.kind == "custom_openai" }) { Text($0.label).tag(Optional($0)) }
                    }
                }
                if let kind {
                    Section("Base URL") { TextField(kind.defaultBaseUrl, text: $baseUrl).textInputAutocapitalization(.never).autocorrectionDisabled() }
                    Section("API key") { SecureField("Paste your key", text: $apiKey).textInputAutocapitalization(.never).autocorrectionDisabled() }
                }
            }
            .navigationTitle("Add provider").navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) { Button("Add") { add() }.disabled(kind == nil) }
            }
            .onChange(of: kind) { _, k in baseUrl = k?.defaultBaseUrl ?? "" }
        }
    }

    private func add() {
        guard let kind, let c = model.client else { return }
        let cfg = ProviderConfig(id: "", kind: kind.kind, name: kind.label, baseUrl: baseUrl, extra: .object([:]), enabled: true, hasKey: false, updatedAt: 0)
        do {
            _ = try c.saveProvider(cfg, apiKey: apiKey.isEmpty ? nil : apiKey)
            model.refreshProviders()
            dismiss()
        } catch { model.toast("error", "\(error)") }
    }
}

struct ProviderEditor: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let provider: ProviderConfig
    @State private var apiKey = ""
    @State private var enabled = true
    @State private var confirmDelete = false

    var body: some View {
        NavigationStack {
            Form {
                Section { Toggle("Enabled", isOn: $enabled) }
                Section("API key") {
                    SecureField(provider.hasKey ? "•••••••• (saved)" : "Paste your key", text: $apiKey).textInputAutocapitalization(.never).autocorrectionDisabled()
                    if provider.hasKey { Button("Remove saved key", role: .destructive) { save(key: "") } }
                }
                Section { Button("Delete provider", role: .destructive) { confirmDelete = true } }
            }
            .navigationTitle(provider.name).navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) { Button("Save") { save(key: apiKey.isEmpty ? nil : apiKey) } }
            }
            .onAppear { enabled = provider.enabled }
            .confirmationDialog("Delete \(provider.name)?", isPresented: $confirmDelete, titleVisibility: .visible) {
                Button("Delete", role: .destructive) { try? model.client?.deleteProvider(provider.id); model.refreshProviders(); dismiss() }
            } message: { Text("It is removed on all your synced devices.") }
        }
    }

    private func save(key: String?) {
        var p = provider
        p.enabled = enabled
        do { _ = try model.client?.saveProvider(p, apiKey: key); model.refreshProviders(); dismiss() }
        catch { model.toast("error", "\(error)") }
    }
}
