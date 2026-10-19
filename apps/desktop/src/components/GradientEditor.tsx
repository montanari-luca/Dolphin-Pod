import { AlertTriangle, Minus, Plus } from "lucide-react";
import type { AgentColor } from "@dolphin/shared";
import { Segmented } from "@/components/ui";
import { cn } from "@/lib/cn";
import { cssBackground, primary, worstContrast } from "@/lib/color";

const PRESETS: AgentColor[] = [
  g("#2563eb", "#22d3ee"),
  g("#7c3aed", "#f59e0b"),
  g("#dc2626", "#7f1d1d"),
  g("#f97316", "#facc15"),
  g("#ec4899", "#f472b6"),
  g("#10b981", "#14b8a6"),
  g("#3d8bfd", "#a77bdb"),
  g("#f7826f", "#fbb36a"),
  g("#0ea5e9", "#6366f1", "#d946ef"),
  { type: "gradient", angle: 90, stops: ["#ff3b5c", "#ff9f1c", "#ffd60a", "#2ee59d", "#3d8bfd", "#b15cff"].map((c, i) => ({ color: c, position: [0, 20, 38, 56, 76, 100][i] })) },
  { type: "solid", color: "#64748b" },
];

function g(...colors: string[]): AgentColor {
  return { type: "gradient", angle: 135, stops: colors.map((c, i) => ({ color: c, position: Math.round((i / (colors.length - 1)) * 100) })) };
}

export function GradientEditor({ value, onChange }: { value: AgentColor; onChange: (c: AgentColor) => void }) {
  const isGradient = value.type === "gradient";
  const stops = value.type === "gradient" ? value.stops : [];
  const worstDark = worstContrast(value, "dark");
  const worstLight = worstContrast(value, "light");

  const setStop = (i: number, patch: Partial<{ color: string; position: number }>) => {
    if (value.type !== "gradient") return;
    onChange({ ...value, stops: value.stops.map((s, j) => (j === i ? { ...s, ...patch } : s)) });
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <div className="h-12 flex-1 rounded-md border border-line shadow-inner" style={{ background: cssBackground(value) }} />
        <Segmented
          value={value.type}
          options={[
            { value: "solid", label: "Solid" },
            { value: "gradient", label: "Gradient" },
          ]}
          onChange={(t) =>
            onChange(t === "solid" ? { type: "solid", color: primary(value) } : { type: "gradient", angle: 135, stops: [{ color: primary(value), position: 0 }, { color: "#a77bdb", position: 100 }] })
          }
        />
      </div>

      {value.type === "solid" && (
        <label className="flex items-center gap-3 text-[12.5px] text-muted">
          <input type="color" value={value.color} onChange={(e) => onChange({ type: "solid", color: e.target.value })} className="h-8 w-12 cursor-pointer rounded border border-line bg-transparent" />
          <span className="font-mono">{value.color}</span>
        </label>
      )}

      {isGradient && value.type === "gradient" && (
        <div className="space-y-2">
          {stops.map((s, i) => (
            <div key={i} className="flex items-center gap-3">
              <input type="color" value={s.color} onChange={(e) => setStop(i, { color: e.target.value })} className="h-7 w-10 cursor-pointer rounded border border-line bg-transparent" aria-label={`Stop ${i + 1} color`} />
              <input type="range" min={0} max={100} value={s.position} onChange={(e) => setStop(i, { position: Number(e.target.value) })} className="flex-1 accent-[var(--accent)]" aria-label={`Stop ${i + 1} position`} />
              <span className="w-9 text-right font-mono text-[11px] text-faint">{Math.round(s.position)}%</span>
              <button
                className="size-6 inline-flex items-center justify-center rounded text-faint hover:bg-hover hover:text-fg disabled:opacity-30"
                disabled={stops.length <= 2}
                onClick={() => onChange({ ...value, stops: stops.filter((_, j) => j !== i) })}
                aria-label="Remove stop"
              >
                <Minus className="size-3.5" />
              </button>
            </div>
          ))}
          <div className="flex items-center gap-3">
            <button
              className="inline-flex items-center gap-1 text-[12px] text-muted hover:text-fg disabled:opacity-40"
              disabled={stops.length >= 8}
              onClick={() => onChange({ ...value, stops: [...stops, { color: "#f7826f", position: 100 }] })}
            >
              <Plus className="size-3.5" /> Add stop
            </button>
            <div className="ml-auto flex items-center gap-2 text-[12px] text-muted">
              Angle
              <input type="range" min={0} max={360} value={value.angle} onChange={(e) => onChange({ ...value, angle: Number(e.target.value) })} className="w-28 accent-[var(--accent)]" aria-label="Angle" />
              <span className="w-9 font-mono text-[11px] text-faint">{Math.round(value.angle)}°</span>
            </div>
          </div>
        </div>
      )}

      <div className="flex flex-wrap gap-1.5">
        {PRESETS.map((p, i) => (
          <button
            key={i}
            onClick={() => onChange(p)}
            className={cn("size-6 rounded-full border border-line transition-transform hover:scale-110")}
            style={{ background: cssBackground(p) }}
            aria-label="Preset"
          />
        ))}
      </div>

      {(worstDark < 3 || worstLight < 3) && (
        <div className="flex items-start gap-2 rounded-sm bg-warning/10 px-2.5 py-2 text-[11.5px] text-warning">
          <AlertTriangle className="size-3.5 mt-px shrink-0" />
          <span>
            Low contrast in {worstDark < 3 && worstLight < 3 ? "both themes" : worstDark < 3 ? "dark mode" : "light mode"}. Names are brightened or darkened automatically so they stay readable.
          </span>
        </div>
      )}
    </div>
  );
}
