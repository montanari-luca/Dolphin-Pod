import SwiftUI

/// The agent's card: job, all-time usage and timing. Phones have no hover, so it opens on tap or long-press.
struct AgentCardView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let agent: Agent

    var body: some View {
        let u = model.usage[agent.id]
        NavigationStack {
            ScrollView {
                VStack(spacing: 18) {
                    VStack(spacing: 10) {
                        AgentAvatar(agent: agent, size: 76)
                        AgentNameText(agent: agent, font: .system(size: 24, weight: .bold))
                        Tag(text: roleLabel, tint: agent.color.tint)
                        Text(agent.job).font(.system(size: 14.5)).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    }
                    .padding(.top, 8)

                    VStack(spacing: 0) {
                        StatRow(title: "Requests", value: "\(u?.requests ?? 0)")
                        StatRow(title: "Tokens in", value: Fmt.tokens(u?.tokensIn ?? 0))
                        StatRow(title: "Tokens out", value: Fmt.tokens(u?.tokensOut ?? 0))
                        StatRow(title: "Time thinking", value: Fmt.duration(u?.thinkingMs ?? 0))
                        StatRow(title: "Time working", value: Fmt.duration(u?.workingMs ?? 0))
                        if let last = u?.lastQueryMs { StatRow(title: "Last query took", value: Fmt.duration(last)) }
                    }
                    .glass()

                    VStack(alignment: .leading, spacing: 8) {
                        Text("MODEL ON THIS IPHONE").font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary)
                        if let m = model.effectiveModel(agent) {
                            Label(m.modelId, systemImage: agent.iosModel == m ? "iphone" : "cloud").font(.system(size: 14, weight: .medium))
                        } else if model.unavailableOnPhone(agent) {
                            Label("Runs on your computer", systemImage: "desktopcomputer").font(.system(size: 14, weight: .medium)).foregroundStyle(.orange)
                            Text("\(agent.name)'s model can't be reached from a phone. Pick an iPhone model in Settings → Agents, or send requests to your desktop.")
                                .font(.system(size: 12.5)).foregroundStyle(.secondary)
                        } else {
                            Label("No model yet", systemImage: "questionmark.circle").font(.system(size: 14, weight: .medium)).foregroundStyle(.secondary)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading).padding(16).glass()
                }
                .padding(16)
            }
            .background(PodBackground())
            .navigationTitle("").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() } } }
        }
    }

    private var roleLabel: String {
        switch agent.specialty {
        case "moderator": return "Moderator"
        case "orchestrator": return "Orchestrator"
        case "coder": return "Coding"
        case "long_horizon": return "Long horizon"
        case "security": return "Security"
        case "debug": return "Debugging"
        case "creative": return "Creative writing"
        case "docs": return "Documentation"
        case "image": return "Images"
        case "teacher": return "Teaching"
        default: return agent.specialty.capitalized
        }
    }
}

struct StatRow: View {
    let title: String
    let value: String
    var body: some View {
        HStack {
            Text(title).font(.system(size: 14.5))
            Spacer()
            Text(value).font(.system(size: 14.5, weight: .semibold)).monospacedDigit()
        }
        .padding(.horizontal, 16).padding(.vertical, 11)
        .overlay(alignment: .bottom) { Divider().opacity(0.4).padding(.leading, 16) }
    }
}
