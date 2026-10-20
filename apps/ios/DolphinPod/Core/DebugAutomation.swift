#if DEBUG
import Foundation

/// Development hooks so the app can be driven from the command line (simulator tests, screenshots).
/// Pass them with `SIMCTL_CHILD_` env vars, e.g. `SIMCTL_CHILD_DOLPHIN_SYNC_URL=postgres://… xcrun simctl launch …`.
/// Compiled out of release builds.
///
/// - `DOLPHIN_SYNC_URL`, `DOLPHIN_SYNC_PASS`: turn sync on with these.
/// - `DOLPHIN_OPEN_CHAT`: open the chat with this name.
/// - `DOLPHIN_TARGET`: `desktop` to wait for a desktop and send there instead of this iPhone.
/// - `DOLPHIN_SEND`: a message to send in the opened chat.
enum DebugAutomation {
    @MainActor
    static func run(_ model: AppModel) async {
        let env = ProcessInfo.processInfo.environment
        guard let c = model.client else { return }
        if let url = env["DOLPHIN_SYNC_URL"], let pass = env["DOLPHIN_SYNC_PASS"], model.sync.state == .off {
            do {
                let r = try await c.syncSetup(url: url, passphrase: pass)
                print("[debug] sync setup ok, created=\(r.created)")
            } catch { print("[debug] sync setup failed: \(error)") }
        }
        if env["DOLPHIN_DUMP"] == "agents" {
            try? await Task.sleep(for: .seconds(8))
            for a in (try? c.agents()) ?? [] { print("[debug] agent \(a.name): model=\(a.model?.modelId ?? "nil") ios=\(a.iosModel?.modelId ?? "nil")") }
        }
        if let name = env["DOLPHIN_OPEN_CHAT"] {
            for _ in 0..<60 {
                if let chat = model.chats.first(where: { $0.name == name }) { model.selectedChatId = chat.id; model.openChat(chat.id); break }
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
        if env["DOLPHIN_TARGET"] == "desktop" {
            for _ in 0..<120 {
                await model.refreshRemote()
                if let d = model.remoteTargets.first { model.runTarget = .desktop(id: d.id, name: d.name); break }
                c.syncWake()
                try? await Task.sleep(for: .milliseconds(1000))
            }
            print("[debug] run target: \(model.runTarget.label)")
        }
        if let text = env["DOLPHIN_SEND"], let id = model.selectedChatId {
            await model.send(chatId: id, text: text, attachments: [])
            print("[debug] sent via \(model.lastRoute?.path ?? "phone")")
        }
    }
}
#endif
