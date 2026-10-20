import { useEffect } from "react";
import { isTauri } from "@/ipc/api";

const INTERACTIVE = "button, a, input, textarea, select, label, summary, [role='button'], [role='menuitem'], [contenteditable='true'], .no-drag";

/**
 * WKWebView ignores CSS `app-region`, so `.drag-region` elements move the window through this
 * handler instead: press-and-drag starts a native drag, double-click toggles maximize.
 */
export function useWindowDrag() {
  useEffect(() => {
    if (!isTauri) return;
    const onDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement | null;
      if (!target?.closest(".drag-region") || target.closest(INTERACTIVE)) return;
      void import("@tauri-apps/api/window").then(({ getCurrentWindow }) => {
        const win = getCurrentWindow();
        void (e.detail === 2 ? win.toggleMaximize() : win.startDragging());
      });
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, []);
}
