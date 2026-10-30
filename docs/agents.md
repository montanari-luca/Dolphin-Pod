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
| Vera | Software director. Routes the development chats in place of Roger and Lincoln (see [Development chats](#development-chats)) |
| Nolan, Priya | Software engineers. Engineer the software (components, interfaces, risks, test plan, build order) before the coder writes code |
| Mira, Kai | Designers. Each works out one design idea, side by side; you pick one or merge them |
| Tilda | Level designer (games) |
| Jules | 3D artist (games) |
| Sonia | Audio designer (games) |
| Wren | Story designer (games) |
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

## Development chats

Software Development, Web Development and Game Development are chats with Vera, the software director, in them. In a chat
with the director she decides who works on your message in place of Roger and Lincoln (settings, small talk and
general questions still go to Roger). @mentions still go straight to the agent.

- **The team, in order:** designers, then the game specialists (Game Development), then the engineers, then the coder
  (Rick), then the check if you asked for one. Both engineers or neither work on a job, and both designers or neither.
- **Engineers before the coder:** whenever Rick is asked to build something, Nolan and Priya engineer it first, and
  Rick builds from their plan. Quick requests, plans and questions skip them.
- **Review before building:** Vera reviews the engineers' plan once, read-only, before Rick starts. That review
  counts as the turn's review, so DP Council stays out of it. Vera reviews on the model set for Vera, or on the strongest model
  in the pod if none is set.
- **Two design ideas:** Mira and Kai work at the same time and do not see each other's idea. The run then waits on a
  card with both ideas side by side. **Pick A** or **Pick B** hands that idea to the rest of the team; **Merge…**
  takes a note (for example "A's layout with B's colors"), has Mira write one merged design from both, and continues
  with it. Your choice appears in the chat as a message from you.
- **Your own chats:** the new-chat dialog does not preselect the development team. Add Vera to a chat to make it a
  development chat, and the rest of the team along with her.
- **Existing pods** get the three chats and the nine agents once on update. A chat or agent you delete stays deleted.

## Asking you questions

Any agent with a tool-capable model can pause its work and ask you a question with the `ask_user` tool. A panel appears
above the message box with up to four choices the agent wrote, plus an **Other…** field where you type your own answer.
Click a choice (or press `1` to `4`), or press **Skip** to let the agent use its best judgment. The agent waits as long as
it takes; **Stop** dismisses the question and ends the run. Agents don't ask in automations, in runs started from another
device, or when working as a delegate.
