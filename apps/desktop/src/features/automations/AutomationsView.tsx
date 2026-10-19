import { AnimatePresence, motion } from "motion/react";
import { CalendarClock, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import type { Automation } from "@dolphin/shared";
import { Button, Dialog, EmptyState, Field, Input, Select, Switch, Textarea } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { chatIcon } from "@/lib/chatIcons";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

function describeDays(days: number[]) {
  if (days.length === 0 || days.length === 7) return "Every day";
  if (days.length === 5 && [0, 1, 2, 3, 4].every((d) => days.includes(d))) return "Weekdays";
  if (days.length === 2 && days.includes(5) && days.includes(6)) return "Weekends";
  return [...days].sort().map((d) => DAYS[d]).join(", ");
}

export function AutomationsView() {
  const api = useApp((s) => s.api);
  const chats = useApp((s) => s.chats);
  const toast = useApp((s) => s.toast);
  const [items, setItems] = useState<Automation[]>([]);
  const [editing, setEditing] = useState<Automation | null>(null);
  const load = async () => api && setItems(await api.listAutomations());
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api]);

  const save = async (a: Automation) => {
    if (!api) return false;
    try {
      await api.saveAutomation(a);
      await load();
      return true;
    } catch (e) {
      toast("error", errorText(e));
      return false;
    }
  };

  return (
    <section className="glass flex h-full min-w-0 flex-1 flex-col overflow-hidden rounded-xl">
      <header className="drag-region flex h-[60px] shrink-0 items-center gap-3 border-b border-line px-5">
        <CalendarClock className="size-[18px] text-muted" />
        <h1 className="text-[15px] font-semibold text-fg">Automations</h1>
        <div className="no-drag ml-auto">
          <Button
            variant="primary"
            size="sm"
            onClick={() => setEditing({ id: "", chatId: chats[0]?.id ?? "", text: "", timeOfDay: "09:00", weekdays: [], enabled: true, lastRunDate: null, updatedAt: 0 })}
          >
            <Plus className="size-3.5" /> New automation
          </Button>
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[720px] px-6 py-6">
          <p className="mb-5 text-[12.5px] text-muted leading-relaxed">
            Send a message to a chat at a set time of day. It goes through routing exactly as if you had typed it. Automations run only while Dolphin Pod is open on this device.
          </p>
          {items.length === 0 ? (
            <div className="rounded-lg border border-dashed border-line-strong">
              <EmptyState icon={<CalendarClock />} title="No automations yet">
                For example: every weekday at 08:30, ask the General chat for a summary of today's priorities.
              </EmptyState>
            </div>
          ) : (
            <div className="space-y-2">
              <AnimatePresence initial={false}>
                {items.map((a) => {
                  const chat = chats.find((c) => c.id === a.chatId);
                  const Icon = chatIcon(chat?.icon ?? "hash");
                  return (
                    <motion.div key={a.id} layout initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, scale: 0.98 }} className={cn("flex items-center gap-4 rounded-lg border border-line bg-surface-0 px-4 py-3", !a.enabled && "opacity-60")}>
                      <div className="w-16 text-center">
                        <div className="font-mono text-[18px] font-semibold tabular-nums text-fg">{a.timeOfDay}</div>
                        <div className="text-[10.5px] text-faint">{describeDays(a.weekdays)}</div>
                      </div>
                      <button className="min-w-0 flex-1 text-left" onClick={() => setEditing(a)}>
                        <div className="flex items-center gap-1.5 text-[12px] text-muted">
                          <Icon className="size-3.5" /> {chat?.name ?? "Deleted chat"}
                        </div>
                        <div className="truncate text-[13.5px] text-fg">{a.text}</div>
                      </button>
                      <Switch checked={a.enabled} onChange={(enabled) => void save({ ...a, enabled })} label="Enabled" />
                    </motion.div>
                  );
                })}
              </AnimatePresence>
            </div>
          )}
        </div>
      </div>
      {editing && (
        <AutomationEditor
          initial={editing}
          onClose={() => setEditing(null)}
          onSave={async (a) => (await save(a)) && setEditing(null)}
          onDelete={async () => {
            if (api && editing.id) await api.deleteAutomation(editing.id);
            await load();
            setEditing(null);
          }}
        />
      )}
    </section>
  );
}

function AutomationEditor({ initial, onClose, onSave, onDelete }: { initial: Automation; onClose: () => void; onSave: (a: Automation) => void; onDelete: () => void }) {
  const chats = useApp((s) => s.chats);
  const [a, setA] = useState(initial);
  const toggleDay = (d: number) => setA({ ...a, weekdays: a.weekdays.includes(d) ? a.weekdays.filter((x) => x !== d) : [...a.weekdays, d].sort() });
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={initial.id ? "Edit automation" : "New automation"}
      footer={
        <>
          {initial.id && (
            <Button variant="danger" className="mr-auto" onClick={onDelete}>
              <Trash2 className="size-3.5" /> Delete
            </Button>
          )}
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!a.text.trim() || !a.chatId} onClick={() => onSave(a)}>Save</Button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="grid grid-cols-[1fr_120px] gap-3">
          <Field label="Chat">
            <Select value={a.chatId} onChange={(e) => setA({ ...a, chatId: e.target.value })}>
              {chats.map((c) => (
                <option key={c.id} value={c.id}>{c.name}</option>
              ))}
            </Select>
          </Field>
          <Field label="Time">
            <Input type="time" value={a.timeOfDay} onChange={(e) => setA({ ...a, timeOfDay: e.target.value })} />
          </Field>
        </div>
        <Field label="Message" hint="Mentions work too, e.g. “@Prof quiz me on yesterday's topic”.">
          <Textarea rows={4} value={a.text} onChange={(e) => setA({ ...a, text: e.target.value })} placeholder="Summarize what's on my plate today" />
        </Field>
        <Field label="Repeat" hint={a.weekdays.length === 0 ? "No days selected means every day." : describeDays(a.weekdays)}>
          <div className="flex gap-1.5">
            {DAYS.map((d, i) => (
              <button key={d} onClick={() => toggleDay(i)} className={cn("h-8 w-11 rounded-sm border text-[12px] font-medium transition-ui", a.weekdays.includes(i) ? "border-transparent text-white [background:var(--brand-gradient)]" : "border-line text-muted hover:bg-hover")}>
                {d}
              </button>
            ))}
          </div>
        </Field>
      </div>
    </Dialog>
  );
}
