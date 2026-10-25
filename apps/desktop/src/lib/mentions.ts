import type { Agent } from "@dolphin/shared";

/** The `@query` the caret is in, if any: { start, query }. */
export function mentionAt(text: string, caret: number): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const m = /(^|[^\w@])@([\p{L}\p{N}_-]*)$/u.exec(before);
  if (!m) return null;
  return { start: caret - m[2].length - 1, query: m[2] };
}

export function filterAgents(agents: Agent[], query: string): Agent[] {
  const q = query.toLowerCase();
  return agents
    .filter((a) => a.role !== "orchestrator")
    .filter((a) => !q || a.name.toLowerCase().startsWith(q) || a.slug.startsWith(q) || a.specialty.startsWith(q))
    .slice(0, 20);
}

export function insertMention(text: string, start: number, caret: number, name: string): { text: string; caret: number } {
  const next = `${text.slice(0, start)}@${name} ${text.slice(caret)}`;
  return { text: next, caret: start + name.length + 2 };
}

/** Split text into plain segments and @mentions of known agents. */
export function splitMentions(text: string, agents: Agent[]): (string | Agent)[] {
  const out: (string | Agent)[] = [];
  const re = /(^|[^\w@])@([\p{L}\p{N}_-]+)/gu;
  let last = 0;
  for (const m of text.matchAll(re)) {
    const agent = agents.find((a) => a.name.toLowerCase() === m[2].toLowerCase() || a.slug === m[2].toLowerCase());
    if (!agent) continue;
    const at = (m.index ?? 0) + m[1].length;
    if (at > last) out.push(text.slice(last, at));
    out.push(agent);
    last = at + m[2].length + 1;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}
