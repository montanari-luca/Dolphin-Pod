import { describe, expect, it } from "vitest";
import { nextRating, ratingOf } from "./rating";

describe("message ratings", () => {
  it("reads the rating from the message, ignoring anything else", () => {
    expect(ratingOf({ meta: { rating: "up" } })).toBe("up");
    expect(ratingOf({ meta: { rating: "down" } })).toBe("down");
    expect(ratingOf({ meta: { rating: "meh" } })).toBeNull();
    expect(ratingOf({ meta: {} })).toBeNull();
    expect(ratingOf({ meta: null })).toBeNull();
  });

  it("a thumb click chooses, switches or takes back", () => {
    expect(nextRating(null, "up")).toBe("up");
    expect(nextRating("up", "down")).toBe("down");
    expect(nextRating("down", "down")).toBeNull();
  });
});
