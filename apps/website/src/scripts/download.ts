// Keeps download buttons in sync with the live update manifest, so a new release shows up
// without a site rebuild. Build-time values are already in the HTML; this only refines them.
import { MANIFEST_URL } from "../lib/site";

type Manifest = Record<string, unknown>;
const PLATFORM_LABEL: Record<string, string> = { macos: "macOS", windows: "Windows", linux: "Linux", ios: "iOS" };

export function detectPlatform(): string | null {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const ua = `${nav.userAgentData?.platform ?? ""} ${navigator.userAgent}`;
  if (/iPhone|iPad|iPod/i.test(ua)) return "ios";
  if (/Android/i.test(ua)) return null;
  if (/Mac/i.test(ua)) return "macos";
  if (/Win/i.test(ua)) return "windows";
  if (/Linux|X11|CrOS/i.test(ua)) return "linux";
  return null;
}

const str = (v: unknown) => (typeof v === "string" ? v.trim() : "");
/** Links come from a remote file, so only ever accept https URLs for hrefs. */
const safeLink = (v: unknown) => (/^https:\/\//i.test(str(v)) ? str(v) : "");

function apply(m: Manifest, platform: string | null) {
  const version = str(m.latest);
  if (version) document.querySelectorAll("[data-dl-version]").forEach((el) => (el.textContent = version));

  document.querySelectorAll<HTMLAnchorElement>("[data-dl-platform]").forEach((el) => {
    const id = el.dataset.dlPlatform!;
    const link = safeLink(m[id]);
    const label = el.querySelector("[data-dl-label]");
    el.dataset.available = link ? "true" : "false";
    if (link) {
      el.href = link;
      el.removeAttribute("aria-disabled");
      if (label) label.textContent = `Download for ${PLATFORM_LABEL[id]}`;
    } else {
      el.removeAttribute("href");
      el.setAttribute("aria-disabled", "true");
      if (label) label.textContent = `${PLATFORM_LABEL[id]}: coming soon`;
    }
  });

  // Hero call to action: the visitor's platform if known, otherwise the downloads page.
  const hero = document.querySelector<HTMLAnchorElement>("[data-dl-hero]");
  if (hero) {
    const label = hero.querySelector("[data-dl-label]");
    const link = platform ? safeLink(m[platform]) : "";
    if (platform && label) label.textContent = link ? `Download for ${PLATFORM_LABEL[platform]}` : `${PLATFORM_LABEL[platform]} build coming soon`;
    hero.href = link || "/download/";
  }
}

export async function initDownloads() {
  if (!document.querySelector("[data-dl-platform], [data-dl-hero], [data-dl-version]")) return;
  const platform = detectPlatform();
  const inline = document.getElementById("manifest-data");
  let known: Manifest = {};
  try {
    known = inline?.textContent ? (JSON.parse(inline.textContent) as Manifest) : {};
  } catch {
    /* ignore malformed inline data */
  }
  apply(known, platform);
  try {
    const res = await fetch(MANIFEST_URL, { cache: "no-cache" });
    if (res.ok) apply((await res.json()) as Manifest, platform);
  } catch {
    /* offline or blocked: the build-time values stay */
  }
}

void initDownloads();
