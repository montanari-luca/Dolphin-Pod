import { describe, expect, it } from "vitest";
import { THEME_BG, contrastRatio, cssBackground, ensureContrast, onColor, worstContrast } from "./color";

describe("color", () => {
  it("computes WCAG contrast", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 0);
    expect(contrastRatio("#777777", "#777777")).toBeCloseTo(1, 5);
  });

  it("nudges low-contrast colors until readable in both themes", () => {
    const darkYellow = ensureContrast("#fde047", THEME_BG.light);
    expect(contrastRatio(darkYellow, THEME_BG.light)).toBeGreaterThanOrEqual(4.5);
    const brightNavy = ensureContrast("#1e1b4b", THEME_BG.dark);
    expect(contrastRatio(brightNavy, THEME_BG.dark)).toBeGreaterThanOrEqual(4.5);
    expect(ensureContrast("#ffffff", THEME_BG.dark)).toBe("#ffffff");
  });

  it("renders gradients and picks readable foregrounds", () => {
    const g = { type: "gradient" as const, angle: 90, stops: [{ color: "#ff0000", position: 100 }, { color: "#0000ff", position: 0 }] };
    expect(cssBackground(g)).toBe("linear-gradient(90deg, #0000ff 0%, #ff0000 100%)");
    expect(onColor({ type: "solid", color: "#fde047" })).toBe("#111827");
    expect(onColor({ type: "solid", color: "#1e3a8a" })).toBe("#ffffff");
    expect(worstContrast({ type: "solid", color: "#fde047" }, "light")).toBeLessThan(3);
  });
});
