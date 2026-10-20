import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

struct Composer: View {
    @Environment(AppModel.self) private var model
    let chatId: String
    @State private var text = ""
    @State private var attachments: [Attachment] = []
    @State private var photoItems: [PhotosPickerItem] = []
    @State private var showFiles = false
    @State private var sending = false
    @FocusState private var focused: Bool

    private var remote: Bool { if case .desktop = model.runTarget { return true } else { return false } }
    private var working: Bool { !model.streamingAgents(in: chatId).isEmpty }
    private var canSend: Bool { !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !sending }

    var body: some View {
        VStack(spacing: 8) {
            if let route = model.lastRoute, remote { RouteChip(path: route.path, target: route.target).transition(.opacity) }
            MentionSuggestions(chatId: chatId, text: $text)
            if !attachments.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack { ForEach(attachments) { a in
                        AttachmentChip(attachment: a).overlay(alignment: .topTrailing) {
                            Button { attachments.removeAll { $0.id == a.id } } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
                                .buttonStyle(.plain).offset(x: 4, y: -4)
                        }
                    } }
                    .padding(.horizontal, 16)
                }
            }
            HStack(alignment: .bottom, spacing: 8) {
                Menu {
                    if !remote {
                        PhotosPicker(selection: $photoItems, maxSelectionCount: 4, matching: .images) { Label("Photo", systemImage: "photo") }
                        Button { showFiles = true } label: { Label("File", systemImage: "doc") }
                    } else {
                        Text("Attachments run on this iPhone only")
                    }
                } label: {
                    Image(systemName: "plus").font(.system(size: 17, weight: .semibold)).frame(width: 38, height: 38)
                        .glass(radius: 19)
                }
                .disabled(remote)

                HStack(alignment: .bottom, spacing: 6) {
                    TextField(remote ? "Ask \(model.runTarget.label)…" : "Message the pod…", text: $text, axis: .vertical)
                        .lineLimit(1...6)
                        .focused($focused)
                        .padding(.vertical, 9).padding(.leading, 14)
                    RunTargetMenu()
                    sendButton
                }
                .glass(radius: 20)
            }
            .padding(.horizontal, 12)
        }
        .padding(.top, 6).padding(.bottom, 8)
        .onChange(of: photoItems) { _, items in Task { await stage(items) } }
        .fileImporter(isPresented: $showFiles, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result else { return }
            for url in urls { stageFile(url) }
        }
    }

    @ViewBuilder private var sendButton: some View {
        if working && !remote {
            Button { model.stop(chatId: chatId) } label: {
                Image(systemName: "stop.fill").font(.system(size: 13, weight: .bold)).foregroundStyle(.white)
                    .frame(width: 30, height: 30).background(Color.red.gradient, in: Circle())
            }.padding(5)
        } else {
            Button(action: send) {
                Image(systemName: "arrow.up").font(.system(size: 14, weight: .bold)).foregroundStyle(.white)
                    .frame(width: 30, height: 30)
                    .background(canSend ? AnyShapeStyle(Brand.gradient) : AnyShapeStyle(Color.gray.opacity(0.4)), in: Circle())
            }
            .disabled(!canSend).padding(5)
        }
    }

    private func send() {
        let body = text
        let files = attachments
        text = ""; attachments = []; sending = true
        Task {
            await model.send(chatId: chatId, text: body, attachments: files)
            sending = false
        }
    }

    private func stage(_ items: [PhotosPickerItem]) async {
        for item in items {
            guard let data = try? await item.loadTransferable(type: Data.self) else { continue }
            let type = item.supportedContentTypes.first
            let ext = type?.preferredFilenameExtension ?? "jpg"
            if let a = try? model.client?.stageAttachment(name: "photo-\(attachments.count + 1).\(ext)", mime: type?.preferredMIMEType ?? "image/jpeg", data: data) { attachments.append(a) }
        }
        photoItems = []
    }

    private func stageFile(_ url: URL) {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard let data = try? Data(contentsOf: url) else { return }
        let mime = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
        if let a = try? model.client?.stageAttachment(name: url.lastPathComponent, mime: mime, data: data) { attachments.append(a) }
    }
}

/// Where this message runs: this iPhone, or one of the desktops that accepts remote requests.
struct RunTargetMenu: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Menu {
            Button { model.runTarget = .phone } label: {
                Label("This iPhone", systemImage: model.runTarget == .phone ? "checkmark" : "iphone")
            }
            ForEach(model.remoteTargets) { d in
                Button { model.runTarget = .desktop(id: d.id, name: d.name) } label: {
                    Label(d.name, systemImage: model.runTarget == .desktop(id: d.id, name: d.name) ? "checkmark" : "desktopcomputer")
                }
            }
            if model.remoteTargets.isEmpty {
                Text("No desktop online. Turn on Remote control on your computer.")
            }
        } label: {
            Image(systemName: model.runTarget == .phone ? "iphone" : "desktopcomputer")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(model.runTarget == .phone ? Color.secondary : Brand.blue)
                .frame(width: 30, height: 30)
        }
    }
}

struct RouteChip: View {
    let path: String
    let target: String
    var body: some View {
        HStack(spacing: 5) {
            Image(systemName: path == "lan" ? "wifi" : "cloud.fill").font(.system(size: 10, weight: .bold))
            Text(path == "lan" ? "Sent to \(target) over Wi-Fi" : "Sent to \(target) through the cloud").font(.system(size: 11.5, weight: .medium))
        }
        .foregroundStyle(Brand.blue)
        .padding(.horizontal, 10).padding(.vertical, 4)
        .background(Brand.blue.opacity(0.12), in: Capsule())
    }
}

/// `@name` autocomplete for the members of this chat.
struct MentionSuggestions: View {
    @Environment(AppModel.self) private var model
    let chatId: String
    @Binding var text: String

    private var query: String? {
        guard let at = text.lastIndex(of: "@") else { return nil }
        let tail = text[text.index(after: at)...]
        if tail.contains(where: { $0.isWhitespace }) { return nil }
        if at != text.startIndex, !text[text.index(before: at)].isWhitespace { return nil }
        return String(tail).lowercased()
    }

    var body: some View {
        if let q = query, let chat = model.chat(chatId) {
            let options = model.members(of: chat).filter { q.isEmpty || $0.name.lowercased().hasPrefix(q) }
            if !options.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(options) { a in
                            Button { complete(a) } label: {
                                HStack(spacing: 6) {
                                    AgentAvatar(agent: a, size: 22)
                                    Text(a.name).font(.system(size: 13.5, weight: .semibold))
                                }
                                .padding(.horizontal, 10).padding(.vertical, 6).glass(radius: 18)
                            }.buttonStyle(.plain)
                        }
                    }.padding(.horizontal, 16)
                }
            }
        }
    }

    private func complete(_ agent: Agent) {
        guard let at = text.lastIndex(of: "@") else { return }
        text = String(text[..<at]) + "@\(agent.name) "
    }
}
