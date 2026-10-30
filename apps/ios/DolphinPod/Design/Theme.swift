import SwiftUI

// MARK: - Colors

extension Color {
    init(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: "#", with: "")
        if s.count == 3 { s = s.map { "\($0)\($0)" }.joined() }
        var v: UInt64 = 0
        Scanner(string: s).scanHexInt64(&v)
        let hasAlpha = s.count == 8
        let r = Double((v >> (hasAlpha ? 24 : 16)) & 0xFF) / 255
        let g = Double((v >> (hasAlpha ? 16 : 8)) & 0xFF) / 255
        let b = Double((v >> (hasAlpha ? 8 : 0)) & 0xFF) / 255
        let a = hasAlpha ? Double(v & 0xFF) / 255 : 1
        self.init(.sRGB, red: r, green: g, blue: b, opacity: a)
    }

    /// Relative luminance, for choosing readable text on top of an agent's color.
    var luminance: Double {
        #if canImport(UIKit)
        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        UIColor(self).getRed(&r, green: &g, blue: &b, alpha: &a)
        func lin(_ c: CGFloat) -> Double { let c = Double(c); return c <= 0.03928 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
        return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
        #else
        return 0.5
        #endif
    }
}

enum Brand {
    static let blue = Color(hex: "#3d8bfd")
    static let violet = Color(hex: "#a77bdb")
    static let gradient = LinearGradient(colors: [blue, violet], startPoint: .topLeading, endPoint: .bottomTrailing)
    static let accent = blue
}

extension AgentColor {
    /// CSS-style gradient (angle 0 = up, clockwise) as a SwiftUI gradient.
    var gradient: LinearGradient {
        switch self {
        case .solid(let c):
            return LinearGradient(colors: [Color(hex: c)], startPoint: .top, endPoint: .bottom)
        case .gradient(let angle, let stops):
            let rad = angle * .pi / 180
            let dx = sin(rad), dy = -cos(rad)
            let ss = stops.map { Gradient.Stop(color: Color(hex: $0.color), location: max(0, min(1, $0.position / 100))) }
            return LinearGradient(stops: ss.isEmpty ? [.init(color: .gray, location: 0)] : ss,
                                  startPoint: UnitPoint(x: 0.5 - dx / 2, y: 0.5 - dy / 2),
                                  endPoint: UnitPoint(x: 0.5 + dx / 2, y: 0.5 + dy / 2))
        }
    }

    var firstColor: Color {
        switch self {
        case .solid(let c): return Color(hex: c)
        case .gradient(_, let stops): return Color(hex: stops.first?.color ?? "#7c8cff")
        }
    }

    /// A single representative color (middle of a gradient), for tints.
    var tint: Color {
        switch self {
        case .solid(let c): return Color(hex: c)
        case .gradient(_, let stops): return Color(hex: stops[stops.count / 2].color)
        }
    }
}

// MARK: - Icons

enum Icons {
    static func specialty(_ key: String) -> String {
        switch key {
        case "moderator": return "bubble.left.and.bubble.right.fill"
        case "orchestrator": return "point.3.connected.trianglepath.dotted"
        case "coder": return "chevron.left.forwardslash.chevron.right"
        case "long_horizon": return "paperplane.fill"
        case "security": return "shield.lefthalf.filled"
        case "debug": return "ladybug.fill"
        case "creative": return "pencil.and.scribble"
        case "docs": return "doc.text.fill"
        case "image": return "photo.fill"
        case "teacher": return "graduationcap.fill"
        case "fun_facts": return "lightbulb.fill"
        case "verifier": return "checkmark.seal.fill"
        case "researcher": return "binoculars.fill"
        case "git": return "arrow.triangle.branch"
        case "council": return "scalemass.fill"
        case "director": return "safari.fill"
        case "engineer": return "ruler.fill"
        case "designer": return "pencil.and.ruler.fill"
        case "level_designer": return "map.fill"
        case "artist_3d": return "cube.fill"
        case "audio_designer": return "waveform"
        case "story_designer": return "theatermasks.fill"
        default: return "sparkles"
        }
    }

    static func chat(_ name: String) -> String {
        switch name {
        case "code": return "chevron.left.forwardslash.chevron.right"
        case "message-circle": return "message.fill"
        case "rocket": return "paperplane.fill"
        case "graduation-cap": return "graduationcap.fill"
        case "palette": return "paintpalette.fill"
        case "briefcase": return "briefcase.fill"
        case "book-open": return "book.fill"
        case "music": return "music.note"
        case "camera": return "camera.fill"
        case "globe": return "globe"
        case "heart": return "heart.fill"
        case "lightbulb": return "lightbulb.fill"
        case "shield": return "shield.fill"
        case "terminal": return "terminal.fill"
        case "wrench": return "wrench.fill"
        case "star": return "star.fill"
        case "zap": return "bolt.fill"
        case "leaf": return "leaf.fill"
        case "bot": return "cpu"
        default: return "number"
        }
    }

    static func role(_ name: String) -> String {
        switch name {
        case "crown": return "crown.fill"
        case "star": return "star.fill"
        case "sparkles": return "sparkles"
        case "gem": return "diamond.fill"
        case "trophy": return "trophy.fill"
        case "shield": return "shield.fill"
        case "zap": return "bolt.fill"
        case "flame": return "flame.fill"
        case "rocket": return "paperplane.fill"
        case "heart": return "heart.fill"
        case "moon": return "moon.fill"
        case "sun": return "sun.max.fill"
        default: return "star.fill"
        }
    }
}

// MARK: - Backgrounds and glass

struct PodBackground: View {
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        ZStack {
            (scheme == .dark ? Color(hex: "#0b0f1c") : Color(hex: "#eef2fb")).ignoresSafeArea()
            // Soft brand glows behind the glass, like the desktop window.
            Circle().fill(Brand.blue.opacity(scheme == .dark ? 0.28 : 0.20)).frame(width: 380).blur(radius: 90).offset(x: -130, y: -260)
            Circle().fill(Brand.violet.opacity(scheme == .dark ? 0.24 : 0.18)).frame(width: 340).blur(radius: 100).offset(x: 150, y: 120)
        }
        .ignoresSafeArea()
    }
}

extension View {
    /// Frosted card: ultra-thin material with a hairline border.
    func glass(radius: CGFloat = 16) -> some View {
        background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: radius, style: .continuous).strokeBorder(Color.primary.opacity(0.09), lineWidth: 0.7))
    }
}

// MARK: - Avatars

struct AgentAvatar: View {
    let agent: Agent
    var size: CGFloat = 36
    var busy = false
    @State private var pulse = false

    var body: some View {
        let glyph = agent.color.firstColor.luminance > 0.55 ? Color.black.opacity(0.78) : Color.white
        ZStack {
            if busy {
                Circle().fill(agent.color.gradient).opacity(pulse ? 0.7 : 0.25).scaleEffect(pulse ? 1.18 : 1.0)
                    .animation(.easeInOut(duration: 1.5).repeatForever(autoreverses: true), value: pulse)
            }
            Circle().fill(agent.color.gradient)
            Image(systemName: Icons.specialty(agent.specialty))
                .font(.system(size: size * 0.42, weight: .semibold))
                .foregroundStyle(glyph)
        }
        .frame(width: size, height: size)
        .onAppear { pulse = busy }
        .onChange(of: busy) { _, v in pulse = v }
    }
}

struct AgentNameText: View {
    let agent: Agent
    var font: Font = .system(size: 15, weight: .semibold)

    var body: some View {
        Text(agent.name).font(font).foregroundStyle(agent.color.gradient)
    }
}

struct UserAvatar: View {
    @Environment(AppModel.self) private var model
    var size: CGFloat = 36

    var body: some View {
        let role = model.roles.sorted { $0.sort < $1.sort }.first
        ZStack {
            Circle().fill(role?.color.gradient ?? Brand.gradient)
            Image(systemName: role.map { Icons.role($0.icon) } ?? "person.fill")
                .font(.system(size: size * 0.42, weight: .semibold))
                .foregroundStyle(.white)
        }
        .frame(width: size, height: size)
    }
}

struct UserNameText: View {
    @Environment(AppModel.self) private var model
    var font: Font = .system(size: 15, weight: .semibold)

    var body: some View {
        let role = model.roles.sorted { $0.sort < $1.sort }.first
        if let role {
            Text(model.displayName).font(font).foregroundStyle(role.color.gradient)
        } else {
            Text(model.displayName).font(font)
        }
    }
}

struct ChatGlyph: View {
    let chat: Chat
    var size: CGFloat = 38

    var body: some View {
        Image(systemName: Icons.chat(chat.icon))
            .font(.system(size: size * 0.42, weight: .semibold))
            .foregroundStyle(Brand.gradient)
            .frame(width: size, height: size)
            .background(Brand.blue.opacity(0.14), in: RoundedRectangle(cornerRadius: size * 0.3, style: .continuous))
    }
}

// MARK: - Formatting

enum Fmt {
    static func tokens(_ n: Int64) -> String {
        if n >= 1_000_000 { return String(format: "%.1fM", Double(n) / 1_000_000) }
        if n >= 1_000 { return String(format: "%.1fk", Double(n) / 1_000) }
        return "\(n)"
    }

    static func duration(_ ms: Int64) -> String {
        let s = Double(ms) / 1000
        if s < 1 { return "\(ms) ms" }
        if s < 60 { return String(format: "%.1f s", s) }
        let m = Int(s) / 60
        if m < 60 { return "\(m) min \(Int(s) % 60) s" }
        return "\(m / 60) h \(m % 60) min"
    }

    static func time(_ ms: Int64) -> String {
        let d = Date(timeIntervalSince1970: Double(ms) / 1000)
        if Calendar.current.isDateInToday(d) { return d.formatted(date: .omitted, time: .shortened) }
        return d.formatted(.dateTime.month(.abbreviated).day().hour().minute())
    }

    static func ago(_ ms: Int64?) -> String {
        guard let ms else { return "never" }
        let s = max(0, Int(Date().timeIntervalSince1970 - Double(ms) / 1000))
        if s < 10 { return "just now" }
        if s < 60 { return "\(s) s ago" }
        if s < 3600 { return "\(s / 60) min ago" }
        if s < 86400 { return "\(s / 3600) h ago" }
        return "\(s / 86400) d ago"
    }
}
