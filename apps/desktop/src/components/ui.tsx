import * as RDialog from "@radix-ui/react-dialog";
import * as RMenu from "@radix-ui/react-dropdown-menu";
import { AnimatePresence, motion } from "motion/react";
import { Loader2, X } from "lucide-react";
import { forwardRef, useState, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes, type TextareaHTMLAttributes } from "react";
import { cn } from "@/lib/cn";

// ---------------------------------------------------------------- buttons

type Variant = "primary" | "secondary" | "ghost" | "danger";

const variants: Record<Variant, string> = {
  primary: "text-white shadow-[0_6px_20px_-8px_rgba(80,120,255,0.65)] [background:var(--brand-gradient)] hover:brightness-110 active:brightness-95",
  secondary: "bg-surface-2 border border-line hover:bg-active text-fg",
  ghost: "hover:bg-hover text-muted hover:text-fg",
  danger: "bg-danger/12 text-danger border border-danger/25 hover:bg-danger/20",
};

export const Button = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md"; loading?: boolean }>(
  function Button({ variant = "secondary", size = "md", loading, className, children, disabled, ...rest }, ref) {
    return (
      <button
        ref={ref}
        disabled={disabled || loading}
        className={cn(
          "no-drag inline-flex items-center justify-center gap-1.5 rounded-sm font-medium transition-ui select-none disabled:opacity-50 disabled:pointer-events-none whitespace-nowrap",
          size === "sm" ? "h-7 px-2.5 text-[12.5px]" : "h-8.5 px-3.5 text-[13px]",
          variants[variant],
          className,
        )}
        {...rest}
      >
        {loading && <Loader2 className="size-3.5 animate-spin" />}
        {children}
      </button>
    );
  },
);

export const IconButton = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean; size?: "sm" | "md" }>(
  function IconButton({ label, active, size = "md", className, children, ...rest }, ref) {
    return (
      <button
        ref={ref}
        aria-label={label}
        title={label}
        className={cn(
          "no-drag inline-flex items-center justify-center rounded-sm text-muted transition-ui hover:bg-hover hover:text-fg disabled:opacity-40",
          size === "sm" ? "size-7" : "size-8",
          active && "bg-active text-fg",
          className,
        )}
        {...rest}
      >
        {children}
      </button>
    );
  },
);

// ---------------------------------------------------------------- inputs

const field = "w-full rounded-sm bg-input border border-line px-3 text-[13px] text-fg placeholder:text-faint outline-none transition-ui focus:border-[var(--focus-ring)] focus:ring-3 focus:ring-[var(--accent-soft)]";

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function Input({ className, ...rest }, ref) {
  return <input ref={ref} className={cn(field, "h-8.5", className)} {...rest} />;
});

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(function Textarea({ className, ...rest }, ref) {
  return <textarea ref={ref} className={cn(field, "py-2 leading-relaxed resize-y", className)} {...rest} />;
});

export function Select({ className, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select className={cn(field, "h-8.5 pr-8 appearance-none bg-[url('data:image/svg+xml;utf8,<svg xmlns=%22http://www.w3.org/2000/svg%22 width=%2212%22 height=%2212%22 viewBox=%220 0 24 24%22 fill=%22none%22 stroke=%22%23888%22 stroke-width=%222%22><path d=%22m6 9 6 6 6-6%22/></svg>')] bg-no-repeat bg-[right_10px_center]", className)} {...rest}>
      {children}
    </select>
  );
}

export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label?: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "no-drag relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border transition-ui disabled:opacity-50",
        checked ? "border-transparent [background:var(--brand-gradient)]" : "bg-active border-line",
      )}
    >
      <span className={cn("inline-block size-4 rounded-full bg-white shadow transition-transform duration-200", checked ? "translate-x-[17px]" : "translate-x-[1px]")} />
    </button>
  );
}

export function Field({ label, hint, children, className }: { label: ReactNode; hint?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <label className={cn("block space-y-1.5", className)}>
      <span className="block text-[12px] font-medium text-muted">{label}</span>
      {children}
      {hint && <span className="block text-[11.5px] text-faint leading-snug">{hint}</span>}
    </label>
  );
}

export function Row({ title, description, children }: { title: ReactNode; description?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 py-3">
      <div className="min-w-0">
        <div className="text-[13px] font-medium text-fg">{title}</div>
        {description && <div className="text-[12px] text-faint leading-snug mt-0.5">{description}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function Segmented<T extends string>({ value, options, onChange }: { value: T; options: { value: T; label: ReactNode }[]; onChange: (v: T) => void }) {
  return (
    <div className="inline-flex rounded-sm bg-input border border-line p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          onClick={() => onChange(o.value)}
          className={cn("no-drag h-7 px-3 rounded-[7px] text-[12.5px] font-medium transition-ui", value === o.value ? "bg-surface-2 text-fg shadow-sm border border-line" : "text-muted hover:text-fg")}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Badge({ children, tone = "neutral", className }: { children: ReactNode; tone?: "neutral" | "accent" | "success" | "warning" | "danger"; className?: string }) {
  const tones = {
    neutral: "bg-hover text-muted border-line",
    accent: "bg-accent-soft text-accent border-transparent",
    success: "bg-success/12 text-success border-transparent",
    warning: "bg-warning/14 text-warning border-transparent",
    danger: "bg-danger/12 text-danger border-transparent",
  };
  return <span className={cn("inline-flex items-center gap-1 rounded-full border px-2 h-5 text-[11px] font-medium whitespace-nowrap", tones[tone], className)}>{children}</span>;
}

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cn("size-4 animate-spin text-faint", className)} />;
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="rounded-[5px] border border-line bg-hover px-1.5 py-px font-mono text-[10.5px] text-muted">{children}</kbd>;
}

// ---------------------------------------------------------------- dialog

/** Modal confirmation for destructive actions. `onConfirm` may be async; the dialog closes once it settles. */
export function ConfirmDialog({ open, onOpenChange, title, description, confirmLabel, onConfirm }: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  title: ReactNode;
  description: ReactNode;
  confirmLabel: string;
  onConfirm: () => void | Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !busy && onOpenChange(o)}
      title={title}
      description={description}
      width={420}
      footer={
        <>
          <Button variant="ghost" autoFocus onClick={() => onOpenChange(false)} disabled={busy}>Cancel</Button>
          <Button
            variant="danger"
            loading={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await onConfirm();
              } finally {
                setBusy(false);
                onOpenChange(false);
              }
            }}
          >
            {confirmLabel}
          </Button>
        </>
      }
    />
  );
}

export function Dialog({ open, onOpenChange, title, description, children, footer, width = 520 }: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  return (
    <RDialog.Root open={open} onOpenChange={onOpenChange}>
      <AnimatePresence>
        {open && (
          <RDialog.Portal forceMount>
            <RDialog.Overlay asChild forceMount>
              <motion.div className="fixed inset-0 z-50 bg-black/35" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.16 }} />
            </RDialog.Overlay>
            <RDialog.Content asChild forceMount aria-describedby={description ? undefined : undefined}>
              <motion.div
                className="glass-float fixed left-1/2 top-1/2 z-50 max-h-[86vh] w-[calc(100vw-48px)] overflow-hidden rounded-xl flex flex-col"
                style={{ maxWidth: width, x: "-50%", y: "-50%" }}
                initial={{ opacity: 0, scale: 0.96, y: "-46%" }}
                animate={{ opacity: 1, scale: 1, y: "-50%" }}
                exit={{ opacity: 0, scale: 0.97, y: "-48%" }}
                transition={{ type: "spring", stiffness: 420, damping: 32 }}
              >
                <div className="flex items-start justify-between gap-4 px-5 pt-5 pb-3">
                  <div>
                    <RDialog.Title className="text-[15px] font-semibold text-fg">{title}</RDialog.Title>
                    {description ? <RDialog.Description className="mt-1 text-[12.5px] text-muted leading-snug">{description}</RDialog.Description> : <RDialog.Description className="sr-only">{String(title)}</RDialog.Description>}
                  </div>
                  <RDialog.Close asChild>
                    <IconButton label="Close" size="sm">
                      <X className="size-4" />
                    </IconButton>
                  </RDialog.Close>
                </div>
                <div className="min-h-0 overflow-y-auto px-5 pb-4">{children}</div>
                {footer && <div className="flex items-center justify-end gap-2 border-t border-line px-5 py-3 bg-hover/40">{footer}</div>}
              </motion.div>
            </RDialog.Content>
          </RDialog.Portal>
        )}
      </AnimatePresence>
    </RDialog.Root>
  );
}

// ---------------------------------------------------------------- menu

export function Menu({ trigger, children, align = "end" }: { trigger: ReactNode; children: ReactNode; align?: "start" | "end" }) {
  return (
    <RMenu.Root>
      <RMenu.Trigger asChild>{trigger}</RMenu.Trigger>
      <RMenu.Portal>
        <RMenu.Content align={align} sideOffset={6} className="glass-float z-50 min-w-48 rounded-md p-1 data-[state=open]:animate-in">
          {children}
        </RMenu.Content>
      </RMenu.Portal>
    </RMenu.Root>
  );
}

export function MenuItem({ icon, children, onSelect, danger }: { icon?: ReactNode; children: ReactNode; onSelect: () => void; danger?: boolean }) {
  return (
    <RMenu.Item
      onSelect={onSelect}
      className={cn("flex h-8 items-center gap-2.5 rounded-[7px] px-2.5 text-[13px] outline-none data-[highlighted]:bg-hover", danger ? "text-danger" : "text-fg")}
    >
      <span className="text-muted [&>svg]:size-4">{icon}</span>
      {children}
    </RMenu.Item>
  );
}

export function MenuSeparator() {
  return <RMenu.Separator className="my-1 h-px bg-line" />;
}

export function EmptyState({ icon, title, children }: { icon: ReactNode; title: ReactNode; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center text-center gap-2 py-10 px-6">
      <div className="size-11 rounded-lg bg-hover border border-line flex items-center justify-center text-muted [&>svg]:size-5">{icon}</div>
      <div className="text-[14px] font-semibold text-fg">{title}</div>
      {children && <div className="text-[12.5px] text-muted max-w-sm leading-relaxed">{children}</div>}
    </div>
  );
}
