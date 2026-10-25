import SwiftUI

/// An agent's question: up to four choices it wrote, plus an "Other" row the user types into in place.
struct QuestionPanel: View {
    @Environment(AppModel.self) private var model
    let chatId: String

    private var question: PendingQuestion? { model.questions.first { $0.chatId == chatId } }

    var body: some View {
        Group {
            if let q = question {
                QuestionCard(question: q).id(q.id).transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .animation(.snappy, value: question?.id)
    }
}

private struct QuestionCard: View {
    @Environment(AppModel.self) private var model
    let question: PendingQuestion
    @State private var typing = false
    @State private var custom = ""
    @FocusState private var focused: Bool

    private var agent: Agent? { model.agent(question.agentId) }
    private var trimmed: String { custom.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                if let agent { AgentAvatar(agent: agent, size: 26) } else { Image(systemName: "questionmark.bubble").font(.system(size: 20)).foregroundStyle(.secondary) }
                VStack(alignment: .leading, spacing: 2) {
                    Text(agent.map { "\($0.name) is asking" } ?? "Question").font(.system(size: 12)).foregroundStyle(.secondary)
                    Text(question.question).font(.system(size: 15, weight: .medium)).fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 8)
                Button("Skip") { model.client?.skipQuestion(id: question.id) }
                    .font(.system(size: 13)).foregroundStyle(.secondary)
            }
            ForEach(Array(question.options.enumerated()), id: \.offset) { i, o in
                Button { model.client?.answerQuestion(id: question.id, answer: o.label, custom: false) } label: {
                    HStack(alignment: .top, spacing: 10) {
                        Text("\(i + 1)").font(.system(size: 11, weight: .semibold, design: .rounded)).foregroundStyle(.secondary)
                            .frame(width: 20, height: 20).background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 5, style: .continuous))
                        VStack(alignment: .leading, spacing: 1) {
                            Text(o.label).font(.system(size: 14, weight: .medium)).foregroundStyle(.primary)
                            if let d = o.description { Text(d).font(.system(size: 12)).foregroundStyle(.secondary) }
                        }
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 12).padding(.vertical, 9)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Color.primary.opacity(0.12), lineWidth: 0.7))
                    .contentShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .buttonStyle(.plain)
            }
            if typing {
                HStack(spacing: 8) {
                    Image(systemName: "pencil").font(.system(size: 13)).foregroundStyle(.secondary)
                    TextField("Type your own answer", text: $custom, axis: .vertical)
                        .lineLimit(1...4).focused($focused).submitLabel(.send).onSubmit(submit)
                    Button(action: submit) { Image(systemName: "arrow.up.circle.fill").font(.system(size: 26)) }
                        .disabled(trimmed.isEmpty)
                }
                .padding(.horizontal, 12).padding(.vertical, 6)
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Color.accentColor.opacity(0.6), lineWidth: 1))
            } else {
                Button { typing = true; focused = true } label: {
                    Label("Other…", systemImage: "pencil").font(.system(size: 14)).foregroundStyle(.secondary)
                        .padding(.horizontal, 12).padding(.vertical, 9).frame(maxWidth: .infinity, alignment: .leading)
                        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Color.primary.opacity(0.12), style: StrokeStyle(lineWidth: 0.7, dash: [4, 3])))
                        .contentShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .buttonStyle(.plain)
            }
        }
        .padding(14)
        .glass(radius: 18)
        .padding(.horizontal, 12).padding(.bottom, 4)
    }

    private func submit() {
        guard !trimmed.isEmpty else { return }
        model.client?.answerQuestion(id: question.id, answer: trimmed, custom: true)
    }
}
