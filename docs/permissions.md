# Permissions and workspaces

Give a chat a **workspace folder** (chat settings) to let agents work with files there. A folder that is far too
broad (`/`, your home folder) or that holds credentials (`~/.ssh`, `~/.aws`, …) is refused.

Each agent has one of three levels in a chat. Click the lock beside its name in the members panel to change it.

- **Read-only** (default, and always while planning): read, list and search files.
- **Ask first**: may also write and edit files and run commands, but each one waits for your approval. The
  question shows the exact command or file. Answer **Allow once**, **Deny**, **Allow file changes for this run**, or,
  for a plain command, **Always allow** `` `cargo test` `` (a command prefix kept in this chat's settings). Automations,
  runs started from another device and delegates cannot ask you, so their writes and commands are refused at this level.
- **Full agentic**: writes, edits and commands run without asking, only inside the workspace.

A plan is approved with the **Approve plan** button or by replying with an approval; Lincoln then grants the
planner full permissions and the implementation starts. Every change is announced in the chat, every action is listed
in the **Action log**, and permissions are enforced by the app itself, not by prompt instructions.

## Commands

Commands run in an OS sandbox on macOS (`sandbox-exec`) and on Linux with bubblewrap: writes only inside the workspace
and temp/cache folders, credential folders hidden (macOS). There is no sandbox on Windows, or on Linux without
`bwrap`; the command output says so.

**Commands have no network access by default** (macOS: outbound connections and DNS denied, loopback still works;
Linux: an empty network namespace). That stops a prompt injected through a web page or file from sending your
files out. Turn on **Commands can use the network** in the chat settings for installs and downloads. Without a sandbox
(Windows) the network cannot be blocked.

Pre-approved commands only match a single plain command: anything with `;`, `&`, `|`, `<`, `>`, `$`, backticks,
parentheses or a backslash is always asked about, so `npm test; rm -rf ~` never rides in on `npm test`.

Agents cannot write inside any `.git` folder; git work goes through the git tool (Robert).

## Undoing a run

Before an agent's first file change, command or git write in a run, Dolphin Pod snapshots the workspace; when the run
ends it snapshots again. The agent's message then shows **Changed N files** with the file list, the full diff on
request, and a **Revert** button.

- Reverting puts back only the files that run changed. A file you (or another agent) edited again afterwards is left
  alone and listed. The state just before a revert is snapshotted too, so nothing is lost by pressing it.
- Snapshots live in a separate git repository in the app's data folder, never in your project, and your project's own
  `.git` is not touched. Your `.gitignore` is honoured, and `node_modules`, `target` and virtualenvs are always left out.
  A nested repository inside the workspace is recorded as a whole, not file by file.
- This needs `git` installed. Without it, runs still work and the message says undo is unavailable.
- Snapshots stay on the device that made them. Reverting a run that was synced from another device is refused with a
  clear message. Deleting a chat deletes its snapshots.

## Web pages and files that talk to the agent

Text an agent reads can contain instructions aimed at it (a hidden line on a web page, a comment in a downloaded file).
Two things limit what that can do:

- **Untrusted content.** Web pages, search results, files, command output and git output reach the model wrapped in
  `<untrusted_content>` tags, and every agent's instructions say to treat that text purely as data: never follow it,
  and tell you what it asked instead. Invisible characters (zero-width, bidirectional overrides, Unicode "tag"
  characters) are removed from it, text a page hides from people (`hidden`, `display:none`, zero-size text, comments)
  is dropped before the model sees it, and content cannot close the wrapper early.
- **Ask after reading the web.** Once a run has opened a web page or read search results, every file change, command
  and git write in that run asks you first, even at the full level, and nothing counts as already approved (no
  pre-approved commands, no "allow file changes for this run"). The question says the agent has read web content and
  offers only **Allow once** and **Deny**. Automations, delegated and remote runs cannot ask, so they refuse these
  actions. What a delegate read on the web taints the run that delegated to it. Turn this off with **Ask before acting
  after reading the web** in Settings → General (changing it through Roger needs your confirmation).

Limits: this is a mitigation, not a guarantee. Models can still be fooled by text that is not marked as untrusted
(for example OpenRouter's server-side search results, which arrive inside the model's own answer and do not trigger the
ask rule), files in your project count as trusted for the ask rule (otherwise every run would ask), and an agent that
read a secret could still try to send it out in the address of a page it opens. Keep web access off in chats where
that matters.
