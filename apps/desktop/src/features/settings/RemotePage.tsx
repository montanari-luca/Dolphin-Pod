import { Laptop, Radio, ShieldCheck, Smartphone, Wifi } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import type { DeviceInfo, RemoteSettings } from "@dolphin/shared";
import { Badge, Row, Switch } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { useApp } from "@/store/app";
import { ago } from "./SyncPage";
import { Card, PageHeader } from "./SettingsView";

export function RemotePage() {
  const api = useApp((s) => s.api);
  const sync = useApp((s) => s.sync);
  const toast = useApp((s) => s.toast);
  const setView = useApp((s) => s.setView);
  const [remote, setRemote] = useState<RemoteSettings | null>(null);
  const [devices, setDevices] = useState<DeviceInfo[]>([]);

  const load = useCallback(async () => {
    if (!api) return;
    setRemote(await api.remoteSettings());
    try {
      setDevices(await api.syncDevices());
    } catch {
      /* offline */
    }
  }, [api]);
  useEffect(() => {
    void load();
  }, [load, sync?.state]);

  const syncOn = sync && sync.state !== "off" && sync.state !== "revoked";
  const others = devices.filter((d) => !d.thisDevice && !d.revoked);

  return (
    <>
      <PageHeader
        title="Remote control"
        description="Let your iPhone start work on this computer, where your agents, files and local models are. Requests come in over your Wi-Fi when you are nearby and through your sync database when you are not."
      />
      {!syncOn && (
        <div className="mb-4 rounded-lg border border-line bg-surface-0 px-4 py-3 text-[12.5px] text-muted">
          Remote control needs sync, which is how your devices find each other and keep requests private.{" "}
          <button className="underline hover:text-fg" onClick={() => setView("settings", "sync")}>Set up sync</button>
        </div>
      )}
      <Card>
        <Row
          title="Accept remote requests on this computer"
          description="Off by default. While on, this computer listens for requests from devices you allow below. Every request shows up in the chat, marked as remote, and in the action log."
        >
          <Switch
            checked={remote?.enabled ?? false}
            disabled={!syncOn}
            label="Accept remote requests"
            onChange={async (enabled) => {
              try {
                setRemote(await api!.setRemoteEnabled(enabled));
                await load();
              } catch (e) {
                toast("error", errorText(e));
              }
            }}
          />
        </Row>
        {remote?.enabled && (
          <div className="flex items-center gap-2 py-2.5 text-[12px] text-muted">
            <Wifi className="size-3.5" />
            {remote.lanPort ? <>Nearby connections are on (port {remote.lanPort}). Your phone may ask for Local Network access once.</> : <>Starting the nearby listener…</>}
          </div>
        )}
      </Card>

      <Card title="Devices that may control this computer">
        {others.length === 0 && <div className="py-3 text-[12.5px] text-faint">No other devices in your pod yet. Turn on sync on your iPhone with the same database and passphrase.</div>}
        {others.map((d) => (
          <div key={d.id} className="flex items-center gap-3 py-3">
            <div className="flex size-9 items-center justify-center rounded-md border border-line bg-hover text-muted">
              {d.kind === "phone" ? <Smartphone className="size-4" /> : <Laptop className="size-4" />}
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2 text-[13px] font-medium text-fg">
                <span className="truncate">{d.name}</span>
                {d.authorized && <Badge tone="success"><ShieldCheck className="size-3" />allowed</Badge>}
              </div>
              <div className="text-[11.5px] text-faint">{d.platform} · seen {ago(d.lastSeen)}</div>
            </div>
            <Switch
              checked={d.authorized}
              label={`Allow ${d.name}`}
              onChange={async (allow) => {
                try {
                  await api!.remoteAuthorize(d.id, allow);
                  await load();
                } catch (e) {
                  toast("error", errorText(e));
                }
              }}
            />
          </div>
        ))}
      </Card>
      <p className="flex gap-2 text-[12px] leading-relaxed text-faint">
        <Radio className="mt-0.5 size-3.5 shrink-0" />
        Allowing a device pins its signing key. Each request is signed by that device, expires after five minutes and runs at most once, and agents keep the same permission rules as local use: read-only until you approve a plan.
      </p>
    </>
  );
}
