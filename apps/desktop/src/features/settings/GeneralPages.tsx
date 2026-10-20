import { Monitor, Moon, Sun } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ModelRef } from "@dolphin/shared";
import { ModelPicker } from "@/components/ModelPicker";
import { Input, Row, Segmented, Switch, Textarea } from "@/components/ui";
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

export function GeneralPage() {
  const [utility, setUtility] = useSetting<ModelRef | null>("utility.model");
  return (
    <>
      <PageHeader title="General" description="Housekeeping, internal models and how much context agents get." />
      <Card title="Utility model">
        <div className="py-3">
          <p className="mb-3 text-[12.5px] text-muted leading-relaxed">
            A small, cheap model for internal tasks: changing settings from chat, picking which of your preferences matter. A free OpenRouter model (ids ending in
            <span className="font-mono"> :free</span>) or a small local model works well. If none is set, Roger's model is used.
          </p>
          <ModelPicker value={utility ?? null} onChange={(m) => void setUtility(m)} purpose="utility" />
        </div>
      </Card>
      <Card title="Cleanup">
        <Row title="Clean chats periodically" description="Global switch. Each chat also has its own setting. Cleaning is done by a scheduler, no model involved.">
          <BoolSetting k="cleanup.enabled" />
        </Row>
        <Row title="Default cleanup age" description="Used for new chats. Messages older than this are removed on a rolling basis.">
          <NumberSetting k="cleanup.default_ttl_hours" min={1} max={2160} suffix="hours" />
        </Row>
      </Card>
      <Card title="Agents">
        <Row title="Context window" description="How many recent messages agents see.">
          <NumberSetting k="context.history_messages" min={4} max={200} suffix="messages" />
        </Row>
        <Row title="Stuck-agent timeout" description="If an agent produces nothing for this long, Roger checks on it.">
          <NumberSetting k="supervisor.stall_seconds" min={15} max={1800} suffix="seconds" />
        </Row>
        <Row title="Web access" description="Let agents search the web and open public pages for current information. OpenRouter models use OpenRouter's search; all others use a built-in key-less search. Search terms and page requests leave your computer.">
          <BoolSetting k="web.access" />
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
