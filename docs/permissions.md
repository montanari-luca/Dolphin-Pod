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
