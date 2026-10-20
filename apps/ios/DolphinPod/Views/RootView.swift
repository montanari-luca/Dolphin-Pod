import SwiftUI

struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var selection: String?

    var body: some View {
        ZStack(alignment: .top) {
            if let error = model.bootError {
                BootErrorView(message: error)
            } else if !model.ready {
                ZStack { PodBackground(); ProgressView() }
            } else {
                NavigationSplitView {
                    ChatListView(selection: $selection)
                } detail: {
                    if let id = selection { ChatView(chatId: id).id(id) } else { Welcome() }
                }
                .onAppear { if selection == nil, UIDevice.current.userInterfaceIdiom != .phone { selection = model.chats.first?.id } }
            }
            ToastStack()
        }
        .alert(model.confirmation?.title ?? "", isPresented: Binding(get: { model.confirmation != nil }, set: { _ in })) {
            Button("Allow") { if let c = model.confirmation { model.client?.resolveConfirmation(id: c.id, accepted: true) }; model.confirmation = nil }
            Button("Deny", role: .cancel) { if let c = model.confirmation { model.client?.resolveConfirmation(id: c.id, accepted: false) }; model.confirmation = nil }
        } message: { Text(model.confirmation?.body ?? "") }
    }
}

struct Welcome: View {
    var body: some View {
        ZStack {
            PodBackground()
            VStack(spacing: 10) {
                Image("DolphinMark").resizable().scaledToFit().frame(width: 84)
                Text("Dolphin Pod").font(.system(size: 26, weight: .bold)).foregroundStyle(Brand.gradient)
                Text("Pick a chat to start.").foregroundStyle(.secondary)
            }
        }
    }
}

struct BootErrorView: View {
    let message: String
    var body: some View {
        ZStack {
            PodBackground()
            VStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 36)).foregroundStyle(.orange)
                Text("Dolphin Pod couldn't start").font(.system(size: 18, weight: .semibold))
                Text(message).font(.system(size: 13)).foregroundStyle(.secondary).multilineTextAlignment(.center).textSelection(.enabled)
            }.padding(32)
        }
    }
}

struct ToastStack: View {
    @Environment(AppModel.self) private var model
    var body: some View {
        VStack(spacing: 8) {
            ForEach(model.toasts) { t in
                Text(t.text).font(.system(size: 13.5, weight: .medium)).multilineTextAlignment(.leading)
                    .padding(.horizontal, 14).padding(.vertical, 10)
                    .background(t.level == "error" ? AnyShapeStyle(Color.red.opacity(0.85)) : AnyShapeStyle(.regularMaterial), in: Capsule())
                    .foregroundStyle(t.level == "error" ? Color.white : Color.primary)
                    .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .padding(.top, 6).padding(.horizontal, 16)
        .animation(.snappy, value: model.toasts)
        .allowsHitTesting(false)
    }
}
