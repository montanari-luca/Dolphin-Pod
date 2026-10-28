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
| DP Council | Council. Criticizes big tasks once, on the strongest model, before the specialist works (see [DP Council](#dp-council)) |
| Robert | Git manager. Status, history, commit, fetch, push and pull. The coding agents hand git work to him; push, pull and anything destructive wait for your confirmation |

Every agent can be renamed (one word, so mentions work), recolored with a solid color or gradient, and given its
own provider, model and reasoning effort. Hover any agent name for its job, all-time token use, thinking time and
working time.

Each agent can also have its own **persona** (how it sounds), its own **memory** of what it knows about you, and it
**learns** from your corrections, reverts and thumbs. See [Personality, memory and learning](memory.md).

## Working together

Coding and long-horizon agents (Rick, Micheal, Jordan, Viktor, Ducky) can hand a task to Robert, Amelia or Luna mid-task
with the `delegate_to_agent` tool. The delegate answers in the chat under their own name, with the permissions the caller
has in that chat. Amelia only ever runs when you ask for a check (or @mention her), and she always goes after the agent whose work she checks.
Robert needs a workspace folder in the chat. Luna needs web access turned on.

## DP Council

DP Council reviews other agents' work so the answer is better, not to take over. It speaks as a normal member of the
chat (its thinking is collapsed like everyone's), says what is solid, what is risky or missing and what to do instead,
and then steps out. It never writes files or runs commands.

- **When:** on coding, long-horizon, security, debugging, documentation and research tasks that Roger routes as
  *implement* or *plan*, and when you @mention one of those agents. It skips chat, quick answers, learning, creative
  writing, images, git, verification and scheduled automations.
- **Once per turn:** one critique before the work starts. If several agents work on the same request, it speaks
  once, before the first.
- **Plans:** the planner writes a draft, the council critiques the draft, and the planner writes the final plan.
  Only the final plan has the Approve button; approving it does not call the council again.
- **Model:** pick the strongest model you have for it in Settings → Agents → DP Council. If it has none, it uses
  the strongest model among your other agents (highest reasoning effort, then largest context window). It works
  better the smarter it is, because it sets the bar for everyone else.
- **Switch:** *DP Council reviews big tasks* in Settings turns it off to save time and tokens. Each review is one
  extra call on its model (plans use two extra calls: the draft and the review).
- **Ask it yourself:** `@DP Council` reviews whatever is in the chat.

## Asking you questions

Any agent with a tool-capable model can pause its work and ask you a question with the `ask_user` tool. A panel appears
above the message box with up to four choices the agent wrote, plus an **Other…** field where you type your own answer.
Click a choice (or press `1` to `4`), or press **Skip** to let the agent use its best judgment. The agent waits as long as
it takes; **Stop** dismisses the question and ends the run. Agents don't ask in automations, in runs started from another
device, or when working as a delegate.
