export type CommandId = "clear" | "chat-settings" | "action-log" | "members" | "stop" | "attach" | "settings" | "agents" | "automations";

export interface SlashCommand {
  id: CommandId;
  /** Typed after the slash. */
  name: string;
  description: string;
  /** Extra names that also match. */
  aliases?: string[];
  /** Only offered while a chat is running. */
  whenBusy?: boolean;
}

export const COMMANDS: SlashCommand[] = [
  { id: "clear", name: "clear", description: "Clear all messages in this chat", aliases: ["clean"] },
  { id: "chat-settings", name: "chat-settings", description: "Edit this chat's name, agents and workspace", aliases: ["chat"] },
  { id: "action-log", name: "action-log", description: "Show what agents did in this chat", aliases: ["log"] },
  { id: "members", name: "members", description: "Show or hide the members panel" },
  { id: "stop", name: "stop", description: "Stop the agents that are running", whenBusy: true },
  { id: "attach", name: "attach", description: "Attach files to your message", aliases: ["upload"] },
  { id: "settings", name: "settings", description: "Open app settings" },
  { id: "agents", name: "agents", description: "Open agent settings" },
  { id: "automations", name: "automations", description: "Open automations" },
];

/** The `/query` being typed: the whole input must be a single slash token and the caret inside it. */
export function slashAt(text: string, caret: number): { query: string } | null {
  const m = /^\/([a-z-]*)$/i.exec(text);
  if (!m || caret < 1 || caret > text.length) return null;
  return { query: m[1].toLowerCase() };
}

export function filterCommands(query: string, busy: boolean): SlashCommand[] {
  const q = query.toLowerCase();
  return COMMANDS.filter((c) => !c.whenBusy || busy).filter((c) => !q || c.name.startsWith(q) || c.aliases?.some((a) => a.startsWith(q)));
}

/** The command a finished `/name` input names exactly, if any. */
export function exactCommand(text: string, busy: boolean): SlashCommand | null {
  const m = /^\/([a-z-]+)$/i.exec(text.trim());
  if (!m) return null;
  const q = m[1].toLowerCase();
  return filterCommands(q, busy).find((c) => c.name === q || c.aliases?.includes(q)) ?? null;
}
