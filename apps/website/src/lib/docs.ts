import { getCollection, type CollectionEntry } from "astro:content";

export type DocEntry = CollectionEntry<"docs">;

/** Sidebar order. Anything not listed is appended alphabetically so new docs are never lost. */
const ORDER = ["getting-started", "agents", "providers", "routing", "permissions", "sync", "remote-control", "ios", "profile"];
const GUIDE_ORDER = ["guides/ollama", "guides/lm-studio", "guides/openrouter"];

export const isGuide = (e: DocEntry) => e.id.startsWith("guides/");
export const routeFor = (e: DocEntry) => (isGuide(e) ? `/${e.id}/` : `/docs/${e.id}/`);

/** The first `# Heading` of the markdown source is the page title. */
export function titleOf(e: DocEntry): string {
  return e.body?.match(/^#\s+(.+)$/m)?.[1].trim() ?? e.id;
}

/** The text of the first paragraph, used as the meta description. */
export function summaryOf(e: DocEntry): string {
  const para = (e.body ?? "")
    .split(/\n{2,}/)
    .map((p) => p.trim())
    .find((p) => p && !p.startsWith("#") && !p.startsWith("|") && !p.startsWith("```") && !/^\d+\./.test(p));
  const text = (para ?? "").replace(/[*_`]/g, "").replace(/\[([^\]]+)\]\([^)]*\)/g, "$1").replace(/\s+/g, " ");
  return text.length > 200 ? `${text.slice(0, 197)}...` : text;
}

function sorted(list: DocEntry[], order: string[]) {
  const rank = (id: string) => (order.includes(id) ? order.indexOf(id) : order.length);
  return [...list].sort((a, b) => rank(a.id) - rank(b.id) || a.id.localeCompare(b.id));
}

export async function allDocs() {
  const all = await getCollection("docs");
  return { docs: sorted(all.filter((e) => !isGuide(e)), ORDER), guides: sorted(all.filter(isGuide), GUIDE_ORDER) };
}

/** Previous/next across the docs then guides, in sidebar order. */
export async function neighbours(entry: DocEntry) {
  const { docs, guides } = await allDocs();
  const flat = [...docs, ...guides];
  const i = flat.findIndex((e) => e.id === entry.id);
  return { prev: flat[i - 1], next: flat[i + 1] };
}
