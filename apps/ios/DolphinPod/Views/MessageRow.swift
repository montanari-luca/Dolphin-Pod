import SwiftUI

struct MessageRow: View {
    @Environment(AppModel.self) private var model
    let message: Message
    let grouped: Bool
    var onAgentTap: (Agent) -> Void

    var body: some View {
        switch message.kind {
        case .notice: NoticeRow(message: message)
        case .ack: AckRow(message: message, agent: model.agent(message.agentId), onAgentTap: onAgentTap)
        default: contentRow
        }
    }

    private var agent: Agent? { model.agent(message.agentId) }

    private var contentRow: some View {
        HStack(alignment: .top, spacing: 10) {
            Group {
                if grouped { Color.clear } else if message.isUser {
                    UserAvatar(size: 36)
                } else if let agent {
                    AgentAvatar(agent: agent, size: 36, busy: message.isStreaming)
                        .onTapGesture { onAgentTap(agent) }
                        .onLongPressGesture { onAgentTap(agent) }
                } else { Color.clear }
            }
            .frame(width: 36, height: 36)

            VStack(alignment: .leading, spacing: 6) {
                if !grouped { header }
                if let r = message.reasoning, !r.isEmpty { ThinkingBlock(text: r, streaming: message.isStreaming && message.content.isEmpty, thinkingMs: message.thinkingMs) }
                if !message.content.isEmpty { MarkdownView(text: message.content) }
                else if message.isStreaming && (message.reasoning ?? "").isEmpty { TypingDots() }
                ForEach(message.attachments) { AttachmentChip(attachment: $0) }
                ToolList(meta: message.meta)
                if message.status == .error { ErrorBox(message: message) }
                if message.kind == .plan && message.status == .done { PlanBar(message: message) }
                if message.status == .cancelled { Text("Stopped").font(.caption).foregroundStyle(.secondary) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 16)
        .padding(.top, grouped ? 1 : 10)
        .padding(.bottom, 2)
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            if message.isUser { UserNameText() } else if let agent { AgentNameText(agent: agent) } else { Text("Unknown").font(.system(size: 15, weight: .semibold)) }
            if !message.isUser, let mode = message.routingMode, mode != "answer" { Tag(text: mode.capitalized, tint: mode == "plan" ? .orange : Brand.blue) }
            if message.isUser && message.isRemote {
                Tag(text: message.remoteDevice.map { "via \($0)" } ?? message.remoteTarget.map { "sent to \($0)" } ?? "remote", tint: Brand.blue, symbol: "antenna.radiowaves.left.and.right")
            }
            Text(Fmt.time(message.createdAt)).font(.system(size: 11)).foregroundStyle(.tertiary)
        }
    }
}

struct Tag: View {
    let text: String
    var tint: Color = Brand.blue
    var symbol: String?

    var body: some View {
        HStack(spacing: 3) {
            if let symbol { Image(systemName: symbol).font(.system(size: 9, weight: .semibold)) }
            Text(text).font(.system(size: 10.5, weight: .semibold))
        }
        .foregroundStyle(tint)
        .padding(.horizontal, 7).padding(.vertical, 2)
        .background(tint.opacity(0.14), in: Capsule())
    }
}

struct TypingDots: View {
    @State private var phase = false
    var body: some View {
        HStack(spacing: 4) {
            ForEach(0..<3, id: \.self) { i in
                Circle().fill(Color.secondary).frame(width: 6, height: 6)
                    .opacity(phase ? 1 : 0.3)
                    .animation(.easeInOut(duration: 0.6).repeatForever().delay(Double(i) * 0.18), value: phase)
            }
        }
        .padding(.vertical, 6)
        .onAppear { phase = true }
    }
}

struct ThinkingBlock: View {
    let text: String
    let streaming: Bool
    let thinkingMs: Int64?
    @State private var open = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button { withAnimation(.snappy) { open.toggle() } } label: {
                HStack(spacing: 6) {
                    Image(systemName: "brain").font(.system(size: 12, weight: .semibold))
                    Text(streaming ? "Thinking…" : (thinkingMs.map { "Thought for \(Fmt.duration($0))" } ?? "Thoughts"))
                        .font(.system(size: 12.5, weight: .medium))
                    Image(systemName: "chevron.right").font(.system(size: 9, weight: .bold)).rotationEffect(.degrees(open || streaming ? 90 : 0))
                }
                .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            if open || streaming {
                Text(text).font(.system(size: 13)).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
                    .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            }
        }
    }
}

struct ToolList: View {
    let meta: JSON

    var body: some View {
        let tools = meta["tools"].array ?? []
        if !tools.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(tools.enumerated()), id: \.offset) { _, t in
                    let ok = t["ok"].bool ?? true
                    HStack(spacing: 6) {
                        Image(systemName: ok ? "wrench.and.screwdriver.fill" : "exclamationmark.triangle.fill")
                            .font(.system(size: 10)).foregroundStyle(ok ? Color.secondary : Color.orange)
                        Text(t["summary"].string ?? t["name"].string ?? "tool").font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(2)
                    }
                }
            }
        }
    }
}

struct AttachmentChip: View {
    let attachment: Attachment
    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: attachment.kind == .image ? "photo" : "doc.fill").font(.system(size: 12))
            Text(attachment.name).font(.system(size: 12.5, weight: .medium)).lineLimit(1)
            Text(ByteCountFormatter.string(fromByteCount: attachment.size, countStyle: .file)).font(.system(size: 11)).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10).padding(.vertical, 6)
        .background(Color.primary.opacity(0.07), in: Capsule())
    }
}

struct ErrorBox: View {
    let message: Message
    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
            Text(message.error ?? "Something went wrong.").font(.system(size: 13)).textSelection(.enabled)
        }
        .padding(10)
        .background(Color.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
    }
}

struct PlanBar: View {
    @Environment(AppModel.self) private var model
    let message: Message

    var body: some View {
        let approved = message.meta["plan"]["approved"].bool ?? false
        HStack(spacing: 10) {
            Image(systemName: approved ? "checkmark.seal.fill" : "list.clipboard.fill").foregroundStyle(approved ? Color.green : Brand.blue)
            Text(approved ? "Plan approved" : "Read-only plan. Approve to let the agent act.").font(.system(size: 13))
            Spacer(minLength: 0)
            if !approved {
                Button("Approve") { model.approvePlan(message.id) }
                    .buttonStyle(.borderedProminent).tint(Brand.blue).controlSize(.small)
            }
        }
        .padding(10)
        .glass(radius: 12)
    }
}

struct NoticeRow: View {
    let message: Message
    var body: some View {
        Text(message.content)
            .font(.system(size: 12.5)).foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 8).padding(.horizontal, 24)
    }
}

struct AckRow: View {
    let message: Message
    let agent: Agent?
    var onAgentTap: (Agent) -> Void

    var body: some View {
        HStack(spacing: 8) {
            if let agent {
                AgentAvatar(agent: agent, size: 22).onTapGesture { onAgentTap(agent) }
                Text(message.content).font(.system(size: 13.5)).foregroundStyle(.secondary).italic()
            }
        }
        .padding(.horizontal, 16).padding(.vertical, 4)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
