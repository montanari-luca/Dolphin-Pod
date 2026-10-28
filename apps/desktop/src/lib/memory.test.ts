import { describe, expect, it } from "vitest";
import { MEMORY_MAX_TEXT, cleanMemoryText, cleanProject, projectOf } from "./memory";

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
});
