import type { MemoryKind } from "@dolphin/shared";

/** Match the core's limits (crates/dolphin-core/src/memory/mod.rs). */
export const MEMORY_MAX_ENTRIES = 40;
export const MEMORY_MAX_TEXT = 300;

export const MEMORY_KINDS: { value: MemoryKind; label: string; hint: string }[] = [
  { value: "style", label: "Style", hint: "How it should sound: terse, no emojis, British English" },
  { value: "fact", label: "Fact", hint: "Something true about you or your setup" },
  { value: "workflow", label: "Workflow", hint: "How you like work done: run the tests first" },
  { value: "avoid", label: "Avoid", hint: "Something it should not do again" },
];

export const MEMORY_KIND_LABEL = Object.fromEntries(MEMORY_KINDS.map((k) => [k.value, k.label])) as Record<MemoryKind, string>;

/** An entry as stored: one line, single spaces, trimmed. Throws the user-facing reason when it can't be kept. */
export function cleanMemoryText(text: string): string {
  const s = text.split(/\s+/).filter(Boolean).join(" ");
  if (!s) throw "write what the agent should remember";
  if ([...s].length > MEMORY_MAX_TEXT) throw `a memory is limited to ${MEMORY_MAX_TEXT} characters; keep it to one thought`;
  return s;
}

/** A project tag as stored: trimmed, blank meaning "everywhere". */
export function cleanProject(project: string | null | undefined): string | null {
  const p = project?.trim();
  return p ? p : null;
}

/** The project a workspace belongs to: its folder name (the core tags memories the same way). */
export function projectOf(workspacePath: string | null | undefined): string | null {
  const parts = (workspacePath ?? "").split(/[\\/]+/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : null;
}
