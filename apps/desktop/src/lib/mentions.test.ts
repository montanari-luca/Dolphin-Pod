import { describe, expect, it } from "vitest";
import type { Agent } from "@dolphin/shared";
import { filterAgents, insertMention, mentionAt, splitMentions } from "./mentions";

const agent = (slug: string, name: string, role: Agent["role"] = "specialist"): Agent => ({
  id: slug, slug, name, role, specialty: slug === "lincoln" ? "orchestrator" : "coder", job: "", color: { type: "solid", color: "#fff" },
  model: null, imageModel: null, iosModel: null, sort: 0, updatedAt: 0,
});
const agents = [agent("roger", "Roger", "moderator"), agent("lincoln", "Lincoln", "orchestrator"), agent("rick", "Rick"), agent("prof", "Prof")];

describe("mentions", () => {
  it("detects the @query at the caret, not emails", () => {
    expect(mentionAt("hey @Ri", 7)).toEqual({ start: 4, query: "Ri" });
    expect(mentionAt("@", 1)).toEqual({ start: 0, query: "" });
    expect(mentionAt("mail a@b", 8)).toBeNull();
    expect(mentionAt("@Rick done", 10)).toBeNull();
  });

  it("autocomplete excludes the orchestrator", () => {
    expect(filterAgents(agents, "").map((a) => a.name)).toEqual(["Roger", "Rick", "Prof"]);
    expect(filterAgents(agents, "r").map((a) => a.name)).toEqual(["Roger", "Rick"]);
  });

  it("inserts a mention and moves the caret after it", () => {
    expect(insertMention("ask @pr now", 4, 7, "Prof")).toEqual({ text: "ask @Prof  now", caret: 10 });
  });

  it("splits known mentions out of text", () => {
    const parts = splitMentions("@Rick and @nobody, ask @prof", agents);
    expect(parts.map((p) => (typeof p === "string" ? p : `<${p.name}>`)).join("")).toBe("<Rick> and @nobody, ask <Prof>");
  });
});
