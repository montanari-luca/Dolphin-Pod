import { AtSign, ExternalLink, FolderOpen, RefreshCw } from "lucide-react";
import { useState } from "react";
import { Badge, Button } from "@/components/ui";
import { errorText } from "@/ipc/api";
import { useApp } from "@/store/app";

export function AboutPage() {
  const info = useApp((s) => s.info);
  const api = useApp((s) => s.api);
  const update = useApp((s) => s.update);
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  const check = async () => {
    if (!api) return;
    setChecking(true);
    setResult(null);
    try {
      const u = await api.checkUpdate();
      if (u.available && u.url) {
        useApp.setState({ update: { version: u.latest, url: u.url } });
        setResult(`Version ${u.latest} is available.`);
      } else setResult(`You're up to date (${u.current}).`);
    } catch (e) {
      setResult(errorText(e));
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="pt-10 pb-6">
      <div className="flex flex-col items-center text-center">
        <img src="/brand/dolphin-icon.png" alt="Dolphin Pod" className="size-24 drop-shadow-xl" draggable={false} />
        <h2 className="mt-4 text-[26px] font-semibold tracking-tight brand-text">Dolphin Pod</h2>
        <p className="mt-1 text-[13px] text-muted">Your pod of AI agents, in one chat.</p>
        <div className="mt-3 flex items-center gap-2">
          <Badge>Version {info?.version}</Badge>
          <Badge>{info?.platform}</Badge>
        </div>
        <div className="mt-5 flex items-center gap-2">
          <Button loading={checking} onClick={check}><RefreshCw className="size-3.5" />Check for updates</Button>
          {update && <Button variant="primary" onClick={() => api?.openUrl(update.url)}>Download {update.version}</Button>}
        </div>
        {result && <p className="mt-2 max-w-md text-[12px] text-faint selectable">{result}</p>}
      </div>

      <div className="mx-auto mt-10 max-w-lg rounded-xl border border-line bg-surface-0 p-5">
        <div className="text-[11px] font-semibold uppercase tracking-wider text-faint">Made by</div>
        <button onClick={() => api?.openUrl("https://mintflow.dev")} className="mt-3 flex w-full items-center gap-3 rounded-md p-2 -mx-2 text-left hover:bg-hover transition-ui">
          <img src="/brand/mintflow-mark.webp" alt="" className="size-10 rounded-md" draggable={false} />
          <div className="flex-1">
            <div className="text-[14px] font-semibold text-fg">MintFlow Technologies</div>
            <div className="text-[12px] text-muted">mintflow.dev</div>
          </div>
          <ExternalLink className="size-4 text-faint" />
        </button>
        <button onClick={() => api?.openUrl("https://instagram.com/montanarii.luca")} className="mt-1 flex w-full items-center gap-3 rounded-md p-2 -mx-2 text-left hover:bg-hover transition-ui">
          <div className="flex size-10 items-center justify-center rounded-md [background:var(--brand-gradient)] text-white">
            <AtSign className="size-5" />
          </div>
          <div className="flex-1">
            <div className="text-[14px] font-semibold text-fg">Luca Montanari</div>
            <div className="text-[12px] text-muted">Lead developer · @montanarii.luca</div>
          </div>
          <ExternalLink className="size-4 text-faint" />
        </button>
      </div>

      <div className="mx-auto mt-4 max-w-lg text-center text-[11.5px] text-faint space-y-1">
        <p>© 2026 MintFlow Technologies. All rights reserved.</p>
        {info?.dataDir && api?.isTauri && (
          <button onClick={() => api.revealPath(info.dataDir)} className="inline-flex items-center gap-1 hover:text-muted">
            <FolderOpen className="size-3" /> Show data folder
          </button>
        )}
      </div>
    </div>
  );
}
