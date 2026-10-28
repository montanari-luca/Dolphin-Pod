import { Monitor, Moon, Sun } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ModelRef } from "@dolphin/shared";
import { ModelPicker } from "@/components/ModelPicker";
import { Button, Input, Row, Segmented, Switch, Textarea } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { useApp } from "@/store/app";
import { Card, PageHeader } from "./SettingsView";

function useSetting<T>(key: string): [T, (v: T) => Promise<boolean>] {
  const value = useApp((s) => s.settings[key]) as T;
  const setSetting = useApp((s) => s.setSetting);
  return [value, (v: T) => setSetting(key, v)];
}

function NumberSetting({ k, min, max, suffix, width = 80 }: { k: string; min: number; max: number; suffix?: string; width?: number }) {
  const [value, set] = useSetting<number>(k);
  const [draft, setDraft] = useState(String(value ?? ""));
  useEffect(() => setDraft(String(value ?? "")), [value]);
  return (
    <div className="flex items-center gap-2">
      <Input
        type="number"
        min={min}
        max={max}
        value={draft}
        style={{ width }}
        className="text-right"
        onChange={(e) => setDraft(e.target.value)}
        onBlur={async () => {
          const n = Number(draft);
          if (!Number.isFinite(n) || !(await set(Math.round(n)))) setDraft(String(value));
        }}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
      {suffix && <span className="text-[12px] text-faint">{suffix}</span>}
    </div>
  );
}

function BoolSetting({ k }: { k: string }) {
  const [value, set] = useSetting<boolean>(k);
  return <Switch checked={Boolean(value)} onChange={(v) => void set(v)} />;
}

/** Optional: rank an agent's many notes by meaning instead of shared words. A Test button says whether the model works. */
function EmbeddingModelRow() {
  const [model, setModel] = useSetting<ModelRef | null>("memory.embedding_model");
  const api = useApp((s) => s.api);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [testing, setTesting] = useState(false);
  useEffect(() => setResult(null), [model?.providerId, model?.modelId]);
  const test = async () => {
    if (!api) return;
    setTesting(true);
    try {
      const size = await api.testEmbeddingModel();
      setResult({ ok: true, text: `Works: vectors of ${size} numbers.` });
    } catch (e) {
      setResult({ ok: false, text: errorText(e) });
    } finally {
      setTesting(false);
    }
  };
  return (
    <div className="space-y-3 py-3">
      <p className="text-[12.5px] leading-relaxed text-muted">
        Optional. When an agent has many notes about you, Dolphin Pod normally sends the ones that share words with your request. An
        embedding model can pick the ones that <em>mean</em> something related instead (a note about PostgreSQL for a question about a
        slow sql query). It needs a model that offers embeddings on Ollama, LM Studio, OpenRouter or another OpenAI-compatible server, for
        example <span className="font-mono">nomic-embed-text</span> on Ollama. If it fails, words are used. Leave it empty to turn it off.
      </p>
      <ModelPicker value={model ?? null} onChange={(m) => void setModel(m)} purpose="utility" />
      {model && (
        <div className="flex flex-wrap items-center gap-3">
          <Button size="sm" loading={testing} onClick={() => void test()}>Test</Button>
          {result && <span className={result.ok ? "text-[12.5px] text-success" : "text-[12.5px] text-danger"}>{result.text}</span>}
        </div>
      )}
    </div>
  );
}

export function GeneralPage() {
  const [utility, setUtility] = useSetting<ModelRef | null>("utility.model");
  return (
    <>
      <PageHeader title="General" description="Housekeeping, internal models and how context is managed." />
      <Card title="Utility model">
        <div className="py-3">
          <p className="mb-3 text-[12.5px] text-muted leading-relaxed">
            A small, cheap model for internal tasks: changing settings from chat, picking which of your preferences matter. A free OpenRouter model (ids ending in
            <span className="font-mono"> :free</span>) or a small local model works well. If none is set, Roger's model is used.
          </p>
          <ModelPicker value={utility ?? null} onChange={(m) => void setUtility(m)} purpose="utility" />
        </div>
      </Card>
      <Card title="Memory">
        <EmbeddingModelRow />
      </Card>
      <Card title="Cleanup">
        <Row title="Clean chats periodically" description="Global switch. Each chat also has its own setting. Cleaning is done by a scheduler, no model involved.">
          <BoolSetting k="cleanup.enabled" />
        </Row>
        <Row title="Default cleanup age" description="Used for new chats. Messages older than this are removed on a rolling basis.">
          <NumberSetting k="cleanup.default_ttl_hours" min={1} max={2160} suffix="hours" />
        </Row>
      </Card>
      <Card title="Context">
        <Row title="Compact long conversations" description="When a conversation closes in on a model's context window, older messages are summarized instead of dropped, so the model keeps the thread without hitting its limit.">
          <BoolSetting k="context.auto_compact" />
        </Row>
        <Row title="Compact at" description="How full the usable context window may get before compaction starts.">
          <NumberSetting k="context.compact_at" min={30} max={98} suffix="%" />
        </Row>
        <Row title="Recent messages kept verbatim" description="Agents see this many recent messages word for word; older ones are summarized once a chat grows past it.">
          <NumberSetting k="context.history_messages" min={4} max={200} suffix="messages" />
        </Row>
        <Row title="Compress tool output" description="Shrink large tool results (logs, files, search hits, JSON) before the model sees them. Errors and the important parts are kept, and the model can fetch the full text on request.">
          <BoolSetting k="context.compress_tools" />
        </Row>
        <Row title="Local model context length" description="Context length requested from Ollama models (a model's own maximum is used if smaller). Higher remembers more but uses more memory.">
          <NumberSetting k="context.local_window" min={2048} max={1048576} suffix="tokens" width={96} />
        </Row>
      </Card>
      <Card title="Agents">
        <Row title="Stuck-agent timeout" description="If an agent produces nothing for this long, Roger checks on it.">
          <NumberSetting k="supervisor.stall_seconds" min={15} max={1800} suffix="seconds" />
        </Row>
        <Row title="Web access" description="Let agents search the web and open public pages for current information. OpenRouter models use OpenRouter's search; all others use a built-in key-less search. Search terms and page requests leave your computer.">
          <BoolSetting k="web.access" />
        </Row>
        <Row title="Ask before acting after reading the web" description="Once an agent has read a web page or search result in a run, every file change, command and git write in that run waits for your approval, even with full permissions, and automations refuse them. A page can hide instructions aimed at the agent; this keeps it from acting on them unseen.">
          <BoolSetting k="safety.taint_guard" />
        </Row>
        <Row title="DP Council reviews big tasks" description="Before a coding, agentic, documentation or research agent starts a big task (or after its draft, for plans), DP Council criticizes the approach once on the strongest model. Pick its model in Agents. Turn this off to save time and tokens.">
          <BoolSetting k="council.enabled" />
        </Row>
        <Row title="Learn from finished conversations" description="When a chat has been quiet for ten minutes, the agents that took part look back over it for lasting preferences you showed without ever correcting them. This sends a short transcript of the conversation to your utility model, so it is off by default. Only conversations after you turn it on are looked at.">
          <BoolSetting k="learning.reflect" />
        </Row>
        <Row title="Show orchestration details" description="Show Lincoln's brief and the routing decision on messages.">
          <BoolSetting k="routing.show_briefs" />
        </Row>
      </Card>
      <Card title="Input">
        <Row title="Long paste threshold" description="Pasted text longer than this becomes a .txt attachment.">
          <NumberSetting k="chat.paste_threshold" min={500} max={200000} suffix="chars" width={96} />
        </Row>
      </Card>
      <Card title="Updates">
        <Row title="Check for updates automatically" description="On launch and every six hours.">
          <BoolSetting k="updates.auto_check" />
        </Row>
      </Card>
    </>
  );
}

export function PreferencesPage() {
  const [value, set] = useSetting<string>("preferences.custom_instructions");
  const [draft, setDraft] = useState(value ?? "");
  const [saved, setSaved] = useState(true);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => setDraft(value ?? ""), [value]);
  const save = (v: string) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(async () => {
      if (await set(v)) setSaved(true);
    }, 600);
  };
  const chunks = draft.split(/\n\s*\n/).flatMap((b) => (/^\s*[-*•\d]/m.test(b) && b.split("\n").length > 1 ? b.split("\n") : [b])).filter((s) => s.trim()).length;
  return (
    <>
      <PageHeader
        title="Preferences"
        description="Tell the pod how you like to work. Not everything is sent every time: your instructions are split into pieces and only the pieces relevant to each request are included, which keeps requests fast and cheap."
      />
      <Textarea
        value={draft}
        rows={14}
        maxLength={20000}
        placeholder={"Examples:\n- I prefer quick implementations over long explanations when coding.\n- Always answer in British English.\n- For Python, use type hints and pytest.\n- When writing stories, keep them under 500 words."}
        onChange={(e) => {
          setDraft(e.target.value);
          setSaved(false);
          save(e.target.value);
        }}
        className="font-[inherit] text-[13px]"
      />
      <div className="mt-2 flex justify-between text-[11.5px] text-faint">
        <span>
          {chunks} piece{chunks === 1 ? "" : "s"} · write one preference per line or paragraph. Lines with “always” or “never” are included in every request.
        </span>
        <span>{saved ? "Saved" : "Saving…"}</span>
      </div>
    </>
  );
}

export function AppearancePage() {
  const [theme, setTheme] = useSetting<string>("appearance.theme");
  return (
    <>
      <PageHeader title="Appearance" description="Frosted glass, gradients, and calm motion. Pick what suits your eyes." />
      <Card>
        <Row title="Theme">
          <Segmented
            value={(theme as "system" | "dark" | "light") ?? "system"}
            onChange={(v) => void setTheme(v)}
            options={[
              { value: "system", label: <span className="inline-flex items-center gap-1.5"><Monitor className="size-3.5" />System</span> },
              { value: "dark", label: <span className="inline-flex items-center gap-1.5"><Moon className="size-3.5" />Dark</span> },
              { value: "light", label: <span className="inline-flex items-center gap-1.5"><Sun className="size-3.5" />Light</span> },
            ]}
          />
        </Row>
        <Row title="Reduce motion" description="Turn off non-essential animations. The system setting is respected too.">
          <BoolSetting k="appearance.reduce_motion" />
        </Row>
        <Row title="Reduce transparency" description="Use solid surfaces instead of frosted glass.">
          <BoolSetting k="appearance.reduce_transparency" />
        </Row>
      </Card>
    </>
  );
}
