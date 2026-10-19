import { beforeEach, describe, expect, it } from "vitest";
import type { Message } from "@dolphin/shared";
import { useApp } from "./app";

const msg = (id: string, content = ""): Message => ({
  id, chatId: "c", agentId: "a", kind: "text", status: "streaming", content, reasoning: null, error: null, mentions: [], attachments: [],
  meta: {}, thinkingMs: null, workingMs: null, tokensIn: null, tokensOut: null, createdAt: 0, updatedAt: 0,
});

describe("store event handling", () => {
  beforeEach(() => useApp.setState({ messages: { c: [] } }));

  it("streams deltas into the right message and ignores duplicate creates", () => {
    const { apply } = useApp.getState();
    apply({ type: "messageCreated", message: msg("m1") });
    apply({ type: "messageCreated", message: msg("m1") });
    apply({ type: "messageDelta", chatId: "c", messageId: "m1", text: null, reasoning: "hmm " });
    apply({ type: "messageDelta", chatId: "c", messageId: "m1", text: "Hello", reasoning: null });
    apply({ type: "messageDelta", chatId: "c", messageId: "m1", text: " world", reasoning: null });
    const [m] = useApp.getState().messages.c;
    expect(useApp.getState().messages.c).toHaveLength(1);
    expect(m.content).toBe("Hello world");
    expect(m.reasoning).toBe("hmm ");
  });

  it("applies cleanup removals", () => {
    const { apply } = useApp.getState();
    apply({ type: "messageCreated", message: msg("a") });
    apply({ type: "messageCreated", message: msg("b") });
    apply({ type: "messagesRemoved", chatId: "c", messageIds: ["a"], all: false });
    expect(useApp.getState().messages.c.map((m) => m.id)).toEqual(["b"]);
    apply({ type: "messagesRemoved", chatId: "c", messageIds: [], all: true });
    expect(useApp.getState().messages.c).toEqual([]);
  });
});
