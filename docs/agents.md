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

## Working together

Coding and long-horizon agents (Rick, Micheal, Jordan, Viktor, Ducky) can hand a task to Robert, Amelia or Luna mid-task
with the `delegate_to_agent` tool. The delegate answers in the chat under their own name, with the permissions the caller
has in that chat. Amelia only ever runs when you ask for a check (or @mention her), and she always goes after the agent whose work she checks.
Robert needs a workspace folder in the chat. Luna needs web access turned on.
