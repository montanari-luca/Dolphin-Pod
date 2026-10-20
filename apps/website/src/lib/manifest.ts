import { MANIFEST_URL } from "./site";

export type Manifest = { latest: string; macos: string; windows: string; linux: string; ios: string };

/** Used when the manifest cannot be fetched at build time (offline CI, GitHub outage). */
export const FALLBACK_MANIFEST: Manifest = { latest: "0.1.0", macos: "", windows: "", linux: "", ios: "" };

export function normalize(raw: unknown): Manifest {
  const r = (raw ?? {}) as Record<string, unknown>;
  const s = (v: unknown) => (typeof v === "string" ? v.trim() : "");
  const url = (v: unknown) => (/^https:\/\//i.test(s(v)) ? s(v) : "");
  return {
    latest: s(r.latest) || FALLBACK_MANIFEST.latest,
    macos: url(r.macos),
    windows: url(r.windows),
    linux: url(r.linux),
    ios: url(r.ios),
  };
}

/** Build-time fetch so download links are static HTML; the client re-fetches to pick up newer releases. */
let cached: Promise<Manifest> | undefined;
export function loadManifest(): Promise<Manifest> {
  return (cached ??= fetchManifest());
}

async function fetchManifest(): Promise<Manifest> {
  try {
    const res = await fetch(MANIFEST_URL, { signal: AbortSignal.timeout(8000) });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return normalize(await res.json());
  } catch (e) {
    console.warn(`[manifest] using fallback (${e instanceof Error ? e.message : e})`);
    return FALLBACK_MANIFEST;
  }
}
