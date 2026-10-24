# ocmux - Opencode Multiplexer

A terminal multiplexer for managing [OpenCode](https://opencode.ai) sessions.

<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/93669f30-bb6c-4a71-935d-29541ee073ad" width="80%" controls></video>
</p>

## Feature Showcase

<details>
<summary><b>Diff View</b></summary>
<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/70cc5f29-118c-4d96-b611-a7192d246205" width="80%" controls></video>
</p>
</details>

<details>
<summary><b>Visual Mode</b></summary>
<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/ac156ecc-e8b8-47ee-8bd0-7a0815b5b836" width="80%" controls></video>
</p>
</details>

<details>
<summary><b>Sidebar Layouts</b></summary>
<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/2b2b4c2e-0739-4009-99bb-62e9de0d67f7" width="80%" controls></video>
</p>
</details>

<details>
<summary><b>Mermaid Diagrams</b></summary>
<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/927fb400-567d-4e36-a17f-64fddd23f286" width="80%" controls></video>
</p>
</details>

## Table of Contents
- [Install](#install)
- [Usage](#usage)
- [Keybindings](#keybindings)
  - [Sidebar navigation](#sidebar-navigation)
  - [Session actions](#session-actions)
  - [Session picker](#session-picker)
  - [Message history picker](#message-history-picker)
  - [Conversation view](#conversation-view)
  - [Diff view](#diff-view)
  - [Agents view](#agents-view)
  - [Visual mode](#visual-mode)
  - [Shell mode](#shell-mode)
  - [Sidebar layout](#sidebar-layout)
- [Advanced workflows](#advanced-workflows)
- [Playbooks](#playbooks)
- [System behaviors](#system-behaviors)
- [Configuration](#configuration)
- [Notes](#notes)
- [License](#license)

## Install

**Cargo:**

```
cargo install opencode-multiplexer
```

**Homebrew:**

```
brew tap joeyism/ocmux
brew install ocmux
```

**Shell (macOS / Linux):**

```
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/joeyism/ocmux-rs/releases/latest/download/ocmux-rs-installer.sh | sh
```

### Optional: Mermaid diagrams in conversation view

Conversation view can render fenced ` ```mermaid ` blocks as images. This requires the [Mermaid CLI](https://github.com/mermaid-js/mermaid-cli) (`mmdc`) on your `PATH`:

```
npm install -g @mermaid-js/mermaid-cli
```

Pixel graphics work best in Kitty (and other terminals that speak the Kitty graphics protocol). Without `mmdc`, mermaid fences fall back to normal syntax-highlighted code blocks.

## Usage

Run `ocmux` in your terminal. The sidebar lists active OpenCode sessions sorted by most recently updated. The main pane shows the attached session's terminal output.

- Press `Enter` to attach to a top-level session
- Press `Tab` to expand/collapse child sessions
- Press `v` to open a read-only conversation view (see below)
- Press `s` to open the session manager
- Click a sidebar row to select it

## Keybindings

### Sidebar navigation

| Key      | Action                    |
|----------|---------------------------|
| `j` / `Down` | Move down             |
| `k` / `Up`   | Move up               |
| `Ctrl-Alt-j` | Cycle next session (keep focus) |
| `Ctrl-Alt-k` | Cycle prev session (keep focus) |
| `Enter`  | Attach to session        |
| `Tab`    | Expand/collapse children |
| `s`      | Open session manager     |
| `Ctrl-h` | Hide/show sidebar panel  |
| `/`      | Open session picker     |
| `r`      | Refresh active session   |
| `?`      | Show help overlay        |
| `q`      | Quit (confirm with `y`) |
| `Ctrl-4` | Toggle focus sidebar/main |

### Session actions

| Key | Action                                |
|-----|---------------------------------------|
| `n` | Spawn new session                     |
| `t` | Create git worktree + spawn           |
| `m` | Move selected session to another repo |
| `a` | Open Agents relationship view         |
| `v` | Open read-only conversation view       |
| `d` | Open diff view for session             |
| `f` | Show files modified by session        |
| `!` | Drop into shell in session directory   |
| `c` | Commit/push modified files            |
| `h` | Open message history picker           |
| `p` | Open playbook picker (or abort a running playbook) |
| `s` | Open session manager (delete junk)    |
| `x` | Kill session and stop its serve when it is the last user (`y` confirm, `n`/`Esc` cancel) |

### Status icons

| Icon | Meaning |
|------|---------|
| `●` (green) | Agent is working (including normal tool startup) |
| `◐` (yellow) | Session is blocked waiting for your input or tool permission |
| `●` (cyan) | Subagents are active |
| `✖` (red) | Session encountered an error |
| `○` (gray) | Session is idle |

Quitting with `q` and confirming stops all `opencode serve` daemons managed by ocmux. Pressing `x` stops the selected session's serve when no other sidebar session shares that backend; shared serves remain alive until their last user is removed.

### Session picker

Press `/` to search and attach to any opencode session. The picker uses **fuzzy search** across repo, title, and directory fields. Live sessions (currently running) are marked with a green dot (`●`).

Attaching starts or reuses an `opencode serve` for the session's exact working directory, then connects the TUI with `attach --session`. This gives attached sessions the same HTTP backend used by new sessions and playbooks.

| Key      | Action                    |
|----------|---------------------------|
| `↑` / `↓` | Move through list       |
| `Enter`  | Attach to selected        |
| `Backspace` | Delete last character |
| any key  | Filter by fuzzy match     |
| `Esc`    | Cancel picker            |

The footer shows `matched/total` counts. Results are sorted by live status first, then fuzzy match score, then recency.

### Session manager

Press `s` to manage your opencode sessions and delete junk that pollutes your message history. You can fuzzy search, select multiple sessions, and hard-delete them (cascading to all their messages, parts, and subagent children).

| Key      | Action                    |
|----------|---------------------------|
| `↑` / `↓` | Move through list       |
| `Tab`    | Toggle selection         |
| `Ctrl-a` | Select all matched       |
| `Ctrl-u` | Clear selection          |
| `Ctrl-d` | Delete selected (or current) |
| `y`      | Confirm hard-delete      |
| `Backspace` | Delete last character |
| any key  | Filter by fuzzy match     |
| `Esc`    | Cancel / close           |

> [!WARNING]
> Deletion is permanent and writes directly to the OpenCode database. It is recommended to back up `~/.local/share/opencode/opencode.db` before bulk cleanup.

### Message history picker

Press `h` to search past user messages and paste one into the active terminal session. The picker uses **fuzzy search** across session title and message text. The top table shows session name and message preview; selecting a row shows the full message below.

| Key      | Action                    |
|----------|---------------------------|
| `↑` / `↓` | Move through list       |
| `Enter`  | Paste selected message    |
| `Backspace` | Delete last character |
| any key  | Filter by fuzzy match     |
| `Esc`    | Cancel picker            |

The footer shows `matched/total` counts. Results are sorted by fuzzy match score, then recency.

### Conversation view

<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/927fb400-567d-4e36-a17f-64fddd23f286" width="80%" controls></video>
</p>

Press `v` from the sidebar to open a read-only view of the session's conversation history. The view polls the opencode database every second and renders messages, markdown, syntax-highlighted code blocks, and tool call status.

Turns are visually separated with a role-colored left gutter (`│`). User turns use cyan (`YOU`); assistant turns use green (agent name). Reasoning is dim italic; tool calls are indented and muted under the gutter.

**Mermaid diagrams:** Fenced ` ```mermaid ` blocks are rendered as images when `mmdc` is installed (see [Optional: Mermaid diagrams](#optional-mermaid-diagrams-in-conversation-view)). In Kitty and compatible terminals, diagrams use pixel graphics and scroll with the conversation; otherwise they fall back to half-block rendering. Without `mmdc`, the fence is shown as a normal code block.

| Key      | Action                    |
|----------|---------------------------|
| `j` / `Down` | Scroll down           |
| `k` / `Up`   | Scroll up             |
| `G`      | Jump to end               |
| `g`      | Jump to top               |
| `Ctrl-u` | Page up                   |
| `Ctrl-d` | Page down                 |
| `/`      | Search conversation       |
| `n`      | Next search match         |
| `N`      | Previous search match     |
| `J`      | Jump to next user message |
| `K`      | Jump to previous user message |
| `z`      | Enlarge mermaid diagram (lightbox) |
| `q` / `v` / `Esc` | Close view       |

**Auto-follow:** By default, the view stays at the bottom and scrolls automatically as new messages arrive. Scrolling up manually disengages follow mode. Jump to end (`G`) or page down at the bottom (`Ctrl-d`) to resume following.

**Search:** Search is case-insensitive and incremental — type to filter, `Enter` to confirm. The search bar shows current match position (`1/5`). Pressing `/` again or `Esc` cancels.

**Selecting text:** Drag with the mouse to select; releasing copies the selection to the clipboard. Selection copies readable text — soft-wrapped lines are joined back together, real line breaks and code indentation are kept, and gutters, timestamps, separator rules, Markdown syntax, and Mermaid images are left out. Dragging into the top or bottom edge keeps scrolling while the pointer is held there, so long passages can be selected without flicking. `Esc` clears the selection. This is handled by ocmux rather than the terminal's own selection, so it behaves the same in Kitty, Ghostty, WezTerm, and other modern terminals.

Mouse scroll is supported.

### Diff view
<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/70cc5f29-118c-4d96-b611-a7192d246205" width="80%" controls></video>
</p>

Press `d` from the sidebar to open a read-only view of the session's git diff. The view shows both tracked and untracked changes, preferring the opencode serve API when available and falling back to `git diff` otherwise.

The diff view uses a **cursor-based** navigation model (distinct from the scroll-based conversation view). The cursor determines the position for visual selection. Searching (`/`, `n`, `N`) moves the cursor to the match line, and mouse scrolling keeps the cursor within the visible viewport.

| Key      | Action                    |
|----------|---------------------------|
| `j` / `Down` | Move cursor down      |
| `k` / `Up`   | Move cursor up        |
| `G`      | Jump to end               |
| `g`      | Jump to top               |
| `Ctrl-u` | Page up                   |
| `Ctrl-d` | Page down                 |
| `/`      | Search diff               |
| `n`      | Next search match         |
| `N`      | Previous search match     |
| `v`      | Toggle visual selection   |
| `Enter`  | Confirm selection & paste |
| `Esc`    | Cancel selection / close  |
| `q` / `d`| Close view                |

**Search:** Case-insensitive, incremental. The search bar shows current/total matches. Jumping to matches moves the cursor to the match line. The `/` key is disabled while visual mode is active — cancel visual mode first to search.

Mouse scroll is supported and keeps the cursor within the viewport (same behavior as `Ctrl-y` / `Ctrl-e`).

### Agents view

Press `a` from the sidebar to inspect the selected session's subagent relationships. The view shows a live session tree by default. When the session is the parent of a [parallel-builds](https://github.com/joeyism/opencode-parallel-builds) run, ocmux automatically loads its `plan.json` and `runs.db` to show dependency waves, task status, and linked worker sessions.

Select a node with `j`/`k`. Press `Enter` to attach to a linked worker, `v` to open its conversation, or `d` to inspect its diff. Press `a`, `q`, or `Esc` to return.

### Visual mode
<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/ac156ecc-e8b8-47ee-8bd0-7a0815b5b836" width="80%" controls></video>
</p>

Visual mode lets you select lines from the diff view and paste them as file references into the active terminal session.

Press `v` from the diff view to enter visual mode. The current cursor line is marked as the anchor. Use `j`/`k` to move the cursor and highlight lines. Press `Enter` to confirm — this closes the diff, returns to the terminal, and pastes the selected lines into the chatbox.

**Selection formatting:** Lines are grouped by file and formatted as `filepath:line` or `filepath:start-end` (e.g., `foo.rs:42-58`). Deleted files (`/dev/null`) are skipped. If the selection spans multiple files, each gets its own reference separated by spaces.

| Key | Action |
|-----|--------|
| `v` | Toggle visual mode on/off |
| `j` / `k` | Move cursor to expand/contract selection |
| `Enter` | Confirm and paste into terminal |
| `Esc` | Cancel visual mode (stays in diff view) |

- Search (`/`) is disabled while visual mode is active
- If no valid file references are in the selection, nothing is pasted

### Shell mode

Press `!` from the sidebar to drop into a shell in the selected session's working directory. The shell inherits the session's environment variables. Exit the shell normally (e.g., `exit` or `Ctrl-D`) to return to ocmux.

This works on **top-level sessions only** — child sessions do not support shell drop.

### Sidebar layout

<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/2b2b4c2e-0739-4009-99bb-62e9de0d67f7" width="80%" controls></video>
</p>

The sidebar has two states:

### Sidebar and Panel

The sidebar shows your active and discovered opencode sessions. You can hide the panel entirely for more terminal space.

| State | Key | Behavior |
|-------|-----|----------|
| Visible | (default) | Full-width sidebar showing session tree, title, and status |
| Hidden | `Ctrl-h` | Sidebar disappears entirely for maximum terminal space |

- Toggle hidden with `Ctrl-h`
- When the sidebar is hidden, `Ctrl-4` (focus toggle) will first unhide it before switching focus
- The sidebar width is configurable via `sidebar_width` in config (default: 30)

## Advanced workflows

- **Inspect conversation output** — press `v` on any session (including child sessions) to watch the agent's progress in real-time without attaching to the PTY. Mermaid diagrams render as images when `mmdc` is available (see [Optional: Mermaid diagrams](#optional-mermaid-diagrams-in-conversation-view)).
- **Inspect changed files** — press `d` to open a diff view of all changes made by a session (tracked and untracked)
- **Inspect changed files (list)** — press `f` to see which files a session has created or modified. Press any key or `Esc` to close.
- **Drop into a shell** — press `!` to open a shell in the selected session's working directory. The shell inherits the session's environment; exiting the shell returns to ocmux.
- **Commit session changes** — press `c` to prompted for a commit message, then commit and push all modified files immediately.
- **Search and attach** — press `/` to open the session picker, search across all opencode sessions, and attach to one.
- **Reuse an earlier prompt** — press `h` to open the message history picker, search past user messages, and paste one into the active terminal session.
- **Create a worktree** — press `t` to pick a repo directory, then enter a branch name (leave empty to spawn in the repo root without a worktree). A new worktree is created and a session is spawned in it.
- **Move a session** — press `m` on a session to move it and its child sessions to another repo. The session must be idle; uncommitted files stay in the source directory.
- **Spawn a session** — press `n` to pick a repo from discovered git repositories (see Configuration for search paths and depth), then spawn a new managed session in that directory.
- **Run a playbook** — press `p` to automate prompts, shell checks, and workflow control against the selected session.

## Playbooks

Press `p` to open the playbook picker. Playbooks are YAML files in `~/.config/ocmux/playbooks/` that automate multi-agent workflows: send prompts to specific agents, run shell commands, and loop or branch based on results.

The selected session does not need to have a serve already. `p` starts or reuses one for the exact working directory and reconnects the active TUI when necessary. Playbook sends use the OpenCode serve HTTP API, not PTY keystrokes.

Playbooks are tracked per session, so different sessions can run one playbook each. Stopping an agent turn with OpenCode's `Esc` action aborts that session's playbook after the interrupted turn is detected. If an agent turn fails (for example a remote provider error), the playbook stops and reports the failure instead of running later steps.

The picker shows the current step in the footer. Press `p` again while a playbook is running to request an abort; confirm with `y` or `Enter`, or cancel with `n` or `Esc`.

The parameter form supports:

| Key | Action |
|-----|--------|
| `Tab` | Move between fields |
| `Enter` | Start the playbook |
| `Esc` | Cancel |
| Paste | Insert text into the focused field |

### Included playbooks

Copy these files into `~/.config/ocmux/playbooks/`.

#### `pr-comments.yaml`

Counts inline review and issue comments. If comments exist, it sends the work through chat, planning, and implementation agents once.

```yaml
name: pr-comments
description: Review, plan, fix, and resolve GitHub PR comments
params:
  GITHUB_PR_URL:
    prompt: GitHub PR URL
    required: true
steps:
  - run: |
      set -euo pipefail
      url="$GITHUB_PR_URL"
      path="${url#https://github.com/}"
      owner="${path%%/*}"
      rest="${path#*/}"
      repo="${rest%%/*}"
      num="${rest##*/pull/}"
      num="${num%%/*}"
      num="${num%%\?*}"
      review=$(gh api "repos/${owner}/${repo}/pulls/${num}/comments" --jq 'length')
      issue=$(gh api "repos/${owner}/${repo}/issues/${num}/comments" --jq 'length')
      echo $((review + issue))
    register: COMMENTS
  - if: test "$COMMENTS" != "0"
    then:
      - send:
          agent: chat
          message: |
            review comments from ${GITHUB_PR_URL}
            don't fix anything, just summarize it and tell me what they mean, and are they worth fixing
      - send:
          agent: plan
          message: make a plan to fix this
      - send:
          agent: build
          message: Proceed with implementation
```

#### `pr-comments-auto.yaml`

Repeats the review, plan, implementation, and resolution workflow until no comments remain or five iterations have run.

```yaml
name: auto pr-comments
description: Review, plan, fix, and resolve GitHub PR comments
params:
  GITHUB_PR_URL:
    prompt: GitHub PR URL
    required: true
steps:
  - run: |
      set -euo pipefail
      url="$GITHUB_PR_URL"
      path="${url#https://github.com/}"
      owner="${path%%/*}"
      rest="${path#*/}"
      repo="${rest%%/*}"
      num="${rest##*/pull/}"
      num="${num%%/*}"
      num="${num%%\?*}"
      review=$(gh api "repos/${owner}/${repo}/pulls/${num}/comments" --jq 'length')
      issue=$(gh api "repos/${owner}/${repo}/issues/${num}/comments" --jq 'length')
      echo $((review + issue))
    register: COMMENTS
  - until: test "$COMMENTS" = "0"
    max: 5
    steps:
      - send:
          agent: chat
          message: |
            review comments from ${GITHUB_PR_URL}
            don't fix anything, just summarize it and tell me what they mean, and are they worth fixing
      - send:
          agent: plan
          message: make a plan to fix this
      - send:
          agent: build
          message: Proceed with implementation
      - send:
          agent: orchestrator
          message: reply in thread and resolve comments
      - run: |
          set -euo pipefail
          url="$GITHUB_PR_URL"
          path="${url#https://github.com/}"
          owner="${path%%/*}"
          rest="${path#*/}"
          repo="${rest%%/*}"
          num="${rest##*/pull/}"
          num="${num%%/*}"
          num="${num%%\?*}"
          review=$(gh api "repos/${owner}/${repo}/pulls/${num}/comments" --jq 'length')
          issue=$(gh api "repos/${owner}/${repo}/issues/${num}/comments" --jq 'length')
          echo $((review + issue))
        register: COMMENTS
```

#### `pr-senior-review.yaml`

Asks the chat agent for a senior engineering review and requests only high-severity findings.

```yaml
name: pr-senior-review
description: Senior eng review of a PR, high severity and above
params:
  GITHUB_URL:
    prompt: GitHub PR URL
    required: true
steps:
  - send:
      agent: chat
      message: |
        do a senior eng review of the PR ${GITHUB_URL}

        You are in the same branch, but you are behind, so pull upstream to get the latest code. Show me all review comments you have, then filter to only show high severity or above
```

`${VAR}` is interpolated in `message:` strings. Shell steps use environment variables (`"$VAR"`). `until` is bash-like (check first); `if`/`then`/`else` branches on exit code. See `skills/ocmux-playbook/SKILL.md` for the full schema and an agent skill that can generate playbooks for you.

Copy the skill so agents can author playbooks:

```bash
mkdir -p ~/.config/opencode/skills/ocmux-playbook
cp skills/ocmux-playbook/SKILL.md ~/.config/opencode/skills/ocmux-playbook/
```

## System behaviors

**Focus tracking** — When the ocmux window loses OS focus, the border dims to dark gray. When focus returns, the border resumes normal styling.

**Sidebar sync** — When focus is on the terminal (attached session), the sidebar selection automatically tracks the active session. If the attached session exits, focus returns to the sidebar with a "session exited" footer message.

**Notifications** — When `notifications: true` in config, ocmux sends desktop alerts on specific session transitions: `Working → Idle`, `Working → NeedsInput`, `Working → Error`. Each session has a 5-second cooldown between notifications. On macOS, `notify-rust` is used (which respects Do Not Disturb / Focus modes). On Linux, `notify-send` is used as a fallback. Notifications are suppressed while ocmux is the focused window.

**Terminal features** — ocmux supports bracketed paste (safe paste of multi-line content), full special-key forwarding (arrows, Home, End, PageUp, PageDown, F-keys, etc.), and proper terminal resize on window changes. Mouse wheel scroll and clicks are forwarded to the active terminal session (SGR), while diff and conversation views scroll locally. Left-drag still selects text for clipboard copy; a click without drag is delivered to the app.

## Configuration

Keybindings, sidebar width, desktop notifications, and repo search depth are configurable via `~/.config/ocmux/config.json`:

```json
{
  "sidebar_width": 30,
  "notifications": true,
  "follow_selection": false,
  "spawn_maxdepth": 5,
  "keybindings": {
    "up": "k",
    "down": "j",
    "spawn": "n",
    "kill": "x",
    "help": "?",
    "worktree": "t",
    "move_session": "m",
    "quit": "q",
    "view": "v",
    "files": "f",
    "diff": "d",
    "history": "h",
    "sessions": "s",
    "agents": "a",
    "playbook": "p",
    "session_next": "ctrl-alt-j",
    "session_prev": "ctrl-alt-k"
  }
}
```

Keybinding values are single characters. `session_next` and `session_prev` are chord strings (`ctrl-alt-j`), not single characters. Default `sidebar_width` is `30`. Only the keys listed above are configurable; other bindings (`Enter`, `Tab`, `/`, `s`, `Ctrl-h`, `r`, `!`, `c`, `Ctrl-4`, arrows) are fixed.

`notifications` controls desktop alerts. Defaults to `true`.

`follow_selection` makes sidebar `j`/`k`/arrows/click auto-attach the landed-on top-level session without stealing focus. Defaults to `false`.

`spawn_maxdepth` controls how deep `find` searches for git repos when spawning or creating worktrees. It searches `~/Programming`, `~/repos`, `~/projects`, and `~/code` (falling back to `~` if none exist). Defaults to `5`.

## Notes

- `Ctrl-4` is the actual focus toggle binding (hold `Ctrl` and press `4`)
- `s` opens the session manager; `Ctrl-h` hides the sidebar entirely for maximum terminal space
- `q` prompts for confirmation before quitting (`y` confirm, `n`/`Esc` cancel)
- `Ctrl-Alt-j` / `Ctrl-Alt-k` cycle top-level sessions without changing focus; cycle order is current sidebar recency order; child rows are skipped
- Child sessions are expandable and selectable in the sidebar, but `Enter` attach is not yet supported for child rows — use `v` to view their conversation instead
- `c` and `!` operate on top-level sessions only
- `r` refreshes the currently active session's PTY, not the selected sidebar row
- `/` and `p` start or reuse a serve for an attached session when one is missing; the first `p` may reconnect the active TUI once
- Killing a managed session (`x`) also terminates its associated opencode serve daemon
- Deleting a session via the session manager (`s`) is permanent and hard-removes all associated messages and parts from the database

## License

Apache-2.0
