import { describe, expect, it } from "vitest";
import type { CoreEvent } from "@dolphin/shared";
import { MEMORY_MAX_ENTRIES } from "@/lib/memory";
import { mockBackend } from "./mock";

// The mock stands in for the Rust core in the browser, so it has to follow the same memory rules.
describe("mock agent memory", () => {
  it("adds, edits, pins and forgets, telling the UI each time", async () => {
    const api = mockBackend();
    const events: CoreEvent[] = [];
    await api.subscribe((e) => events.push(e));
    const [rick, roger] = (await api.listAgents()).filter((a) => ["rick", "roger"].includes(a.slug));

    const m = await api.addAgentMemory(rick.id, "avoid", "  Don't rewrite\n the whole file ", "  ");
    expect([m.text, m.project, m.source, m.pinned]).toEqual(["Don't rewrite the whole file", null, "explicit", false]);

    const pinned = await api.updateAgentMemory({ ...m, pinned: true });
    expect(pinned.source).toBe("explicit");
    const reworded = await api.updateAgentMemory({ ...pinned, text: "Don't rewrite whole files", project: "api" });
    expect([reworded.source, reworded.project]).toEqual(["edited", "api"]);

    await expect(api.addAgentMemory(rick.id, "fact", "   ", null)).rejects.toMatch(/what the agent should remember/);
    await api.addAgentMemory(roger.id, "fact", "Roger's own", null);
    expect(await api.listAgentMemories(rick.id)).toHaveLength(1);

    expect(await api.forgetAgentMemories(rick.id)).toBe(1);
    expect(await api.listAgentMemories(rick.id)).toEqual([]);
    expect(await api.listAgentMemories(roger.id)).toHaveLength(1);
    expect(events.filter((e) => e.type === "memoriesChanged" && e.agentId === rick.id)).toHaveLength(4);
  });

  it("stops at the per-agent cap", async () => {
    const api = mockBackend();
    const [rick] = await api.listAgents();
    for (let i = 0; i < MEMORY_MAX_ENTRIES; i++) await api.addAgentMemory(rick.id, "fact", `fact ${i}`, null);
    await expect(api.addAgentMemory(rick.id, "fact", "one too many", null)).rejects.toMatch(/forget some first/);
  });

  it("rates an agent's message, and refuses to rate the user's", async () => {
    const api = mockBackend();
    const [rick] = await api.listAgents();
    const [coding] = await api.listChats();
    await api.sendMessage(coding.id, `@${rick.name} hello`, []);
    await new Promise((r) => setTimeout(r, 50));
    const msgs = await api.listMessages(coding.id, 50);
    const mine = msgs.find((m) => !m.agentId)!;
    await expect(api.rateMessage(mine.id, "up")).rejects.toMatch(/only an agent/);
  });

  it("keeps a persona suggestion apart from the persona until it is accepted", async () => {
    const api = mockBackend();
    const events: CoreEvent[] = [];
    await api.subscribe((e) => events.push(e));
    const [rick] = await api.listAgents();

    api.proposePersona(rick.id, "Dry and terse.", "asked for short answers");
    expect((await api.personaProposal(rick.id))?.text).toBe("Dry and terse.");
    expect((await api.listAgents())[0].persona).toBeNull();

    const dismissed = (await api.personaProposal(rick.id))!;
    await api.rejectPersonaProposal(dismissed.id);
    expect(await api.personaProposal(rick.id)).toBeNull();
    expect((await api.listAgents())[0].persona).toBeNull();

    api.proposePersona(rick.id, "Friendly and brief.", "asked again");
    const saved = await api.acceptPersonaProposal((await api.personaProposal(rick.id))!.id);
    expect(saved.persona).toBe("Friendly and brief.");
    expect(await api.personaProposal(rick.id)).toBeNull();
    await expect(api.acceptPersonaProposal(dismissed.id)).rejects.toMatch(/not found/);
    expect(events.filter((e) => e.type === "personaProposalChanged")).toHaveLength(4);
  });

  it("tests the embedding model only once one is chosen", async () => {
    const api = mockBackend();
    await expect(api.testEmbeddingModel()).rejects.toMatch(/pick an embedding model/);
    await api.setSetting("memory.embedding_model", null, { providerId: "p", modelId: "nomic-embed-text", effort: null, vision: false, tools: false, contextWindow: null });
    expect(await api.testEmbeddingModel()).toBe(768);
  });
});
