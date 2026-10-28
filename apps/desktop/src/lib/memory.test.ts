import { describe, expect, it } from "vitest";
import { MEMORY_MAX_TEXT, affectedAgents, cleanMemoryText, cleanProject, normalizeShare, projectOf, sharedWith } from "./memory";

describe("memory text", () => {
  it("keeps one clean line", () => {
    expect(cleanMemoryText("  Use tabs,\n not   spaces \n")).toBe("Use tabs, not spaces");
  });

  it("refuses blank and over-long entries with a reason", () => {
    expect(() => cleanMemoryText(" \n ")).toThrow("write what the agent should remember");
    expect(() => cleanMemoryText("x".repeat(MEMORY_MAX_TEXT + 1))).toThrow(/limited to 300/);
    expect(cleanMemoryText("x".repeat(MEMORY_MAX_TEXT))).toHaveLength(MEMORY_MAX_TEXT);
  });

  it("treats a blank project as everywhere", () => {
    expect(cleanProject("  api ")).toBe("api");
    expect(cleanProject("  ")).toBeNull();
    expect(cleanProject(undefined)).toBeNull();
  });

  it("names a project after its workspace folder", () => {
    expect(projectOf("/Users/luca/Dev/Dolphin Pod")).toBe("Dolphin Pod");
    expect(projectOf("/Users/luca/Dev/api/")).toBe("api");
    expect(projectOf("C:\\Dev\\web")).toBe("web");
    expect(projectOf("/")).toBeNull();
    expect(projectOf(null)).toBeNull();
  });

  it("shares a note with everyone, with chosen agents or with nobody, but never back to its owner", () => {
    expect(sharedWith({ type: "none" }, "rick", "prof")).toBe(false);
    expect(sharedWith({ type: "all" }, "rick", "prof")).toBe(true);
    expect(sharedWith({ type: "all" }, "rick", "rick")).toBe(false);
    expect(sharedWith({ type: "agents", ids: ["prof"] }, "rick", "prof")).toBe(true);
    expect(sharedWith({ type: "agents", ids: ["prof"] }, "rick", "roger")).toBe(false);
  });

  it("stores sharing cleaned: real agents only, no owner, no repeats, empty means private", () => {
    const all = ["rick", "prof", "roger"];
    expect(normalizeShare({ type: "agents", ids: ["rick", "nobody", "prof", "prof"] }, "rick", all)).toEqual({ type: "agents", ids: ["prof"] });
    expect(normalizeShare({ type: "agents", ids: ["rick", "nobody"] }, "rick", all)).toEqual({ type: "none" });
    expect(normalizeShare({ type: "all" }, "rick", all)).toEqual({ type: "all" });
  });

  it("knows whose list a change reaches", () => {
    const all = ["rick", "prof", "roger"];
    expect(affectedAgents([{ agentId: "rick", share: { type: "none" } }], all)).toEqual(["rick"]);
    expect(affectedAgents([{ agentId: "rick", share: { type: "agents", ids: ["prof"] } }], all).sort()).toEqual(["prof", "rick"]);
    expect(affectedAgents([{ agentId: "rick", share: { type: "all" } }], all).sort()).toEqual(["prof", "rick", "roger"]);
  });
});
