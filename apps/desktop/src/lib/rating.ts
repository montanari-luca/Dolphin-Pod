import type { Message, Rating } from "@dolphin/shared";

/** The rating stored on a message, if any. */
export function ratingOf(message: Pick<Message, "meta">): Rating | null {
  const r = (message.meta as { rating?: unknown } | null)?.rating;
  return r === "up" || r === "down" ? r : null;
}

/** What a click on a thumb does: choose that rating, or take it back when it already is the rating. */
export function nextRating(current: Rating | null, clicked: Rating): Rating | null {
  return current === clicked ? null : clicked;
}
