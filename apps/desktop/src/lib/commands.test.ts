import { describe, expect, it } from "vitest";
import { exactCommand, filterCommands, slashAt } from "./commands";

describe("slash commands", () => {
  it("only triggers for a lone slash token at the start", () => {
    expect(slashAt("/", 1)).toEqual({ query: "" });
    expect(slashAt("/cl", 3)).toEqual({ query: "cl" });
    expect(slashAt("/clear now", 10)).toBeNull();
    expect(slashAt("see /clear", 10)).toBeNull();
    expect(slashAt("/usr/bin", 8)).toBeNull();
  });

  it("filters by name and alias, hiding busy-only commands when idle", () => {
    expect(filterCommands("cl", false).map((c) => c.id)).toEqual(["clear"]);
    expect(filterCommands("log", false).map((c) => c.id)).toEqual(["action-log"]);
    expect(filterCommands("st", false).map((c) => c.id)).toEqual([]);
    expect(filterCommands("st", true).map((c) => c.id)).toEqual(["stop"]);
    expect(filterCommands("", false).length).toBeGreaterThan(5);
  });

  it("matches a complete command exactly", () => {
    expect(exactCommand("/clear", false)?.id).toBe("clear");
    expect(exactCommand("/CLEAN ", false)?.id).toBe("clear");
    expect(exactCommand("/cle", false)).toBeNull();
    expect(exactCommand("/stop", false)).toBeNull();
    expect(exactCommand("/nope", false)).toBeNull();
  });
});
