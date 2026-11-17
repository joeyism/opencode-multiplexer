# ocmux - Opencode Multiplexer

A terminal multiplexer for managing [OpenCode](https://opencode.ai) sessions.

https://github.com/user-attachments/assets/93669f30-bb6c-4a71-935d-29541ee073ad

## Feature Showcase

<details>
<summary><b>Diff View</b></summary>

https://github.com/user-attachments/assets/70cc5f29-118c-4d96-b611-a7192d246205

</details>

<details>
<summary><b>Visual Mode</b></summary>

https://github.com/user-attachments/assets/ac156ecc-e8b8-47ee-8bd0-7a0815b5b836

</details>

<details>
<summary><b>Sidebar Layouts</b></summary>

https://github.com/user-attachments/assets/2b2b4c2e-0739-4009-99bb-62e9de0d67f7

</details>

<details>
<summary><b>Mermaid Diagrams</b></summary>

https://github.com/user-attachments/assets/927fb400-567d-4e36-a17f-64fddd23f286

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
| `Enter`  | Open selected session workspace |
| `Tab`    | Expand/collapse children |
| `s`      | Open session manager     |
| `Ctrl-h` | Hide/show sidebar panel  |
| `/`      | Open session picker     |
| `r`      | Refresh active session   |
| `?`      | Show help overlay        |
| `q`      | Quit (confirm with `y`) |
| `Ctrl-\\` | Toggle focus sidebar / target pane |
| `|` | Split target pane left/right (sidebar only) |
| `-` | Split target pane top/bottom (sidebar only) |
| `w` | Close comparison pane (shell close asks for confirmation) |

### Pane navigation

| Key | Action |
|-----|--------|
| `Ctrl-Alt-j` | Next pane (while a pane is focused) |
| `Ctrl-Alt-k` | Previous pane (while a pane is focused) |

These chords do nothing while the sidebar is focused. Use `Ctrl-\\` to focus the remembered target pane; use `Ctrl-\\` again to return to the sidebar. The sidebar marks the remembered target with a muted `target` label; the actively focused pane has a bright border and terminal cursor.

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

**Mermaid diagrams:** Fenced ` ```mermaid ` blocks are rendered as images when `mmdc` is installed (see [Optional: Mermaid diagrams](#optional-mermaid-diagrams-in-conversation-view)). In Kitty and compatible terminals, diagrams use pixel graphics and scroll with the conversation; otherwise they fall back to half-block rendering. Click a diagram or press `z` to open it. In Kitty, use the wheel or `+`/`-` to zoom, drag or `h/j/k/l` to pan, and `0` to reset; press `z` or `Esc` to close. Without `mmdc`, the fence is shown as a normal code block.

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
| `z` / click | Enlarge mermaid diagram (lightbox) |
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
- When the sidebar is hidden, `Ctrl-\\` (focus toggle) will first unhide it before switching focus
- The sidebar width is configurable via `sidebar_width` in config (default: 30)

**Session Vitals:** The bottom of the sidebar (when visible) shows a `vitals` panel for the active session — cache hit % and warmth (how stale the prompt cache is), a per-turn context/cache sparkline (cyan/violet/indigo by weighted cache reuse, `│` marks compactions), tokens by agent, tokens by model, post-compaction context culprits, and wall-clock LLM vs. tool time with slowest tools. The `context` section includes a measured system-prompt/rules/tool-schema baseline; agent and model sections allocate only conversation content. These are estimated from visible prompt content. Sections are dropped whole, lowest priority first, when the session list fills the sidebar, with a blank row above the footer.

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
- **Compare sessions or open a shell pane** — from the sidebar, press `|` or `-`, then choose an existing session or a shell. A single pane is shown flat like `[side | A]`; pane frames appear once split, and closing the last comparison returns to the flat layout. Existing sessions reuse their PTY (or attach to their existing session ID); no new OpenCode session is created. Each session remembers its pane layout while ocmux is running. While a pane is focused, `Ctrl-Alt-j/k` moves to the next/previous pane; clicking a pane also focuses it. Conversation, diff, and agents views occupy only the target pane. From the sidebar, `w` closes the targeted comparison; if the primary is targeted and there is only one comparison, `w` closes that pane. With several comparisons, target one first. Closing a shell prompts before terminating its process group.
- **Scroll help** — `?` opens a scrollable help panel. Use `j/k`, `Ctrl-d/u`, PageUp/PageDown, or `g/G`; press `Esc`, `q`, or `?` to close.

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
    "pane_next": "ctrl-alt-j",
    "pane_prev": "ctrl-alt-k"
  }
}
```

Character keybinding values are single characters. `pane_next` and `pane_prev` are chord strings, not single characters. Older `session_next` and `session_prev` config names are accepted as aliases; `pane_*` takes precedence when both are present. Default `sidebar_width` is `30`. Only the keys listed above are configurable; other bindings (`Enter`, `Tab`, `/`, `s`, `Ctrl-h`, `r`, `!`, `c`, `Ctrl-\\`, arrows) are fixed.

`notifications` controls desktop alerts. Defaults to `true`.

`follow_selection` makes sidebar `j`/`k`/arrows/click auto-attach the landed-on top-level session without stealing focus. Defaults to `false`.

`spawn_maxdepth` controls how deep `find` searches for git repos when spawning or creating worktrees. It searches `~/Programming`, `~/repos`, `~/projects`, and `~/code` (falling back to `~` if none exist). Defaults to `5`.

## Notes

- `Ctrl-\\` toggles focus between the sidebar and remembered target pane. Terminals that report this key as `Ctrl-4` are handled internally.
- `s` opens the session manager; `Ctrl-h` hides the sidebar entirely for maximum terminal space
- `q` prompts for confirmation before quitting (`y` confirm, `n`/`Esc` cancel)
- Sidebar `Enter` switches session workspaces. `Ctrl-Alt-j/k` switch panes only while a pane is focused; from the sidebar they are no-ops.
- Child sessions are expandable and selectable in the sidebar, but `Enter` attach is not yet supported for child rows — use `v` to view their conversation instead
- `c` and `!` operate on top-level sessions only
- `r` refreshes the currently active session's PTY, not the selected sidebar row
- `/` and `p` start or reuse a serve for an attached session when one is missing; the first `p` may reconnect the active TUI once
- Killing a managed session (`x`) also terminates its associated opencode serve daemon
- Deleting a session via the session manager (`s`) is permanent and hard-removes all associated messages and parts from the database

## License

Apache-2.0
