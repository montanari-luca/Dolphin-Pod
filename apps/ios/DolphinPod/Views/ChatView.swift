import SwiftUI

struct ChatView: View {
    @Environment(AppModel.self) private var model
    let chatId: String
    @State private var cardAgent: Agent?
    @State private var showCleanConfirm = false

    private var chat: Chat? { model.chat(chatId) }
    private var messages: [Message] { model.messages[chatId] ?? [] }

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 0) {
                        if messages.isEmpty { EmptyChat(chat: chat) }
                        ForEach(Array(messages.enumerated()), id: \.element.id) { i, m in
                            MessageRow(message: m, grouped: isGrouped(i), onAgentTap: { cardAgent = $0 }).id(m.id)
                        }
                        ActivityBar(chatId: chatId).id("bottom")
                    }
                    .padding(.vertical, 8)
                }
                .scrollDismissesKeyboard(.interactively)
                .defaultScrollAnchor(.bottom)
                .onChange(of: messages.last?.id) { _, _ in scroll(proxy) }
                .onChange(of: messages.last?.content.count) { _, _ in scroll(proxy, animated: false) }
                .onAppear { scroll(proxy, animated: false) }
            }
            Composer(chatId: chatId)
        }
        .background(PodBackground())
        .navigationTitle(chat?.name ?? "Chat")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button { showCleanConfirm = true } label: { Label("Clean this chat", systemImage: "sparkles") }
                } label: { Image(systemName: "ellipsis.circle") }
            }
            ToolbarItem(placement: .topBarTrailing) { MembersButton(chatId: chatId) { cardAgent = $0 } }
        }
        .confirmationDialog("Clean this chat?", isPresented: $showCleanConfirm, titleVisibility: .visible) {
            Button("Remove all messages", role: .destructive) { model.cleanChat(chatId) }
        } message: { Text("Messages are removed on all your synced devices.") }
        .sheet(item: $cardAgent) { AgentCardView(agent: $0).presentationDetents([.medium, .large]) }
        .onAppear { model.openChat(chatId) }
    }

    private func isGrouped(_ i: Int) -> Bool {
        guard i > 0 else { return false }
        let a = messages[i - 1], b = messages[i]
        guard a.kind == .text || a.kind == .plan, b.kind == .text || b.kind == .plan else { return false }
        return a.agentId == b.agentId && b.createdAt - a.createdAt < 5 * 60_000
    }

    private func scroll(_ proxy: ScrollViewProxy, animated: Bool = true) {
        if animated { withAnimation(.snappy) { proxy.scrollTo("bottom", anchor: .bottom) } } else { proxy.scrollTo("bottom", anchor: .bottom) }
    }
}

extension Agent: Hashable {
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
}

struct EmptyChat: View {
    let chat: Chat?
    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: "bubble.left.and.bubble.right.fill").font(.system(size: 34)).foregroundStyle(Brand.gradient)
            Text("Say something").font(.system(size: 17, weight: .semibold))
            Text("Roger answers first, then the right specialist picks it up. Use @name to talk to one directly.")
                .font(.system(size: 13.5)).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .padding(.horizontal, 40).padding(.top, 80)
    }
}

/// "Rick is thinking…" rows for agents that are working right now.
struct ActivityBar: View {
    @Environment(AppModel.self) private var model
    let chatId: String

    var body: some View {
        let working = model.streamingAgents(in: chatId)
        VStack(alignment: .leading, spacing: 4) {
            ForEach(working, id: \.0.id) { agent, state in
                HStack(spacing: 8) {
                    AgentAvatar(agent: agent, size: 20, busy: true)
                    Text("\(agent.name) \(verb(state))").font(.system(size: 12.5)).foregroundStyle(.secondary)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16).padding(.top, working.isEmpty ? 0 : 6)
        .frame(minHeight: 4)
    }

    private func verb(_ s: ActivityState) -> String {
        switch s.activity {
        case .routing: return "is deciding who should answer…"
        case .thinking: return "is thinking…"
        case .writing: return "is writing…"
        case .usingTool: return s.detail.map { "is working: \($0)" } ?? "is working…"
        case .stuck: return "seems stuck"
        case .idle: return ""
        }
    }
}

struct MembersButton: View {
    @Environment(AppModel.self) private var model
    let chatId: String
    var onTap: (Agent) -> Void

    var body: some View {
        Menu {
            if let chat = model.chat(chatId) {
                ForEach(model.members(of: chat)) { a in
                    Button { onTap(a) } label: { Label(a.name, systemImage: Icons.specialty(a.specialty)) }
                }
            }
        } label: { Image(systemName: "person.2") }
    }
}
