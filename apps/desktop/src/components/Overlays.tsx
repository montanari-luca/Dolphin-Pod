import { AnimatePresence, motion } from "motion/react";
import { AlertTriangle, CheckCircle2, Download, Info, X, XCircle } from "lucide-react";
import { Button, Dialog } from "@/components/ui";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";

export function Background() {
  return (
    <div aria-hidden className="pointer-events-none fixed inset-0 -z-10 overflow-hidden">
      <div className="absolute inset-0" style={{ background: "var(--bg-base)", opacity: "var(--bg-opacity, 1)" }} />
      <div className="absolute -left-[12%] top-[30%] h-[70vh] w-[55vw] rounded-full blur-[120px]" style={{ background: "var(--bg-glow-1)" }} />
      <div className="absolute right-[-10%] -top-[18%] h-[65vh] w-[50vw] rounded-full blur-[120px]" style={{ background: "var(--bg-glow-3)" }} />
      <div className="absolute left-[35%] top-[45%] h-[50vh] w-[40vw] rounded-full blur-[140px]" style={{ background: "var(--bg-glow-2)" }} />
    </div>
  );
}

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  const icon = { info: Info, success: CheckCircle2, warning: AlertTriangle, error: XCircle };
  const tone = { info: "text-accent", success: "text-success", warning: "text-warning", error: "text-danger" };
  return (
    <div className="pointer-events-none fixed bottom-5 right-5 z-[60] flex w-[360px] flex-col gap-2">
      <AnimatePresence>
        {toasts.map((t) => {
          const Icon = icon[t.level];
          return (
            <motion.div
              key={t.id}
              layout
              initial={{ opacity: 0, y: 12, scale: 0.97 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, x: 24 }}
              transition={{ type: "spring", stiffness: 460, damping: 34 }}
              className="glass-float pointer-events-auto flex items-start gap-2.5 rounded-lg px-3.5 py-3"
            >
              <Icon className={cn("size-4 shrink-0 mt-px", tone[t.level])} />
              <div className="flex-1 text-[12.5px] leading-snug text-fg selectable">{t.text}</div>
              <button onClick={() => dismiss(t.id)} className="text-faint hover:text-fg" aria-label="Dismiss">
                <X className="size-3.5" />
              </button>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}

/** Confirmation for sensitive changes requested through Roger. */
export function ConfirmRequests() {
  const confirms = useApp((s) => s.confirms);
  const api = useApp((s) => s.api);
  const current = confirms[0];
  const resolve = (accepted: boolean) => {
    if (!current) return;
    void api?.resolveConfirmation(current.id, accepted);
    useApp.setState((s) => ({ confirms: s.confirms.slice(1) }));
  };
  return (
    <Dialog
      open={!!current}
      onOpenChange={(o) => !o && resolve(false)}
      title={current?.title ?? ""}
      description={current?.body}
      width={440}
      footer={
        <>
          <Button variant="ghost" onClick={() => resolve(false)}>Cancel</Button>
          <Button variant="primary" onClick={() => resolve(true)}>Confirm</Button>
        </>
      }
    />
  );
}

export function UpdateBanner() {
  const update = useApp((s) => s.update);
  const api = useApp((s) => s.api);
  return (
    <AnimatePresence>
      {update && (
        <motion.div
          initial={{ opacity: 0, y: -12 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -12 }}
          className="glass-float fixed left-1/2 top-3 z-[55] flex -translate-x-1/2 items-center gap-3 rounded-full py-1.5 pl-4 pr-1.5 text-[12.5px]"
        >
          <span className="text-fg">
            Dolphin Pod <span className="font-semibold">{update.version}</span> is available.
          </span>
          <Button size="sm" variant="primary" onClick={() => api?.openUrl(update.url)}>
            <Download className="size-3.5" /> Download
          </Button>
          <button className="size-7 inline-flex items-center justify-center rounded-full text-faint hover:bg-hover hover:text-fg" onClick={() => useApp.setState({ update: null })} aria-label="Dismiss">
            <X className="size-3.5" />
          </button>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
