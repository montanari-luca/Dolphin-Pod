import type { AgentColor } from "@dolphin/shared";

export type Rgb = [number, number, number];

export const THEME_BG = { dark: "#161c33", light: "#f3f6fd" } as const;

export function hexToRgb(hex: string): Rgb {
  let h = hex.trim().replace("#", "");
  if (h.length === 3) h = h.split("").map((c) => c + c).join("");
  const n = parseInt(h.slice(0, 6), 16);
  if (Number.isNaN(n)) return [128, 128, 128];
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function rgbToHex([r, g, b]: Rgb): string {
  return "#" + [r, g, b].map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0")).join("");
}

function channel(v: number) {
  const s = v / 255;
  return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

export function luminance(hex: string): number {
  const [r, g, b] = hexToRgb(hex);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** WCAG contrast ratio, 1..21. */
export function contrastRatio(a: string, b: string): number {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (l1 + 0.05) / (l2 + 0.05);
}

export function mix(a: string, b: string, t: number): string {
  const [x, y] = [hexToRgb(a), hexToRgb(b)];
  return rgbToHex([x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t, x[2] + (y[2] - x[2]) * t]);
}

/** Nudge a color toward white (dark theme) or black (light theme) until it reaches `min` contrast. */
export function ensureContrast(hex: string, bg: string, min = 4.5): string {
  if (contrastRatio(hex, bg) >= min) return hex;
  const target = luminance(bg) < 0.4 ? "#ffffff" : "#000000";
  for (let t = 0.05; t <= 1; t += 0.05) {
    const c = mix(hex, target, t);
    if (contrastRatio(c, bg) >= min) return c;
  }
  return target;
}

export function stops(c: AgentColor): string[] {
  return c.type === "solid" ? [c.color] : c.stops.map((s) => s.color);
}

export function primary(c: AgentColor): string {
  return stops(c)[0] ?? "#7c8cff";
}

export function average(c: AgentColor): string {
  const s = stops(c).map(hexToRgb);
  const n = s.length || 1;
  return rgbToHex([0, 1, 2].map((i) => s.reduce((acc, v) => acc + v[i], 0) / n) as Rgb);
}

export function cssBackground(c: AgentColor): string {
  if (c.type === "solid") return c.color;
  const parts = [...c.stops].sort((a, b) => a.position - b.position).map((s) => `${s.color} ${s.position}%`);
  return `linear-gradient(${c.angle}deg, ${parts.join(", ")})`;
}

/** Background for names: the agent's colors, each adjusted for contrast against the theme. */
export function readableTextBackground(c: AgentColor, theme: "dark" | "light"): string {
  const bg = THEME_BG[theme];
  if (c.type === "solid") return ensureContrast(c.color, bg);
  const parts = [...c.stops].sort((a, b) => a.position - b.position).map((s) => `${ensureContrast(s.color, bg)} ${s.position}%`);
  return `linear-gradient(${c.angle}deg, ${parts.join(", ")})`;
}

/** Foreground for text drawn on top of the agent color (avatar initials). */
export function onColor(c: AgentColor): string {
  return luminance(average(c)) > 0.45 ? "#111827" : "#ffffff";
}

/** Lowest contrast of any stop against a theme background (for the editor's warning). */
export function worstContrast(c: AgentColor, theme: "dark" | "light"): number {
  return Math.min(...stops(c).map((s) => contrastRatio(s, THEME_BG[theme])));
}
