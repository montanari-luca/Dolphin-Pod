# Routing: Roger, Lincoln and @mentions

1. Roger posts an instant acknowledgment (no model call).
2. Roger's model decides the intent and mode: **answer** (he replies himself), **learn**, **quick**, **plan**,
   **implement**, **internal** (settings and housekeeping) or **clarify**.
3. Lincoln writes a brief for the chosen specialist (lightly or fully improving your prompt) and dispatches it.
4. The specialist answers in the chat, with its thinking streamed live.

`@Name` skips steps 1-3 and goes straight to that agent. If Roger has no model, keyword routing is used and the
acknowledgment says so.

Example: "How do I implement a function in Java?" in Coding goes to **Prof** in learn mode, unless your
preferences say you want quick implementations.
