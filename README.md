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
  - [Visual mode](#visual-mode)
  - [Shell mode](#shell-mode)
  - [Sidebar layout](#sidebar-layout)
- [Advanced workflows](#advanced-workflows)
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

## Usage

Run `ocmux` in your terminal. The sidebar lists active OpenCode sessions sorted by most recently updated. The main pane shows the attached session's terminal output.

- Press `Enter` to attach to a top-level session
- Press `Tab` to expand/collapse child sessions
- Press `v` to open a read-only conversation view (see below)
- Press `s` to collapse the sidebar for more screen space
- Click a sidebar row to select it

## Keybindings

### Sidebar navigation

| Key      | Action                    |
|----------|---------------------------|
| `j` / `Down` | Move down             |
| `k` / `Up`   | Move up               |
| `Enter`  | Attach to session        |
| `Tab`    | Expand/collapse children |
| `s`      | Toggle sidebar collapse  |
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
| `v` | Open read-only conversation view       |
| `d` | Open diff view for session             |
| `f` | Show files modified by session        |
| `!` | Drop into shell in session directory   |
| `c` | Commit/push modified files            |
| `h` | Open message history picker           |
| `x` | Kill session (`y` confirm, `n`/`Esc` cancel) |

### Session picker

Press `/` to search and attach to any opencode session. The picker uses **fuzzy search** across repo, title, and directory fields. Live sessions (currently running) are marked with a green dot (`●`).

| Key      | Action                    |
|----------|---------------------------|
| `↑` / `↓` | Move through list       |
| `Enter`  | Attach to selected        |
| `Backspace` | Delete last character |
| any key  | Filter by fuzzy match     |
| `Esc`    | Cancel picker            |

The footer shows `matched/total` counts. Results are sorted by live status first, then fuzzy match score, then recency.

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

Press `v` from the sidebar to open a read-only view of the session's conversation history. The view polls the opencode database every second and renders messages, markdown, syntax-highlighted code blocks, and tool call status.

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
| `q` / `v` / `Esc` | Close view       |

**Auto-follow:** By default, the view stays at the bottom and scrolls automatically as new messages arrive. Scrolling up manually disengages follow mode. Jump to end (`G`) or page down at the bottom (`Ctrl-d`) to resume following.

**Search:** Search is case-insensitive and incremental — type to filter, `Enter` to confirm. The search bar shows current match position (`1/5`). Pressing `/` again or `Esc` cancels.

Mouse scroll is supported.

### Diff view
<p align="center" width="100%">
<video src="https://github.com/user-attachments/assets/70cc5f29-118c-4d96-b611-a7192d246205" width="80%" controls></video>
</p>

Press `d` from the sidebar to open a read-only view of the session's git diff. The view shows both tracked and untracked changes, preferring the opencode serve API when available and falling back to `git diff` otherwise.

The diff view uses a **cursor-based** navigation model (distinct from the scroll-based conversation view). The cursor determines the position for visual selection.

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

**Search:** Case-insensitive, incremental. The search bar shows current/total matches. The `/` key is disabled while visual mode is active — cancel visual mode first to search.

Mouse scroll is supported.

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

The sidebar has three states:

| State | Key | Behavior |
|-------|-----|----------|
| Expanded | (default) | Full-width sidebar showing session tree, title, and status |
| Collapsed | `s` | Sidebar shrinks to a narrow width (12 columns), showing only session names |
| Hidden | `Ctrl-h` | Sidebar disappears entirely for maximum terminal space |

- Toggle between expanded and collapsed with `s`
- Toggle hidden with `Ctrl-h`
- When the sidebar is hidden, `Ctrl-4` (focus toggle) will first unhide it before switching focus
- The sidebar width when expanded is configurable via `sidebar_width` in config (default: 30)

## Advanced workflows

- **Inspect conversation output** — press `v` on any session (including child sessions) to watch the agent's progress in real-time without attaching to the PTY
- **Inspect changed files** — press `d` to open a diff view of all changes made by a session (tracked and untracked)
- **Inspect changed files (list)** — press `f` to see which files a session has created or modified. Press any key or `Esc` to close.
- **Drop into a shell** — press `!` to open a shell in the selected session's working directory. The shell inherits the session's environment; exiting the shell returns to ocmux.
- **Commit session changes** — press `c` to prompted for a commit message, then commit and push all modified files immediately.
- **Search and attach** — press `/` to open the session picker, search across all opencode sessions, and attach to one.
- **Reuse an earlier prompt** — press `h` to open the message history picker, search past user messages, and paste one into the active terminal session.
- **Create a worktree** — press `t` to pick a repo directory, then enter a branch name (leave empty to spawn in the repo root without a worktree). A new worktree is created and a session is spawned in it.
- **Spawn a session** — press `n` to pick a repo from discovered git repositories (see Configuration for search paths and depth), then spawn a new managed session in that directory.

## System behaviors

**Focus tracking** — When the ocmux window loses OS focus, the border dims to dark gray. When focus returns, the border resumes normal styling.

**Sidebar sync** — When focus is on the terminal (attached session), the sidebar selection automatically tracks the active session. If the attached session exits, focus returns to the sidebar with a "session exited" footer message.

**Notifications** — When `notifications: true` in config, ocmux sends desktop alerts on specific session transitions: `Working → Idle`, `Working → NeedsInput`, `Working → Error`. Each session has a 5-second cooldown between notifications. On macOS, `notify-rust` is used (which respects Do Not Disturb / Focus modes). On Linux, `notify-send` is used as a fallback. Notifications are suppressed while ocmux is the focused window.

**Terminal features** — ocmux supports bracketed paste (safe paste of multi-line content), full special-key forwarding (arrows, Home, End, PageUp, PageDown, F-keys, etc.), and proper terminal resize on window changes.

## Configuration

Keybindings, sidebar width, desktop notifications, and repo search depth are configurable via `~/.config/ocmux/config.json`:

```json
{
  "sidebar_width": 30,
  "notifications": true,
  "spawn_maxdepth": 5,
  "keybindings": {
    "up": "k",
    "down": "j",
    "spawn": "n",
    "kill": "x",
    "help": "?",
    "worktree": "t",
    "quit": "q",
    "view": "v",
    "files": "f",
    "diff": "d",
    "history": "h"
  }
}
```

Keybinding values are single characters. Default `sidebar_width` is `30`. Only the keys listed above are configurable; other bindings (`Enter`, `Tab`, `/`, `s`, `Ctrl-h`, `r`, `!`, `c`, `Ctrl-4`, arrows) are fixed.

`notifications` controls desktop alerts. Defaults to `true`.

`spawn_maxdepth` controls how deep `find` searches for git repos when spawning or creating worktrees. It searches `~/Programming`, `~/repos`, `~/projects`, and `~/code` (falling back to `~` if none exist). Defaults to `5`.

## Notes

- `Ctrl-4` is the actual focus toggle binding (hold `Ctrl` and press `4`)
- `s` collapses the sidebar to a narrow width; `Ctrl-h` hides it entirely for maximum terminal space
- `q` prompts for confirmation before quitting (`y` confirm, `n`/`Esc` cancel)
- Child sessions are expandable and selectable in the sidebar, but `Enter` attach is not yet supported for child rows — use `v` to view their conversation instead
- `c` and `!` operate on top-level sessions only
- `r` refreshes the currently active session's PTY, not the selected sidebar row
- Killing a managed session (`x`) also terminates its associated opencode serve daemon

## License

Apache-2.0
