import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Agent, AgentMemory, Message } from "@dolphin/shared";
import type { Backend } from "@/ipc/api";
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

  it("shows a question until it is closed", () => {
    const { apply } = useApp.getState();
    apply({ type: "questionRequest", id: "q1", chatId: "c", agentId: "a", question: "Which?", options: [{ label: "A", description: null }] });
    expect(useApp.getState().questions.map((q) => q.id)).toEqual(["q1"]);
    apply({ type: "questionClosed", id: "q1" });
    expect(useApp.getState().questions).toEqual([]);
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

  it("counts memory changes per agent so the memory list reloads", () => {
    const { apply } = useApp.getState();
    useApp.setState({ memoryVersion: {} });
    apply({ type: "memoriesChanged", agentId: "rick" });
    apply({ type: "memoriesChanged", agentId: "rick" });
    apply({ type: "memoriesChanged", agentId: "roger" });
    expect(useApp.getState().memoryVersion).toEqual({ rick: 2, roger: 1 });
  });

  describe("when an agent learns", () => {
    const rick = { id: "a-rick", name: "Rick" } as Agent;
    const note: AgentMemory = { id: "m1", agentId: "a-rick", kind: "style", text: "Use tabs", project: null, source: "auto", pinned: false, hits: 0, createdAt: 0, updatedAt: 0 };
    const deleteAgentMemory = vi.fn().mockResolvedValue(undefined);
    const updateAgentMemory = vi.fn().mockResolvedValue(undefined);

    beforeEach(() => {
      deleteAgentMemory.mockClear();
      updateAgentMemory.mockClear();
      useApp.setState({ toasts: [], agents: [rick], api: { deleteAgentMemory, updateAgentMemory } as unknown as Backend });
    });

    it("says so, and Undo forgets a new note", async () => {
      useApp.getState().apply({ type: "memoryLearned", memory: note, previousText: null });
      const [t] = useApp.getState().toasts;
      expect(t.text).toBe("Rick learned: “Use tabs”");
      expect(t.action?.label).toBe("Undo");
      await t.action!.run();
      expect(deleteAgentMemory).toHaveBeenCalledWith("m1");
      expect(updateAgentMemory).not.toHaveBeenCalled();
    });

    it("Undo puts a reworded note's old text back", async () => {
      useApp.getState().apply({ type: "memoryLearned", memory: { ...note, text: "Use tabs, never spaces" }, previousText: "Use tabs" });
      const [t] = useApp.getState().toasts;
      expect(t.text).toBe("Rick updated a note: “Use tabs, never spaces”");
      await t.action!.run();
      expect(updateAgentMemory).toHaveBeenCalledWith({ ...note, text: "Use tabs" });
      expect(deleteAgentMemory).not.toHaveBeenCalled();
    });

    it("a failed Undo says why instead of failing silently", async () => {
      deleteAgentMemory.mockRejectedValueOnce("that note is already gone");
      useApp.getState().apply({ type: "memoryLearned", memory: note, previousText: null });
      await useApp.getState().toasts[0].action!.run();
      expect(useApp.getState().toasts.map((x) => x.level)).toContain("error");
    });
  });
});
