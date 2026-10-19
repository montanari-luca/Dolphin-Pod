import { ArrowDown, ArrowUp, Pencil, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import type { UserRole } from "@dolphin/shared";
import { GradientEditor } from "@/components/GradientEditor";
import { Button, Dialog, Field, Input } from "@/components/ui";
import { RoleBadge, RoleIcon, UserAvatar, UserNameText } from "@/components/user";
import { errorText } from "@/ipc/api";
import { cn } from "@/lib/cn";
import { ROLE_ICONS } from "@/lib/roleIcons";
import { useApp } from "@/store/app";
import { Card, PageHeader } from "./SettingsView";

export function ProfilePage() {
  const api = useApp((s) => s.api);
  const roles = useApp((s) => s.roles);
  const toast = useApp((s) => s.toast);
  const setSetting = useApp((s) => s.setSetting);
  const stored = (useApp((s) => s.settings["profile.display_name"]) as string) ?? "You";
  const [name, setName] = useState(stored);
  const [editing, setEditing] = useState<UserRole | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  useEffect(() => setName(stored), [stored]);

  const move = async (i: number, dir: -1 | 1) => {
    if (!api) return;
    const ids = roles.map((r) => r.id);
    [ids[i], ids[i + dir]] = [ids[i + dir], ids[i]];
    try {
      await api.reorderRoles(ids);
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  return (
    <>
      <PageHeader title="Profile" description="How you appear in chats. Roles are just for fun: give yourself as many as you like. Your first role colors your name." />

      <div className="mb-4 flex items-center gap-4 rounded-lg border border-line bg-surface-0 p-4">
        <UserAvatar size={56} />
        <div className="min-w-0 flex-1">
          <UserNameText className="text-[18px]" />
          <div className="mt-1.5 flex flex-wrap gap-1.5">
            {roles.map((r) => (
              <RoleBadge key={r.id} role={r} />
            ))}
          </div>
        </div>
      </div>

      <Card title="Name">
        <div className="py-3">
          <Field label="Display name" hint="Agents address you by this name. Leave it as “You” to stay anonymous.">
            <Input
              value={name}
              maxLength={32}
              onChange={(e) => setName(e.target.value)}
              onBlur={async () => {
                const v = name.trim() || "You";
                if (v !== stored && !(await setSetting("profile.display_name", v))) setName(stored);
                else setName(v);
              }}
              onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
            />
          </Field>
        </div>
      </Card>

      <div className="mb-2 mt-6 flex items-center justify-between">
        <div className="text-[12px] font-semibold uppercase tracking-wider text-faint">Your roles</div>
        <Button size="sm" variant="primary" onClick={() => setEditing({ id: "", name: "", icon: "star", color: { type: "solid", color: "#3d8bfd" }, sort: 0, updatedAt: 0 })}>
          <Plus className="size-3.5" /> New role
        </Button>
      </div>
      <div className="space-y-1.5">
        {roles.length === 0 && <div className="rounded-lg border border-dashed border-line-strong px-4 py-6 text-center text-[12.5px] text-faint">No roles. Create one to color your name.</div>}
        {roles.map((r, i) => (
          <div key={r.id} className="group flex items-center gap-3 rounded-lg border border-line bg-surface-0 px-3.5 py-2.5">
            <RoleIcon role={r} className="size-4" />
            <RoleBadge role={r} />
            {i === 0 && <span className="text-[11px] text-faint">colors your name</span>}
            <div className="ml-auto flex items-center gap-0.5">
              <Button size="sm" variant="ghost" disabled={i === 0} onClick={() => move(i, -1)} aria-label="Move up"><ArrowUp className="size-3.5" /></Button>
              <Button size="sm" variant="ghost" disabled={i === roles.length - 1} onClick={() => move(i, 1)} aria-label="Move down"><ArrowDown className="size-3.5" /></Button>
              <Button size="sm" variant="ghost" onClick={() => setEditing(r)} aria-label="Edit"><Pencil className="size-3.5" /></Button>
              {confirmDelete === r.id ? (
                <Button size="sm" variant="danger" onClick={async () => { await api?.deleteRole(r.id).catch((e) => toast("error", errorText(e))); setConfirmDelete(null); }}>
                  Delete?
                </Button>
              ) : (
                <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(r.id)} aria-label="Delete"><Trash2 className="size-3.5" /></Button>
              )}
            </div>
          </div>
        ))}
      </div>
      {editing && <RoleEditor initial={editing} onClose={() => setEditing(null)} />}
    </>
  );
}

function RoleEditor({ initial, onClose }: { initial: UserRole; onClose: () => void }) {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const [role, setRole] = useState(initial);
  const [saving, setSaving] = useState(false);
  const save = async () => {
    if (!api) return;
    setSaving(true);
    try {
      await api.saveRole(role);
      onClose();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setSaving(false);
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={initial.id ? `Edit ${initial.name}` : "New role"}
      width={560}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" loading={saving} disabled={!role.name.trim()} onClick={save}>Save role</Button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="flex items-center gap-2 rounded-md border border-line bg-hover/40 px-3 py-2.5">
          <span className="text-[12px] text-faint">Preview</span>
          <RoleIcon role={role} />
          <RoleBadge role={{ ...role, name: role.name.trim() || "Role" }} />
        </div>
        <Field label="Name">
          <Input autoFocus value={role.name} maxLength={24} onChange={(e) => setRole({ ...role, name: e.target.value })} placeholder="e.g. Architect" onKeyDown={(e) => e.key === "Enter" && role.name.trim() && save()} />
        </Field>
        <Field label="Icon">
          <div className="flex flex-wrap gap-1">
            {Object.entries(ROLE_ICONS).map(([k, Icon]) => (
              <button
                key={k}
                onClick={() => setRole({ ...role, icon: k })}
                aria-label={k}
                className={cn("size-8 inline-flex items-center justify-center rounded-sm transition-ui", role.icon === k ? "bg-accent-soft text-accent ring-1 ring-[var(--focus-ring)]" : "text-muted hover:bg-hover hover:text-fg")}
              >
                <Icon className="size-4" />
              </button>
            ))}
          </div>
        </Field>
        <Field label="Name color">
          <GradientEditor value={role.color} onChange={(color) => setRole({ ...role, color })} />
        </Field>
      </div>
    </Dialog>
  );
}
