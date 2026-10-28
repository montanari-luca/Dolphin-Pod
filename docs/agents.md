# Agents

| Agent | Role |
| --- | --- |
| Roger | Moderator. Answers general questions, routes requests, checks on stuck agents, changes settings on request. |
| Lincoln | Orchestrator. Never talks to you: turns Roger's decisions into briefs and enforces permissions. |
| Rick | Coding |
| Micheal | Long-horizon projects |
| Viktor | Security and cyber |
| Jordan | Debugging |
| Emma | Creative writing |
| Ducky | Documentation |
| Sarah | Images (a reasoning model writes the prompt, a separate image model renders it) |
| Prof | Teaching |
| Ruby | Fun facts, when you ask for one |
| Amelia | Verification. Double-checks other agents' work with evidence, only when you explicitly ask ("do this, then double-check it") |
| Luna | Research. Many parallel web searches, source validation, fact and timestamp checks, then a cited report |
| Robert | Git manager. Status, history, commit, fetch, push and pull. The coding agents hand git work to him; push, pull and anything destructive wait for your confirmation |

Every agent can be renamed (one word, so mentions work), recolored with a solid color or gradient, and given its
own provider, model and reasoning effort. Hover any agent name for its job, all-time token use, thinking time and
working time.

Each agent can also have a **persona**: a few lines (up to 1,500 characters) describing its voice and character, such
as "dry and to the point, calls me chief". Set it in Settings → Agents, or ask Roger to change it. The persona is added
on top of what the agent is built to do, so it changes how the agent sounds, never what it is allowed to do, and it
stays with that agent only. It syncs between your devices with the rest of the agent.

## Memory

Each agent has its own memory: short notes about you that it carries into every request it handles. Open
Settings → Agents, pick an agent and use **Memory** to add, reword, pin or forget entries; changes save as you go.

Every entry has a **kind**, so the agent can tell how to use it:

- **Style**: how it should sound ("terse, no emojis", "British English").
- **Fact**: something true about you or your setup ("the database is PostgreSQL").
- **Workflow**: how you like work done ("run the tests before committing").
- **Avoid**: something it should not do again.

An entry applies **everywhere**, or only in one project, meaning a workspace folder (for example "Only in dolphin-pod"),
so a habit for one repository doesn't leak into another. Style and Avoid entries, and anything you pin, are always
sent. Facts and workflows are all sent while an agent has only a few; once it has many, only the ones that match
your request come along, so small local models don't carry notes they don't need. Memory is sent after the chat
history, never as part of the agent's instructions.

An agent only ever sees its own memory, never another agent's, and an agent can hold up to 40 entries of up to 300
characters each. Memory syncs between your devices with the rest of the agent. "Forget everything" empties an
agent's memory for good. Your global preferences (Settings → Preferences) are separate and still apply to every agent.

## Working together

Coding and long-horizon agents (Rick, Micheal, Jordan, Viktor, Ducky) can hand a task to Robert, Amelia or Luna mid-task
with the `delegate_to_agent` tool. The delegate answers in the chat under their own name, with the permissions the caller
has in that chat. Amelia only ever runs when you ask for a check (or @mention her), and she always goes after the agent whose work she checks.
Robert needs a workspace folder in the chat. Luna needs web access turned on.

## Asking you questions

Any agent with a tool-capable model can pause its work and ask you a question with the `ask_user` tool. A panel appears
above the message box with up to four choices the agent wrote, plus an **Other…** field where you type your own answer.
Click a choice (or press `1` to `4`), or press **Skip** to let the agent use its best judgment. The agent waits as long as
it takes; **Stop** dismisses the question and ends the run. Agents don't ask in automations, in runs started from another
device, or when working as a delegate.
