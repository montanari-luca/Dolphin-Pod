import SwiftUI

struct SyncView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Group {
            switch model.sync.state {
            case .off, .revoked: SyncSetupForm()
            case .locked: UnlockForm()
            default: SyncActive()
            }
        }
        .scrollContentBackground(.hidden).background(PodBackground())
        .navigationTitle("Sync").navigationBarTitleDisplayMode(.inline)
    }
}

struct SyncSetupForm: View {
    @Environment(AppModel.self) private var model
    @State private var url = ""
    @State private var pass = ""
    @State private var busy = false
    @State private var recovery: String?
    @State private var showRecover = false

    var body: some View {
        List {
            if model.sync.state == .revoked {
                Section { Text("This device was removed from the pod, so it no longer syncs. Your local data is untouched.").foregroundStyle(.orange) }
            }
            Section {
                SecureField("postgresql://user:password@host/db", text: $url)
                    .textInputAutocapitalization(.never).autocorrectionDisabled().font(.system(size: 14, design: .monospaced))
            } header: { Text("Database URL") } footer: {
                Text("The same Postgres connection string you used on your computer (NeonDB works). Stored in the iOS keychain.")
            }
            Section {
                SecureField("Passphrase", text: $pass)
            } header: { Text("Passphrase") } footer: {
                Text("Everything is encrypted on this device with it before it leaves. Use the same one on every device. Chats, agents, settings and provider keys then appear here.")
            }
            Section {
                Button { Task { await start() } } label: {
                    HStack { Text("Turn on sync"); Spacer(); if busy { ProgressView() } }
                }
                .disabled(busy || url.trimmingCharacters(in: .whitespaces).isEmpty || pass.count < 8)
                Button("I forgot my passphrase") { showRecover = true }.font(.system(size: 14))
            }
        }
        .sheet(item: Binding(get: { recovery.map(RecoveryWords.init) }, set: { _ in recovery = nil })) { RecoveryKeyView(words: $0.words) }
        .sheet(isPresented: $showRecover) { RecoverForm() }
    }

    private func start() async {
        guard let c = model.client else { return }
        busy = true; defer { busy = false }
        do {
            let r = try await c.syncSetup(url: url.trimmingCharacters(in: .whitespaces), passphrase: pass)
            pass = ""
            if let k = r.recoveryKey { recovery = k } else { model.toast("info", "Sync is on. Your pod is arriving…") }
        } catch { model.toast("error", "\(error)") }
    }
}

struct RecoveryWords: Identifiable { let words: String; var id: String { words } }

struct RecoveryKeyView: View {
    @Environment(\.dismiss) private var dismiss
    let words: String
    @State private var saved = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text("If you forget your passphrase, these 24 words are the only way back in. They are shown once and stored nowhere. Write them down or keep them in a password manager.")
                        .font(.system(size: 14)).foregroundStyle(.secondary)
                    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], alignment: .leading, spacing: 8) {
                        ForEach(Array(words.split(separator: " ").enumerated()), id: \.offset) { i, w in
                            HStack(spacing: 8) {
                                Text("\(i + 1)").font(.system(size: 12, design: .monospaced)).foregroundStyle(.tertiary).frame(width: 22, alignment: .trailing)
                                Text(w).font(.system(size: 15, design: .monospaced)).textSelection(.enabled)
                            }
                        }
                    }
                    .padding(14).glass()
                    #if canImport(UIKit)
                    Button { UIPasteboard.general.string = words } label: { Label("Copy words", systemImage: "doc.on.doc") }
                    #endif
                    Toggle("I saved my recovery key", isOn: $saved)
                }
                .padding(16)
            }
            .background(PodBackground())
            .navigationTitle("Recovery key").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() }.disabled(!saved) } }
            .interactiveDismissDisabled(!saved)
        }
    }
}

struct RecoverForm: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var url = ""
    @State private var words = ""
    @State private var pass = ""
    @State private var busy = false

    var body: some View {
        NavigationStack {
            List {
                Section("Database URL") { SecureField("postgresql://…", text: $url).textInputAutocapitalization(.never).autocorrectionDisabled() }
                Section("Recovery key (24 words)") { TextField("word word word …", text: $words, axis: .vertical).lineLimit(3...6).textInputAutocapitalization(.never).autocorrectionDisabled() }
                Section("New passphrase") { SecureField("At least 8 characters", text: $pass) }
                Section {
                    Button { Task { await run() } } label: { HStack { Text("Recover"); Spacer(); if busy { ProgressView() } } }
                        .disabled(busy || url.isEmpty || words.split(separator: " ").count < 24 || pass.count < 8)
                }
            }
            .navigationTitle("Recover").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button("Cancel") { dismiss() } } }
        }
    }

    private func run() async {
        busy = true; defer { busy = false }
        do {
            try await model.client?.syncRecover(url: url.trimmingCharacters(in: .whitespaces), recoveryKey: words, newPassphrase: pass)
            model.toast("info", "Recovered. Sync is on with your new passphrase.")
            dismiss()
        } catch { model.toast("error", "\(error)") }
    }
}

struct UnlockForm: View {
    @Environment(AppModel.self) private var model
    @State private var pass = ""
    @State private var busy = false

    var body: some View {
        List {
            Section {
                SecureField("Passphrase", text: $pass)
                Button { Task { await unlock() } } label: { HStack { Text("Unlock"); Spacer(); if busy { ProgressView() } } }.disabled(pass.isEmpty || busy)
            } footer: { Text("The key saved on this device is gone, for example after a keychain reset. Enter your passphrase to keep syncing.") }
        }
    }

    private func unlock() async {
        busy = true; defer { busy = false }
        do { try await model.client?.syncUnlock(pass); pass = "" } catch { model.toast("error", "\(error)") }
    }
}

struct SyncActive: View {
    @Environment(AppModel.self) private var model
    @State private var name = ""
    @State private var showPassphrase = false
    @State private var showOff = false
    @State private var newRecovery: String?
    @State private var askPassForRecovery = false
    @State private var recoveryPass = ""
    @State private var revoke: DeviceInfo?

    var body: some View {
        List {
            Section {
                HStack {
                    Circle().fill(stateColor).frame(width: 9, height: 9)
                    Text(stateLabel).font(.system(size: 16, weight: .semibold))
                    Spacer()
                    Button { Task { try? await model.client?.syncNow(); await model.refreshRemote() } } label: { Label("Sync now", systemImage: "arrow.triangle.2.circlepath") }
                        .font(.system(size: 14)).disabled(model.sync.state == .syncing)
                }
                LabeledContent("Database", value: model.sync.provider ?? "—")
                LabeledContent("Last sync", value: Fmt.ago(model.sync.lastSyncAt))
                if model.sync.pending > 0 { LabeledContent("Waiting to send", value: "\(model.sync.pending)") }
                if let e = model.sync.error { Text(e).font(.system(size: 12.5)).foregroundStyle(.red).textSelection(.enabled) }
            }
            Section("This device") {
                HStack {
                    TextField("Device name", text: $name).onSubmit { try? model.client?.setDeviceName(name) }
                }
            }
            Section("Devices") {
                ForEach(model.devices) { d in
                    HStack(spacing: 12) {
                        Image(systemName: d.kind == "phone" ? "iphone" : "desktopcomputer").foregroundStyle(.secondary).frame(width: 24)
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: 6) {
                                Text(d.name).font(.system(size: 15, weight: .medium))
                                if d.thisDevice { Tag(text: "this device") }
                                if d.revoked { Tag(text: "removed", tint: .red) }
                            }
                            Text("\(d.platform) · \(d.thisDevice ? "active now" : "seen \(Fmt.ago(d.lastSeen))")").font(.system(size: 12)).foregroundStyle(.secondary)
                        }
                        Spacer()
                        if !d.thisDevice && !d.revoked { Button("Remove", role: .destructive) { revoke = d }.font(.system(size: 13)) }
                    }
                }
            }
            Section("Security") {
                Button { showPassphrase = true } label: { Label("Change passphrase", systemImage: "lock.rotation") }
                Button { askPassForRecovery = true } label: { Label("Create a new recovery key", systemImage: "key") }
                Button(role: .destructive) { showOff = true } label: { Label("Turn off sync on this iPhone", systemImage: "icloud.slash") }
            }
        }
        .onAppear { name = model.sync.deviceName }
        .task { await model.refreshRemote() }
        .sheet(isPresented: $showPassphrase) { ChangePassphraseForm() }
        .sheet(item: Binding(get: { newRecovery.map(RecoveryWords.init) }, set: { _ in newRecovery = nil })) { RecoveryKeyView(words: $0.words) }
        .alert("Turn off sync?", isPresented: $showOff) {
            Button("Turn off", role: .destructive) { try? model.client?.syncDisable() }
            Button("Cancel", role: .cancel) {}
        } message: { Text("Chats stay here, but new messages stop reaching your other devices.") }
        .alert("Confirm with your passphrase", isPresented: $askPassForRecovery) {
            SecureField("Passphrase", text: $recoveryPass)
            Button("Create") {
                let p = recoveryPass; recoveryPass = ""
                Task { do { newRecovery = try await model.client?.syncNewRecoveryKey(passphrase: p) } catch { model.toast("error", "\(error)") } }
            }
            Button("Cancel", role: .cancel) { recoveryPass = "" }
        } message: { Text("The old recovery key stops working.") }
        .confirmationDialog("Remove \(revoke?.name ?? "device")?", isPresented: Binding(get: { revoke != nil }, set: { if !$0 { revoke = nil } }), titleVisibility: .visible) {
            Button("Remove device", role: .destructive) {
                if let d = revoke { Task { try? await model.client?.syncRevokeDevice(d.id); await model.refreshRemote() } }
            }
        } message: { Text("It stops syncing the next time it connects.") }
    }

    private var stateLabel: String {
        switch model.sync.state {
        case .idle: return "Up to date"
        case .syncing: return "Syncing…"
        case .offline: return "Offline"
        case .error: return "Problem"
        default: return model.sync.state.rawValue.capitalized
        }
    }

    private var stateColor: Color {
        switch model.sync.state {
        case .idle: return .green
        case .syncing: return Brand.blue
        case .offline: return .orange
        default: return .red
        }
    }
}

struct ChangePassphraseForm: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var old = ""
    @State private var new = ""
    @State private var again = ""
    @State private var busy = false

    var body: some View {
        NavigationStack {
            List {
                Section { SecureField("Current passphrase", text: $old) }
                Section { SecureField("New passphrase (8+ characters)", text: $new); SecureField("New passphrase again", text: $again) }
                Section {
                    Button { Task { await run() } } label: { HStack { Text("Change passphrase"); Spacer(); if busy { ProgressView() } } }
                        .disabled(busy || old.isEmpty || new.count < 8 || new != again)
                } footer: { Text("Other devices keep working; a device that has to sign in again needs the new passphrase.") }
            }
            .navigationTitle("Change passphrase").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button("Cancel") { dismiss() } } }
        }
    }

    private func run() async {
        busy = true; defer { busy = false }
        do { try await model.client?.syncChangePassphrase(old: old, new: new); model.toast("info", "Passphrase changed"); dismiss() }
        catch { model.toast("error", "\(error)") }
    }
}
