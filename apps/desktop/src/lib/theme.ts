import { useEffect, useState } from "react";
import { useApp } from "@/store/app";

/** The effective theme ("system" resolved), kept in sync with <html data-theme>. */
export function useResolvedTheme(): "dark" | "light" {
  const pref = (useApp((s) => s.settings["appearance.theme"]) as string) || "system";
  const [system, setSystem] = useState<"dark" | "light">(() =>
    window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark",
  );
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const on = () => setSystem(mq.matches ? "light" : "dark");
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return pref === "system" ? system : (pref as "dark" | "light");
}

export function useApplyAppearance() {
  const s = useApp((st) => st.settings);
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = (s["appearance.theme"] as string) || "system";
    root.dataset.reduceMotion = String(Boolean(s["appearance.reduce_motion"]));
    root.dataset.reduceTransparency = String(Boolean(s["appearance.reduce_transparency"]));
  }, [s]);
}

export function useReducedMotion(): boolean {
  const pref = Boolean(useApp((s) => s.settings["appearance.reduce_motion"]));
  const [os, setOs] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const on = () => setOs(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return pref || os;
}
