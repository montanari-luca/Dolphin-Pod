import { CheckCircle2, Copy, Cloud, KeyRound, Laptop, LifeBuoy, LockKeyhole, RefreshCw, Smartphone, Trash2, WifiOff } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import type { DeviceInfo, SyncState, SyncStatus } from "@dolphin/shared";
import { Badge, Button, ConfirmDialog, Dialog, Field, Input, Row } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";
import { Card, PageHeader } from "./SettingsView";

const STATE_LABEL: Record<SyncState, string> = {
  off: "Off",
  locked: "Locked",
  idle: "Up to date",
  syncing: "Syncing…",
  offline: "Offline",
  error: "Problem",
  revoked: "Removed",
};

const STATE_DOT: Record<SyncState, string> = {
  off: "bg-faint",
  locked: "bg-warning",
  idle: "bg-success",
  syncing: "bg-accent animate-pulse",
  offline: "bg-warning",
  error: "bg-danger",
  revoked: "bg-danger",
};

export function ago(ms: number | null | undefined): string {
  if (!ms) return "never";
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 10) return "just now";
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}

export function SyncPage() {
  const sync = useApp((s) => s.sync);
  // Lives here, not in the setup card: the card unmounts the moment sync turns on.
  const [recovery, setRecovery] = useState<string | null>(null);
  if (!sync) return null;
  return (
    <>
      <PageHeader
        title="Sync"
        description="Keep chats, agents, settings, providers and usage identical on every device you own. Everything is encrypted on this device with your passphrase before it leaves; the database only ever sees ciphertext."
      />
      {sync.state === "off" || sync.state === "revoked" ? <SetupCard status={sync} onCreated={setRecovery} /> : <ActiveCard status={sync} />}
      {recovery && <RecoveryDialog words={recovery} onDone={() => setRecovery(null)} />}
    </>
  );
}

// ------------------------------------------------------------------------------------------ setup

function SetupCard({ status, onCreated }: { status: SyncStatus; onCreated: (words: string) => void }) {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const [url, setUrl] = useState("");
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  const [recovering, setRecovering] = useState(false);

  const start = async () => {
    if (!api) return;
    setBusy(true);
    try {
      const r = await api.syncSetup(url.trim(), pass);
      setPass("");
      if (r.recoveryKey) onCreated(r.recoveryKey);
      else toast("success", "Sync is on. This device is joining your pod.");
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      {status.state === "revoked" && (
        <div className="mb-4 rounded-lg border border-warning/40 bg-warning/10 px-4 py-3 text-[12.5px] text-fg">
          This device was removed from the pod by another device, so it no longer syncs. Your local data is untouched. You can set sync up again below.
        </div>
      )}
      <Card>
        <div className="space-y-4 py-4">
          <Field
            label="Database URL"
            hint={
              <>
                A Postgres connection string. <button className="underline hover:text-fg" onClick={() => api?.openUrl("https://neon.tech")}>NeonDB</button> has a free tier, and any Postgres-compatible database works.
                Use the direct (non-pooled) connection. The URL is stored in your system keychain.
              </>
            }
          >
            <Input
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="postgresql://user:password@ep-example.neon.tech/neondb?sslmode=require"
              className="font-mono text-[12.5px]"
            />
          </Field>
          <Field label="Passphrase" hint="Encrypts everything. Use the same one on every device. It can't be reset, so keep the recovery key you'll get on first setup.">
            <Input type="password" autoComplete="off" value={pass} onChange={(e) => setPass(e.target.value)} placeholder="At least 8 characters" onKeyDown={(e) => e.key === "Enter" && url && pass && start()} />
          </Field>
          <div className="flex items-center justify-between gap-2 pt-1">
            <Button variant="ghost" size="sm" onClick={() => setRecovering(true)}><LifeBuoy className="size-3.5" />I forgot my passphrase</Button>
            <Button variant="primary" loading={busy} disabled={!url.trim() || pass.length < 8} onClick={start}><Cloud className="size-4" />Turn on sync</Button>
          </div>
        </div>
      </Card>
      <p className="text-[12px] leading-relaxed text-faint">
        First device: this creates your pod and shows a recovery key. Other devices: enter the same URL and passphrase to join it. Local models (Ollama, LM Studio) stay on the machine that runs them; other devices show them as unavailable.
      </p>
      {recovering && <RecoverDialog onClose={() => setRecovering(false)} />}
    </>
  );
}

export function RecoveryDialog({ words, onDone }: { words: string; onDone: () => void }) {
  const [saved, setSaved] = useState(false);
  const [copied, setCopied] = useState(false);
  const list = words.split(" ");
  return (
    <Dialog
      open
      onOpenChange={() => {}}
      title="Save your recovery key"
      description="If you forget your passphrase, these 24 words are the only way back in. They are shown once and not stored anywhere. Write them down or keep them in a password manager."
      width={560}
      footer={
        <>
          <label className="mr-auto flex items-center gap-2 text-[12.5px] text-muted">
            <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} className="size-4 accent-[var(--accent)]" />
            I saved my recovery key
          </label>
          <Button variant="primary" disabled={!saved} onClick={onDone}>Done</Button>
        </>
      }
    >
      <div className="selectable grid grid-cols-3 gap-x-4 gap-y-1.5 rounded-md border border-line bg-input p-4 font-mono text-[12.5px]">
        {list.map((w, i) => (
          <div key={i} className="flex gap-2">
            <span className="w-5 text-right text-faint">{i + 1}</span>
            <span className="text-fg">{w}</span>
          </div>
        ))}
      </div>
      <div className="mt-3 flex justify-end">
        <Button
          size="sm"
          onClick={async () => {
            await navigator.clipboard.writeText(words);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          }}
        >
          {copied ? <CheckCircle2 className="size-3.5" /> : <Copy className="size-3.5" />}
          {copied ? "Copied" : "Copy words"}
        </Button>
      </div>
    </Dialog>
  );
}

function RecoverDialog({ onClose }: { onClose: () => void }) {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const [url, setUrl] = useState("");
  const [words, setWords] = useState("");
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title="Recover with your recovery key"
      description="Enter the 24 words and choose a new passphrase. Your data is not lost; only the passphrase changes."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!url.trim() || words.trim().split(/\s+/).length < 24 || pass.length < 8}
            onClick={async () => {
              if (!api) return;
              setBusy(true);
              try {
                await api.syncRecover(url.trim(), words, pass);
                toast("success", "Recovered. Sync is on with your new passphrase.");
                onClose();
              } catch (e) {
                toast("error", errorText(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            Recover
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <Field label="Database URL">
          <Input type="password" autoComplete="off" value={url} onChange={(e) => setUrl(e.target.value)} className="font-mono text-[12.5px]" />
        </Field>
        <Field label="Recovery key (24 words)">
          <textarea value={words} onChange={(e) => setWords(e.target.value)} rows={3} spellCheck={false} className="w-full resize-none rounded-sm border border-line bg-input px-3 py-2 font-mono text-[12.5px] text-fg outline-none focus:border-[var(--focus-ring)]" />
        </Field>
        <Field label="New passphrase">
          <Input type="password" autoComplete="off" value={pass} onChange={(e) => setPass(e.target.value)} placeholder="At least 8 characters" />
        </Field>
      </div>
    </Dialog>
  );
}

// ------------------------------------------------------------------------------------------ active

function ActiveCard({ status }: { status: SyncStatus }) {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [dialog, setDialog] = useState<null | "passphrase" | "recovery" | "off">(null);
  const [name, setName] = useState(status.deviceName);
  const [revoking, setRevoking] = useState<DeviceInfo | null>(null);

  const load = useCallback(async () => {
    if (!api) return;
    try {
      setDevices(await api.syncDevices());
    } catch {
      /* offline: keep the last list */
    }
  }, [api]);
  useEffect(() => {
    void load();
    const t = setInterval(load, 20_000);
    return () => clearInterval(t);
  }, [load, status.lastSyncAt]);
  useEffect(() => setName(status.deviceName), [status.deviceName]);

  if (status.state === "locked") return <UnlockCard />;

  return (
    <>
      <Card>
        <Row
          title={
            <span className="flex items-center gap-2">
              <span className={cn("size-2 rounded-full", STATE_DOT[status.state])} />
              {STATE_LABEL[status.state]}
              {status.state === "offline" && <WifiOff className="size-3.5 text-warning" />}
            </span>
          }
          description={
            <>
              {status.provider} · last sync {ago(status.lastSyncAt)}
              {status.pending > 0 && ` · ${status.pending} change${status.pending === 1 ? "" : "s"} waiting`}
            </>
          }
        >
          <Button
            size="sm"
            loading={busy || status.state === "syncing"}
            onClick={async () => {
              if (!api) return;
              setBusy(true);
              try {
                await api.syncNow();
                await load();
              } catch (e) {
                toast("error", errorText(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            <RefreshCw className="size-3.5" />Sync now
          </Button>
        </Row>
        {status.error && <div className="selectable py-2.5 text-[12px] text-danger">{status.error}</div>}
        <Row title="This device's name" description="Shown in the device list on your other devices.">
          <div className="flex gap-2">
            <Input value={name} onChange={(e) => setName(e.target.value)} className="w-52" maxLength={40} />
            <Button size="sm" disabled={!name.trim() || name === status.deviceName} onClick={() => api?.setDeviceName(name).then(load).catch((e) => toast("error", errorText(e)))}>Save</Button>
          </div>
        </Row>
      </Card>

      <Card title="Devices">
        {devices.length === 0 && <div className="py-3 text-[12.5px] text-faint">No devices yet.</div>}
        {devices.map((d) => (
          <div key={d.id} className="flex items-center gap-3 py-3">
            <div className="flex size-9 items-center justify-center rounded-md border border-line bg-hover text-muted">
              {d.kind === "phone" ? <Smartphone className="size-4" /> : <Laptop className="size-4" />}
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2 text-[13px] font-medium text-fg">
                <span className="truncate">{d.name}</span>
                {d.thisDevice && <Badge tone="accent">this device</Badge>}
                {d.revoked && <Badge tone="danger">removed</Badge>}
              </div>
              <div className="text-[11.5px] text-faint">{d.platform} · {d.thisDevice ? "active now" : `seen ${ago(d.lastSeen)}`}</div>
            </div>
            {!d.thisDevice && !d.revoked && (
              <Button size="sm" variant="ghost" onClick={() => setRevoking(d)}><Trash2 className="size-3.5" />Remove</Button>
            )}
          </div>
        ))}
      </Card>

      <Card title="Security">
        <Row title="Passphrase" description="Changing it re-wraps your key; nothing is re-uploaded.">
          <Button size="sm" onClick={() => setDialog("passphrase")}><LockKeyhole className="size-3.5" />Change</Button>
        </Row>
        <Row title="Recovery key" description="Make a new one if you lost the old words. The old key stops working.">
          <Button size="sm" onClick={() => setDialog("recovery")}><KeyRound className="size-3.5" />Create new</Button>
        </Row>
        <Row title="Turn off sync on this device" description="Stops syncing and forgets the key and database address here. Data on this device and in the database is kept.">
          <Button size="sm" variant="danger" onClick={() => setDialog("off")}>Turn off</Button>
        </Row>
      </Card>

      {dialog === "passphrase" && <ChangePassphrase onClose={() => setDialog(null)} />}
      {dialog === "recovery" && <NewRecovery onClose={() => setDialog(null)} />}
      <ConfirmDialog
        open={dialog === "off"}
        onOpenChange={(o) => !o && setDialog(null)}
        title="Turn off sync on this device?"
        description="Chats stay here, but new messages will no longer reach your other devices until you set sync up again."
        confirmLabel="Turn off"
        onConfirm={() => api?.syncDisable()}
      />
      <ConfirmDialog
        open={!!revoking}
        onOpenChange={(o) => !o && setRevoking(null)}
        title={`Remove ${revoking?.name ?? "device"}?`}
        description="It stops syncing the next time it connects. To lock out a lost device completely, also change the database password and your passphrase."
        confirmLabel="Remove device"
        onConfirm={async () => {
          if (revoking) await api?.syncRevokeDevice(revoking.id).catch((e) => toast("error", errorText(e)));
          await load();
        }}
      />
    </>
  );
}

function UnlockCard() {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Card>
      <div className="space-y-3 py-4">
        <div className="text-[13px] font-medium text-fg">Enter your passphrase to continue syncing</div>
        <p className="text-[12px] text-faint">The key saved on this device is gone (for example after a keychain reset).</p>
        <Input type="password" value={pass} onChange={(e) => setPass(e.target.value)} placeholder="Passphrase" autoFocus />
        <Button
          variant="primary"
          loading={busy}
          disabled={!pass}
          onClick={async () => {
            if (!api) return;
            setBusy(true);
            try {
              await api.syncUnlock(pass);
            } catch (e) {
              toast("error", errorText(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          Unlock
        </Button>
      </div>
    </Card>
  );
}

function ChangePassphrase({ onClose }: { onClose: () => void }) {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const [oldP, setOld] = useState("");
  const [newP, setNew] = useState("");
  const [again, setAgain] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title="Change passphrase"
      description="Other devices keep working; they use the saved key. A device that has to sign in again will need the new passphrase."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!oldP || newP.length < 8 || newP !== again}
            onClick={async () => {
              if (!api) return;
              setBusy(true);
              try {
                await api.syncChangePassphrase(oldP, newP);
                toast("success", "Passphrase changed");
                onClose();
              } catch (e) {
                toast("error", errorText(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            Change passphrase
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <Field label="Current passphrase"><Input type="password" value={oldP} onChange={(e) => setOld(e.target.value)} autoFocus /></Field>
        <Field label="New passphrase"><Input type="password" value={newP} onChange={(e) => setNew(e.target.value)} placeholder="At least 8 characters" /></Field>
        <Field label="New passphrase again" hint={again && newP !== again ? "These don't match." : undefined}><Input type="password" value={again} onChange={(e) => setAgain(e.target.value)} /></Field>
      </div>
    </Dialog>
  );
}

function NewRecovery({ onClose }: { onClose: () => void }) {
  const api = useApp((s) => s.api);
  const toast = useApp((s) => s.toast);
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  const [words, setWords] = useState<string | null>(null);
  if (words) return <RecoveryDialog words={words} onDone={onClose} />;
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title="Create a new recovery key"
      description="Enter your passphrase to confirm. The previous recovery key stops working."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!pass}
            onClick={async () => {
              if (!api) return;
              setBusy(true);
              try {
                setWords(await api.syncNewRecoveryKey(pass));
              } catch (e) {
                toast("error", errorText(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            Create
          </Button>
        </>
      }
    >
      <Input type="password" value={pass} onChange={(e) => setPass(e.target.value)} placeholder="Passphrase" autoFocus />
    </Dialog>
  );
}
