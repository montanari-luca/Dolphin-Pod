# Personality, memory and learning

Every agent is its own character. You can give it a **persona**, it keeps its own **memory** of what it knows about you,
and it **learns** from how you work with it: what you correct, undo, decline and rate. Nothing an agent learns is
shared with another agent unless you choose to share it, so Rick can be terse while Prof stays patient.

Open **Settings → Agents**, pick an agent, and you will find the persona, the learning switch and the memory list.

## Persona

A persona is a few lines (up to 1,500 characters) about how an agent sounds and behaves: tone, quirks, how it
addresses you. For example: "Dry and to the point. Skips the pleasantries and calls me chief."

An agent never rewrites its own persona. If you want it to *suggest* changes, turn on **Let it suggest persona changes**
in its Memory section (and press Save). When what you have shown it, repeatedly, points to a different tone, it proposes
a rewritten persona with a one-line reason. A card appears above the persona field and a toast tells you; **Accept**
makes it the persona, **Dismiss** throws it away, and a text you dismissed is not suggested again. Only one suggestion
waits at a time, and it needs learning to be on.

The persona is added on top of what the agent is built to do. It changes how the agent sounds, never what it is
allowed to do, so it can't remove a skill or a safety rule. You can also ask Roger to change it in chat. It syncs
between your devices with the rest of the agent.

## Memory

Memory is a list of short notes about you that an agent carries into every request it handles. Each note is one
thought, up to 300 characters, and an agent holds up to 40.

A note has a **kind**, so the agent knows how to use it:

| Kind | What it is | Example |
| --- | --- | --- |
| Style | How it should sound or present | "Keep answers under three sentences" |
| Fact | Something true about you or your setup | "The database is PostgreSQL" |
| Workflow | How you like work done | "Run the tests before committing" |
| Avoid | Something it should not do again | "Never rewrite a whole file" |

It also has a **scope**: **Everywhere**, or **Only in** one project, meaning a workspace folder such as `dolphin-pod`.
A habit for one repository then doesn't leak into another. Projects are matched by folder name, so a note follows
you to another device even though folder paths differ there.

In the list you can add a note, reword it, change its kind or scope, **pin** it, or forget it. Changes save as you
go. **Forget everything** empties that agent's memory for good.

### What an agent is sent

Style and Avoid notes, and anything you pin, are always sent. Facts and workflows are all sent while an agent has few
of them; once it has many, only the ones that share words with your request come along. That keeps small local models
from carrying notes they don't need, and it costs no extra model call.

Notes arrive after the chat history, not as part of the agent's instructions, so they never break the provider's prompt
cache. An agent is only sent its own notes, plus any that you shared with it (below). Your global **Preferences**
(Settings → Preferences) are separate and still apply to every agent.

### Picking by meaning (optional)

By default, once an agent has many notes, the ones sent with a request are the ones that share words with it. That misses
a note about "PostgreSQL" when you ask about a "slow sql query". If you set an **Embedding model** (Settings → General →
Memory), Dolphin Pod compares the *meaning* of your request with each note instead, and sends the closest few.

It needs a model that offers embeddings, on Ollama, LM Studio, OpenRouter or another OpenAI-compatible server; a chat
model usually can't (Ollama answers "this model does not support embeddings"). On Ollama, `nomic-embed-text` is a small
choice. Press **Test** after picking one to see whether it works. Nothing is chosen or downloaded for you, and the setting
is empty by default.

Each note is embedded once per model and the vector is kept on this device (it is rebuilt if you reword the note). Only the
text of your request and of notes not seen before is sent to the embedding model, in one call. An agent with only a few
notes still gets all of them and never calls it. If the model is down, too slow (over four seconds) or answers oddly, the
request carries on with word matching, with no error.

### Sharing a note with other agents

Some things are true for every agent: "keep answers under three sentences", "I work in Rust". Rather than teach each one,
use the share menu on a note: **Private** (the default), **Shared with all agents**, or **Chosen agents…** to pick which.

The note stays in its own agent's memory, where you edit, pin or take it back; the other agents are *sent* it along with
their own notes, under the same rules (project scope, and relevance once there are many). It shows in their Memory
section under **From other agents**, read-only, and doesn't count toward their 40. If an agent already has a note with the
same text, it is sent once. A note you share counts as yours, so its agent's learner won't reword or remove it.

## Learning

An agent adds to its own memory as you work with it. It notices when you:

- **correct it** right after an answer ("no, use tabs", "don't add comments", "too long");
- **undo its changes** with the revert button on a run;
- **decline an action** it asked to take, by answering Deny to an approval;
- **give a thumbs down** (or up) to one of its answers;
- **ask it to remember** something: "remember that I use pnpm", "from now on answer in French".

Whoever answered a "remember that…" message keeps it, exactly as you said it if nothing better can be made of it. That
works even if learning is off. Agents whose model supports tools can also save a note themselves with a `remember`
tool, for things you tell them that will matter next time.

Dismissing an approval question without answering is not a "no", and a thumbs up on its own never starts anything: it
is kept as context for the next real signal.

### How a lesson is made

None of this delays an answer. A short call to your **utility model** (Settings → General; Roger's model if you haven't
picked one) runs afterwards, in the background, and only when something happened that could teach a lesson. It is
asked for brief, lasting notes. One-off task details, code, and anything that looks like a secret (passwords, keys,
tokens, card numbers) are never saved, and a note that repeats an existing one is skipped.

Notes an agent writes itself are marked **learned**. When an agent learns something, a toast tells you what, with an
**Undo** button: a new note is forgotten, and a reworded note gets its old text back.

### Learning from finished conversations

Corrections catch what you say out loud. Some preferences you only show: asking for the same format three times
without ever saying "always". With **Learn from finished conversations** (Settings → General) on, a chat that has been
quiet for ten minutes is looked back over by the agents that took part (the three most active at most). They are shown
the last 24 messages and asked for lasting preferences only; a request for one task is not a preference.

This is **off by default**, because it sends a transcript of the conversation (up to 24 messages, each cut to 400
characters) to your utility model, where the signals above send only a few excerpts. Only conversations after you turn it
on are looked at, and each message is looked at once. What an agent learns this way is treated like everything else it
learns: marked **learned**, shown in a toast with **Undo**, and never able to change a note you wrote.

### You stay in charge

- A learner can never change a persona, only suggest one for you to accept.
- A learner can reword or remove only the notes **it wrote itself**. Never the ones you added, edited or pinned.
- When an agent reaches 40 notes, its least-used learned note expires first.
- To stop an agent learning on its own, turn off **Learn from my corrections** in its Memory section and press Save,
  or tell Roger. With it off, nothing is recorded and the `remember` tool is not offered. "Remember that…" and the
  memory list keep working.
- Only the agent you were talking to learns. The others are untouched, unless a note you shared reaches them.

### Thumbs

Every finished answer has a thumbs up and a thumbs down in its footer. Click one to rate the answer, click it again to
take the rating back. A rated answer keeps its thumb visible. The rating is saved with the message, and when the agent
has learning on it is one more signal for it.

### Privacy

The learner reads a few short excerpts of the exchange: your reply, the answer it was about, and your earlier request.
If your utility model is a cloud model, those excerpts are sent to it. Pick a local model there if you don't want that.
Signals are kept on the device where they happened, for up to two weeks, and are not synced; the notes themselves are,
like the rest of an agent.
