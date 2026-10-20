import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

/// A small Markdown renderer: fenced code, headings, lists, quotes and paragraphs, with inline formatting
/// (bold, italic, code, links) handled by AttributedString.
struct MarkdownView: View {
    let text: String
    var font: Font = .system(size: 16)

    private enum Block: Identifiable {
        case paragraph(String)
        case heading(Int, String)
        case bullet(String, Int)
        case numbered(String, String, Int)
        case quote(String)
        case code(String, String)
        case rule

        var id: String {
            switch self {
            case .paragraph(let s): return "p" + s
            case .heading(let l, let s): return "h\(l)" + s
            case .bullet(let s, let d): return "b\(d)" + s
            case .numbered(let n, let s, _): return "n" + n + s
            case .quote(let s): return "q" + s
            case .code(let l, let s): return "c" + l + s
            case .rule: return "rule"
            }
        }
    }

    private var blocks: [(Int, Block)] {
        Array(Self.parse(text).enumerated()).map { ($0.offset, $0.element) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(blocks, id: \.0) { _, block in
                view(for: block)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private func view(for block: Block) -> some View {
        switch block {
        case .paragraph(let s):
            Text(Self.inline(s)).font(font).fixedSize(horizontal: false, vertical: true)
        case .heading(let level, let s):
            Text(Self.inline(s)).font(.system(size: level == 1 ? 22 : level == 2 ? 19 : 17, weight: .semibold)).padding(.top, 4)
        case .bullet(let s, let depth):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("•").font(font).foregroundStyle(.secondary)
                Text(Self.inline(s)).font(font).fixedSize(horizontal: false, vertical: true)
            }.padding(.leading, CGFloat(depth) * 14)
        case .numbered(let n, let s, let depth):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(n + ".").font(font).foregroundStyle(.secondary).monospacedDigit()
                Text(Self.inline(s)).font(font).fixedSize(horizontal: false, vertical: true)
            }.padding(.leading, CGFloat(depth) * 14)
        case .quote(let s):
            HStack(spacing: 10) {
                RoundedRectangle(cornerRadius: 2).fill(Brand.gradient).frame(width: 3)
                Text(Self.inline(s)).font(font).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        case .code(let lang, let code):
            CodeBlock(language: lang, code: code)
        case .rule:
            Divider()
        }
    }

    static func inline(_ s: String) -> AttributedString {
        var opts = AttributedString.MarkdownParsingOptions()
        opts.interpretedSyntax = .inlineOnlyPreservingWhitespace
        return (try? AttributedString(markdown: s, options: opts)) ?? AttributedString(s)
    }

    private static func parse(_ text: String) -> [Block] {
        var blocks: [Block] = []
        var paragraph: [String] = []
        var inCode = false
        var codeLang = ""
        var code: [String] = []

        func flush() {
            if !paragraph.isEmpty { blocks.append(.paragraph(paragraph.joined(separator: "\n"))); paragraph = [] }
        }

        for raw in text.components(separatedBy: "\n") {
            let line = raw
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("```") {
                if inCode {
                    blocks.append(.code(codeLang, code.joined(separator: "\n")))
                    inCode = false; code = []; codeLang = ""
                } else {
                    flush(); inCode = true
                    codeLang = String(trimmed.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                }
                continue
            }
            if inCode { code.append(line); continue }
            if trimmed.isEmpty { flush(); continue }
            let indent = line.prefix { $0 == " " }.count / 2
            if let m = trimmed.firstMatch(of: /^(#{1,6})\s+(.*)$/) {
                flush(); blocks.append(.heading(m.1.count, String(m.2)))
            } else if let m = trimmed.firstMatch(of: /^[-*+]\s+(.*)$/) {
                flush(); blocks.append(.bullet(String(m.1), indent))
            } else if let m = trimmed.firstMatch(of: /^(\d+)[.)]\s+(.*)$/) {
                flush(); blocks.append(.numbered(String(m.1), String(m.2), indent))
            } else if trimmed.hasPrefix(">") {
                flush(); blocks.append(.quote(String(trimmed.dropFirst()).trimmingCharacters(in: .whitespaces)))
            } else if trimmed == "---" || trimmed == "***" {
                flush(); blocks.append(.rule)
            } else {
                paragraph.append(line)
            }
        }
        if inCode { blocks.append(.code(codeLang, code.joined(separator: "\n"))) }
        flush()
        return blocks
    }
}

struct CodeBlock: View {
    let language: String
    let code: String
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(language.isEmpty ? "code" : language).font(.system(size: 11, weight: .medium)).foregroundStyle(.secondary)
                Spacer()
                Button {
                    #if canImport(UIKit)
                    UIPasteboard.general.string = code
                    #endif
                    copied = true
                    Task { try? await Task.sleep(for: .seconds(1.5)); copied = false }
                } label: {
                    Label(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc").font(.system(size: 11, weight: .medium))
                }
                .buttonStyle(.plain).foregroundStyle(.secondary)
            }
            .padding(.horizontal, 12).padding(.vertical, 7)
            Divider().opacity(0.5)
            ScrollView(.horizontal, showsIndicators: false) {
                Text(code).font(.system(size: 13, design: .monospaced)).textSelection(.enabled)
                    .padding(12).fixedSize(horizontal: true, vertical: true)
            }
        }
        .background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Color.primary.opacity(0.08), lineWidth: 0.7))
    }
}
