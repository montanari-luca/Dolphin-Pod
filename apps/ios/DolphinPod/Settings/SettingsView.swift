import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""

    var body: some View {
        NavigationStack {
            List {
                Section("Profile") {
                    TextField("Your name", text: $name)
                        .onSubmit { save() }
                        .submitLabel(.done)
                }
                Section {
                    NavigationLink { SyncView() } label: {
                        HStack {
                            Label("Sync", systemImage: "icloud")
                            Spacer()
                            Text(syncSummary).font(.system(size: 13)).foregroundStyle(.secondary)
                        }
                    }
                    NavigationLink { RemoteInfoView() } label: {
                        HStack {
                            Label("Remote control", systemImage: "antenna.radiowaves.left.and.right")
                            Spacer()
                            Text(model.remoteTargets.isEmpty ? "No desktop online" : "\(model.remoteTargets.count) online").font(.system(size: 13)).foregroundStyle(.secondary)
                        }
                    }
                }
                Section {
                    NavigationLink { AgentsSettingsView() } label: { Label("Agents", systemImage: "person.2") }
                    NavigationLink { ProvidersView() } label: { Label("Providers", systemImage: "key") }
                } footer: { Text("Cloud providers and keys sync from your computer. Local models (Ollama, LM Studio) stay on the computer that runs them.") }
                Section { NavigationLink { AboutView() } label: { Label("About", systemImage: "info.circle") } }
            }
            .scrollContentBackground(.hidden)
            .background(PodBackground())
            .navigationTitle("Settings").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { save(); dismiss() } } }
            .onAppear { name = model.settings["profile.display_name"]?.string ?? "" }
        }
    }

    private var syncSummary: String {
        switch model.sync.state {
        case .off: return "Off"
        case .idle: return "Up to date"
        case .syncing: return "Syncing…"
        case .offline: return "Offline"
        case .locked: return "Locked"
        case .error: return "Problem"
        case .revoked: return "Removed"
        }
    }

    private func save() {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard trimmed != (model.settings["profile.display_name"]?.string ?? "") else { return }
        try? model.client?.setSetting(key: "profile.display_name", value: .string(trimmed))
    }
}

struct RemoteInfoView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        List {
            Section {
                Text("Start work on your computer from here. When you pick a desktop next to the message box, the request goes straight over Wi-Fi if you are nearby, and through your sync database when you are not. The desktop must have Remote control turned on and must have allowed this iPhone.")
                    .font(.system(size: 14)).foregroundStyle(.secondary)
            }
            Section("Desktops online") {
                if model.remoteTargets.isEmpty {
                    Text("None right now").foregroundStyle(.secondary)
                }
                ForEach(model.remoteTargets) { d in
                    HStack {
                        Image(systemName: "desktopcomputer").foregroundStyle(Brand.blue)
                        VStack(alignment: .leading) {
                            Text(d.name).font(.system(size: 15, weight: .medium))
                            Text("seen \(Fmt.ago(d.lastSeen))").font(.system(size: 12)).foregroundStyle(.secondary)
                        }
                    }
                }
            }
            if model.sync.state == .off {
                Section { Text("Remote control needs sync. Set it up first.").foregroundStyle(.orange) }
            }
        }
        .scrollContentBackground(.hidden).background(PodBackground())
        .navigationTitle("Remote control").navigationBarTitleDisplayMode(.inline)
        .task { await model.refreshRemote() }
    }
}
