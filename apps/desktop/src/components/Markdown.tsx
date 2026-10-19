import { Check, Copy } from "lucide-react";
import { createContext, memo, useContext, useEffect, useState, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import type { Agent } from "@dolphin/shared";
import { AgentMention } from "@/components/agent";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";

const StreamingCtx = createContext(false);

// ---------------------------------------------------------------- syntax highlighting

const cache = new Map<string, string>();

async function highlight(code: string, lang: string): Promise<string> {
  const key = `${lang}\u0000${code}`;
  const hit = cache.get(key);
  if (hit) return hit;
  const { codeToHtml, bundledLanguages } = await import("shiki");
  const language = lang in bundledLanguages ? lang : "text";
  const html = await codeToHtml(code, {
    lang: language,
    themes: { light: "github-light", dark: "github-dark-dimmed" },
    defaultColor: false,
  });
  if (cache.size > 300) cache.clear();
  cache.set(key, html);
  return html;
}

function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const streaming = useContext(StreamingCtx);
  const [html, setHtml] = useState<string | null>(() => cache.get(`${lang}\u0000${code}`) ?? null);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    let alive = true;
    // While streaming, re-highlight at most a few times per second.
    const t = setTimeout(() => highlight(code, lang).then((h) => alive && setHtml(h)).catch(() => {}), streaming ? 350 : 0);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [code, lang, streaming]);
  return (
    <div className="code-block group/code my-2 overflow-hidden rounded-md border border-line bg-code">
      <div className="flex h-8 items-center justify-between border-b border-line px-3 text-[11px] text-faint">
        <span className="font-mono uppercase tracking-wide">{lang || "text"}</span>
        <button
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-faint hover:text-fg hover:bg-hover transition-ui"
          onClick={() => {
            void navigator.clipboard.writeText(code);
            setCopied(true);
            setTimeout(() => setCopied(false), 1400);
          }}
        >
          {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      {html ? (
        <div className="selectable" dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <pre className="selectable">
          <code>{code}</code>
        </pre>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- @mentions inside markdown

type HastNode = { type: string; tagName?: string; value?: string; children?: HastNode[]; properties?: Record<string, unknown> };

function rehypeMentions(agents: Agent[]) {
  const re = /(^|[^\w@])@([\p{L}\p{N}_-]+)/gu;
  const walk = (node: HastNode) => {
    if (!node.children || node.tagName === "code" || node.tagName === "pre" || node.tagName === "a") return;
    const next: HastNode[] = [];
    for (const child of node.children) {
      if (child.type !== "text" || !child.value?.includes("@")) {
        walk(child);
        next.push(child);
        continue;
      }
      const text = child.value;
      let last = 0;
      for (const m of text.matchAll(re)) {
        const agent = agents.find((a) => a.name.toLowerCase() === m[2].toLowerCase());
        if (!agent) continue;
        const at = (m.index ?? 0) + m[1].length;
        if (at > last) next.push({ type: "text", value: text.slice(last, at) });
        next.push({ type: "element", tagName: "dp-mention", properties: { agent: agent.id }, children: [{ type: "text", value: `@${agent.name}` }] });
        last = at + m[2].length + 1;
      }
      if (last < text.length) next.push({ type: "text", value: text.slice(last) });
    }
    node.children = next;
  };
  return () => (tree: HastNode) => walk(tree);
}

function Mention({ agent: id }: { agent?: string }) {
  const agent = useApp((s) => s.agents.find((a) => a.id === id));
  return agent ? <AgentMention agent={agent} /> : null;
}

function Link({ href, children }: { href?: string; children?: ReactNode }) {
  const api = useApp((s) => s.api);
  return (
    <a
      href={href}
      onClick={(e) => {
        e.preventDefault();
        if (href && /^https?:\/\//.test(href)) void api?.openUrl(href);
      }}
    >
      {children}
    </a>
  );
}

const components = {
  code({ className, children }: { className?: string; children?: ReactNode }) {
    const text = String(children ?? "");
    const lang = /language-([\w+#-]+)/.exec(className ?? "")?.[1];
    if (!lang && !text.includes("\n")) return <code>{children}</code>;
    return <CodeBlock code={text.replace(/\n$/, "")} lang={(lang ?? "text").toLowerCase()} />;
  },
  pre({ children }: { children?: ReactNode }) {
    return <>{children}</>;
  },
  a: Link,
  "dp-mention": Mention,
} as unknown as Components;

export const Markdown = memo(function Markdown({ text, streaming = false, className }: { text: string; streaming?: boolean; className?: string }) {
  const agents = useApp((s) => s.agents);
  return (
    <StreamingCtx.Provider value={streaming}>
      <div className={cn("md", streaming && "caret-host", className)}>
        <ReactMarkdown remarkPlugins={[remarkGfm, remarkMath]} rehypePlugins={[rehypeKatex, rehypeMentions(agents)]} components={components}>
          {text}
        </ReactMarkdown>
      </div>
    </StreamingCtx.Provider>
  );
});
