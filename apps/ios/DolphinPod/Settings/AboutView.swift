import SwiftUI

struct AboutView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    @State private var checking = false
    @State private var result: String?

    var body: some View {
        ScrollView {
            VStack(spacing: 18) {
                VStack(spacing: 8) {
                    Image("DolphinMark").resizable().scaledToFit().frame(width: 96)
                    Text("Dolphin Pod").font(.system(size: 28, weight: .bold)).foregroundStyle(Brand.gradient)
                    Text("Your pod of AI agents, in one chat.").foregroundStyle(.secondary)
                    HStack { Tag(text: "Version \(model.version)"); Tag(text: "iOS") }
                }
                .padding(.top, 20)

                VStack(spacing: 8) {
                    Button { Task { await check() } } label: { Label(checking ? "Checking…" : "Check for updates", systemImage: "arrow.triangle.2.circlepath") }
                        .buttonStyle(.bordered).disabled(checking)
                    if let u = model.update, u.available, let url = u.url, let link = URL(string: url) {
                        Button("Get version \(u.latest)") { openURL(link) }.buttonStyle(.borderedProminent)
                    }
                    if let result { Text(result).font(.system(size: 12.5)).foregroundStyle(.secondary) }
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text("MADE BY").font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary).padding(.bottom, 6)
                    Link(destination: URL(string: "https://mintflow.dev")!) {
                        HStack(spacing: 12) {
                            Image("MintFlowMark").resizable().scaledToFit().frame(width: 42, height: 42).clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            VStack(alignment: .leading) {
                                Text("MintFlow Technologies").font(.system(size: 15, weight: .semibold)).foregroundStyle(.primary)
                                Text("mintflow.dev").font(.system(size: 12.5)).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Image(systemName: "arrow.up.right").foregroundStyle(.tertiary)
                        }
                    }
                    Divider().padding(.vertical, 4)
                    Link(destination: URL(string: "https://instagram.com/montanarii.luca")!) {
                        HStack(spacing: 12) {
                            Image(systemName: "at").font(.system(size: 20, weight: .semibold)).foregroundStyle(.white)
                                .frame(width: 42, height: 42).background(Brand.gradient, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                            VStack(alignment: .leading) {
                                Text("Luca Montanari").font(.system(size: 15, weight: .semibold)).foregroundStyle(.primary)
                                Text("Lead developer · @montanarii.luca").font(.system(size: 12.5)).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Image(systemName: "arrow.up.right").foregroundStyle(.tertiary)
                        }
                    }
                }
                .padding(16).glass()

                Text("© 2026 MintFlow Technologies. All rights reserved.").font(.system(size: 11.5)).foregroundStyle(.tertiary)
            }
            .padding(16)
        }
        .background(PodBackground())
        .navigationTitle("About").navigationBarTitleDisplayMode(.inline)
    }

    private func check() async {
        checking = true; defer { checking = false }
        do {
            let u = try await model.client?.checkUpdate()
            if let u, u.available { model.update = u; result = "Version \(u.latest) is available." } else { result = "You're up to date (\(u?.current ?? model.version))." }
        } catch { result = "\(error)" }
    }
}
