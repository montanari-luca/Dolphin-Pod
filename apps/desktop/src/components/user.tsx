import * as HoverCard from "@radix-ui/react-hover-card";
import { motion } from "motion/react";
import { UserRound } from "lucide-react";
import type { ReactNode } from "react";
import type { UserRole } from "@dolphin/shared";
import { cn } from "@/lib/cn";
import { cssBackground, readableTextBackground } from "@/lib/color";
import { roleIcon } from "@/lib/roleIcons";
import { useResolvedTheme } from "@/lib/theme";
import { useApp } from "@/store/app";

/** The user's display name and roles; the first role colors the name. */
export function useUser() {
  const name = (useApp((s) => s.settings["profile.display_name"]) as string | undefined)?.trim() || "You";
  const roles = useApp((s) => s.roles);
  return { name, roles, primary: roles[0] as UserRole | undefined };
}

function textStyle(role: UserRole | undefined, theme: "dark" | "light") {
  if (!role) return {};
  const bg = readableTextBackground(role.color, theme);
  return bg.startsWith("linear")
    ? { backgroundImage: bg, color: "transparent", WebkitBackgroundClip: "text", backgroundClip: "text" }
    : { color: bg };
}

export function RoleIcon({ role, className }: { role: UserRole; className?: string }) {
  const Icon = roleIcon(role.icon);
  const theme = useResolvedTheme();
  const color = readableTextBackground(role.color, theme);
  // Gradient icons: tint with the first stop (SVG strokes can't use background-clip).
  const stroke = color.startsWith("linear") ? (role.color.type === "gradient" ? role.color.stops[0]?.color : undefined) : color;
  return <Icon className={cn("size-3.5 shrink-0", className)} style={{ color: stroke }} strokeWidth={2.2} />;
}

export function UserNameText({ className, withIcon = true }: { className?: string; withIcon?: boolean }) {
  const { name, primary } = useUser();
  const theme = useResolvedTheme();
  return (
    <span className={cn("inline-flex items-center gap-1 font-semibold", className)}>
      <span style={textStyle(primary, theme)} className={cn(!primary && "text-fg")}>{name}</span>
      {withIcon && primary && <RoleIcon role={primary} />}
    </span>
  );
}

export function RoleBadge({ role, className }: { role: UserRole; className?: string }) {
  const theme = useResolvedTheme();
  return (
    <span className={cn("inline-flex h-5 items-center gap-1 rounded-full border border-line bg-hover/70 pl-1.5 pr-2 text-[11px] font-semibold", className)}>
      <span className="size-2 rounded-full" style={{ background: cssBackground(role.color) }} />
      <span style={textStyle(role, theme)}>{role.name}</span>
    </span>
  );
}

export function UserAvatar({ size = 32, className }: { size?: number; className?: string }) {
  const { name, primary } = useUser();
  const initial = name.toLowerCase() === "you" ? null : name.slice(0, 1).toUpperCase();
  return (
    <span
      className={cn("inline-flex shrink-0 rounded-full p-[2px]", className)}
      style={{ width: size, height: size, background: primary ? cssBackground(primary.color) : "var(--border-strong)" }}
    >
      <span className="flex size-full items-center justify-center rounded-full bg-surface-solid text-fg font-semibold" style={{ fontSize: size * 0.42 }}>
        {initial ?? <UserRound style={{ width: size * 0.5, height: size * 0.5 }} className="text-muted" />}
      </span>
    </span>
  );
}

export function UserCardContent() {
  const { name, roles, primary } = useUser();
  const setView = useApp((s) => s.setView);
  return (
    <div className="w-[280px]">
      <div className="h-14 rounded-t-lg" style={{ background: primary ? cssBackground(primary.color) : "var(--brand-gradient)" }} />
      <div className="px-4 pb-4 -mt-7">
        <UserAvatar size={52} className="ring-4 ring-[var(--surface-float)]" />
        <div className="mt-2">
          <UserNameText className="text-[16px]" />
          {name.toLowerCase() === "you" && <div className="text-[11.5px] text-faint">Set your name in Settings → Profile</div>}
        </div>
        <div className="mt-3 text-[10.5px] font-semibold uppercase tracking-wider text-faint">Roles</div>
        <div className="mt-1.5 flex flex-wrap gap-1.5">
          {roles.length ? roles.map((r) => <RoleBadge key={r.id} role={r} />) : <span className="text-[12px] text-faint">No roles</span>}
        </div>
        <button className="mt-3 text-[12px] text-muted hover:text-fg transition-ui" onClick={() => setView("settings", "profile")}>
          Edit profile and roles
        </button>
      </div>
    </div>
  );
}

export function UserHoverCard({ children, side = "top" }: { children: ReactNode; side?: "top" | "right" | "bottom" | "left" }) {
  return (
    <HoverCard.Root openDelay={280} closeDelay={120}>
      <HoverCard.Trigger asChild>{children}</HoverCard.Trigger>
      <HoverCard.Portal>
        <HoverCard.Content side={side} sideOffset={8} collisionPadding={12} className="z-50 outline-none">
          <motion.div className="glass-float overflow-hidden rounded-lg" initial={{ opacity: 0, y: 6, scale: 0.98 }} animate={{ opacity: 1, y: 0, scale: 1 }} transition={{ type: "spring", stiffness: 500, damping: 34 }}>
            <UserCardContent />
          </motion.div>
        </HoverCard.Content>
      </HoverCard.Portal>
    </HoverCard.Root>
  );
}
