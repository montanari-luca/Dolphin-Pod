import SwiftUI

struct ChatListView: View {
    @Environment(AppModel.self) private var model
    @Binding var selection: String?
    @State private var showNew = false
    @State private var newName = ""
    @State private var showSettings = false
    @State private var toDelete: Chat?

    var body: some View {
        List(selection: $selection) {
            if model.sync.state == .off { SyncPrompt { showSettings = true }.listRowBackground(Color.clear).listRowSeparator(.hidden) }
            Section {
                ForEach(model.chats) { chat in
                    NavigationLink(value: chat.id) { ChatRow(chat: chat) }
                        .listRowBackground(Color.clear)
                        .swipeActions(edge: .trailing) {
                            Button(role: .destructive) { toDelete = chat } label: { Label("Delete", systemImage: "trash") }
                            Button { model.cleanChat(chat.id) } label: { Label("Clean", systemImage: "sparkles") }.tint(Brand.violet)
                        }
                }
            } header: { Text("Chats") }
        }
        .scrollContentBackground(.hidden)
        .background(PodBackground())
        .navigationTitle("Dolphin Pod")
        .toolbar {
            ToolbarItem(placement: .topBarLeading) { SyncBadge().onTapGesture { showSettings = true } }
            ToolbarItem(placement: .topBarTrailing) { Button { showNew = true } label: { Image(systemName: "square.and.pencil") } }
            ToolbarItem(placement: .topBarTrailing) { Button { showSettings = true } label: { Image(systemName: "gearshape") } }
        }
        .alert("New chat", isPresented: $showNew) {
            TextField("Name", text: $newName)
            Button("Create") { if !newName.trimmingCharacters(in: .whitespaces).isEmpty { model.createChat(name: newName) }; newName = "" }
            Button("Cancel", role: .cancel) { newName = "" }
        }
        .confirmationDialog("Delete \(toDelete?.name ?? "chat")?", isPresented: Binding(get: { toDelete != nil }, set: { if !$0 { toDelete = nil } }), titleVisibility: .visible) {
            Button("Delete chat", role: .destructive) { if let c = toDelete { model.deleteChat(c.id); if selection == c.id { selection = nil } } }
        } message: { Text("It is deleted on all your synced devices.") }
        .sheet(isPresented: $showSettings) { SettingsView() }
    }
}

struct ChatRow: View {
    @Environment(AppModel.self) private var model
    let chat: Chat

    var body: some View {
        let last = model.messages[chat.id]?.last(where: { $0.kind == .text })
        HStack(spacing: 12) {
            ChatGlyph(chat: chat, size: 42)
            VStack(alignment: .leading, spacing: 3) {
                Text(chat.name).font(.system(size: 16.5, weight: .semibold))
                Text(last.map { ($0.isUser ? "You: " : "") + $0.content.replacingOccurrences(of: "\n", with: " ") } ?? "\(chat.agentIds.count) agents")
                    .font(.system(size: 13.5)).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 0)
            let busy = !model.streamingAgents(in: chat.id).isEmpty
            if busy { ProgressView().controlSize(.small) }
        }
        .padding(.vertical, 4)
        .onAppear { if model.messages[chat.id] == nil { model.openChat(chat.id) } }
    }
}

struct SyncPrompt: View {
    var action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: "icloud.and.arrow.down.fill").font(.system(size: 22)).foregroundStyle(Brand.gradient)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Bring your chats here").font(.system(size: 15, weight: .semibold))
                    Text("Turn on sync with the same database and passphrase as your computer.").font(.system(size: 12.5)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right").font(.system(size: 12, weight: .semibold)).foregroundStyle(.tertiary)
            }
            .padding(14).glass()
        }.buttonStyle(.plain)
    }
}

/// A small status dot in the navigation bar.
struct SyncBadge: View {
    @Environment(AppModel.self) private var model
    var body: some View {
        HStack(spacing: 5) {
            Circle().fill(color).frame(width: 8, height: 8)
            if model.sync.state == .syncing { ProgressView().controlSize(.mini) }
        }
        .padding(.horizontal, 8).padding(.vertical, 6)
        .accessibilityLabel("Sync \(model.sync.state.rawValue)")
    }

    private var color: Color {
        switch model.sync.state {
        case .idle: return .green
        case .syncing: return Brand.blue
        case .offline, .locked: return .orange
        case .error, .revoked: return .red
        case .off: return .gray
        }
    }
}
