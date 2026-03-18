# OpenCode Manager (`ocm`) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** A terminal UI that orchestrates multiple opencode instances across repos, showing real-time status, conversation history, and inline reply — without attaching to the underlying TUI.

**Architecture:** External TypeScript TUI that connects to multiple opencode HTTP servers via the `@opencode-ai/sdk`. Each opencode instance runs in a tmux session. The manager subscribes to each instance's SSE event stream (`GET /event`) for real-time status updates. Users can view conversations and reply via the SDK, or full-attach to the tmux session for deeper interaction.

**Tech Stack:** TypeScript, Bun, Ink (React for terminal), `@opencode-ai/sdk`, Zustand, tmux

---

## Important API Details (from SDK source)

These are the exact SDK methods and types from `@opencode-ai/sdk@1.2.27` (repo: `sst/opencode-sdk-js`).

### Client Initialization

```typescript
import Opencode from "@opencode-ai/sdk"

const client = new Opencode({ baseURL: "http://localhost:4096" })
```

### Session Methods

| Method | Description |
|--------|-------------|
| `client.session.create()` | Create a new session |
| `client.session.list()` | List all sessions, returns `Session[]` |
| `client.session.delete(id)` | Delete a session |
| `client.session.abort(id)` | Abort a running session |
| `client.session.chat(id, params)` | Send a message (this is the reply method) |
| `client.session.messages(id)` | Get all messages for a session |
| `client.session.share(id)` | Share a session |

### `SessionChatParams` (for sending messages)

```typescript
interface SessionChatParams {
  modelID: string        // e.g. "claude-sonnet-4-20250514"
  providerID: string     // e.g. "anthropic"
  parts: Array<TextPartInput | FilePartInput>
  messageID?: string
  mode?: string
  system?: string
  tools?: { [key: string]: boolean }
}

interface TextPartInput {
  type: "text"
  text: string
  id?: string
}
```

### Event Stream (SSE) — `GET /event`

```typescript
const stream = await client.event.list()  // Returns Stream<EventListResponse>
```

Key event types:

| Event | Fires when |
|-------|-----------|
| `session.idle` | Session done generating, waiting for input. Props: `{ sessionID }` |
| `session.error` | Session encountered an error. Props: `{ sessionID, error }` |
| `session.updated` | Session metadata changed. Props: `{ info: Session }` |
| `message.updated` | Message created/updated. Props: `{ info: Message }` |
| `message.part.updated` | Message part (text, tool call) updated. Props: `{ part: Part }` |
| `permission.updated` | Agent requesting permission. Props: `{ id, sessionID, title, metadata }` |

This means **no polling needed**. Subscribe to the event stream for instant updates.

### Message Structure

Messages are returned as `{ info: Message, parts: Part[] }` arrays.

- `Message` is `UserMessage` (role: "user") or `AssistantMessage` (role: "assistant")
- `AssistantMessage` has `time.completed?: number` — if undefined, still generating
- `AssistantMessage` has `modelID` and `providerID` fields (for reuse when replying)
- `Part` types: `TextPart`, `ToolPart`, `StepStartPart`, `StepFinishPart`, `SnapshotPart`, `PatchPart`
- `ToolPart.state.status`: `"pending" | "running" | "completed" | "error"`
- `TextPart.text` contains the actual conversation text

### Status Detection Logic

A session is **"needs-input"** when:
1. `session.idle` event fires for that session, OR
2. `permission.updated` event fires (agent asking for approval), OR
3. Last message is `AssistantMessage` with `time.completed` set

A session is **"working"** when:
1. `message.part.updated` events are flowing, OR
2. Last `AssistantMessage` has `time.completed === undefined`

---

## Project Structure

```
opencode-manager/
├── package.json
├── tsconfig.json
├── src/
│   ├── index.tsx              # Entry point, mounts Ink <App />
│   ├── app.tsx                # Root component, view router
│   │
│   ├── store.ts               # Zustand store (single file, all state)
│   ├── config.ts              # Load ~/.config/opencode-manager/config.json, merge defaults
│   │
│   ├── registry/
│   │   ├── registry.ts        # Instance tracking, persist to disk, health check
│   │   ├── spawner.ts         # Spawn opencode in tmux, assign port, wait ready, register
│   │   └── discovery.ts       # Scan port range for unregistered opencode instances
│   │
│   ├── events/
│   │   └── subscriber.ts      # Subscribe to SSE event stream per instance, dispatch to store
│   │
│   ├── views/
│   │   ├── dashboard.tsx       # Session list grouped by repo, status indicators
│   │   ├── conversation.tsx    # Message history + reply input
│   │   ├── spawn.tsx           # New instance dialog (folder picker via fzf)
│   │   └── help.tsx            # Help overlay showing keybindings
│   │
│   └── hooks/
│       ├── use-keybindings.ts  # Centralized keyboard handler, reads config
│       └── use-attach.ts       # Full-attach flow (execSync tmux attach)
│
└── test/
    ├── spike-status.ts         # Spike: test event stream and status detection
    ├── spike-attach.ts         # Spike: test Ink recovery after tmux attach
    └── spike-chat.ts           # Spike: test sending messages while TUI is running
```

## State Management (Zustand)

Single store in `src/store.ts`:

```typescript
import { create } from "zustand"
import type { Session, Message, Part } from "@opencode-ai/sdk"

type SessionStatus = "working" | "needs-input" | "idle" | "error" | "permission"

interface Instance {
  id: string
  port: number
  cwd: string
  repoName: string        // basename of cwd
  tmuxSession: string     // "ocm-{port}"
  pid?: number
}

interface SessionInfo {
  session: Session
  status: SessionStatus
  lastMessagePreview: string
  lastMessageRole: "user" | "assistant"
  model?: string
  error?: string
  permissionTitle?: string
}

interface Store {
  // Registry
  instances: Map<string, Instance>
  registerInstance: (instance: Instance) => void
  deregisterInstance: (id: string) => void

  // Sessions (populated by event stream)
  // keyed: instanceId -> sessionId -> SessionInfo
  sessions: Map<string, Map<string, SessionInfo>>
  updateSessionInfo: (instanceId: string, sessionId: string, info: Partial<SessionInfo>) => void
  setSessionsForInstance: (instanceId: string, sessions: Map<string, SessionInfo>) => void

  // Navigation
  view: "dashboard" | "conversation" | "spawn"
  selectedInstanceId: string | null
  selectedSessionId: string | null
  cursorIndex: number
  navigate: (view: Store["view"], instanceId?: string, sessionId?: string) => void

  // Conversation (loaded on demand)
  messages: Array<{ info: Message; parts: Part[] }>
  messagesLoading: boolean
  loadMessages: (instanceId: string, sessionId: string) => Promise<void>
  appendMessage: (info: Message, parts: Part[]) => void
  updatePart: (part: Part) => void
}
```

## Configuration

File: `~/.config/opencode-manager/config.json`

```json
{
  "keybindings": {
    "dashboard": {
      "up": "k",
      "down": "j",
      "open": "return",
      "attach": "a",
      "spawn": "n",
      "nextNeedsInput": "tab",
      "delete": "d",
      "abort": "x",
      "quit": "q",
      "help": "?"
    },
    "conversation": {
      "back": "escape",
      "attach": "a",
      "send": "return",
      "scrollUp": "up",
      "scrollDown": "down"
    },
    "spawn": {
      "cancel": "escape",
      "confirm": "return"
    }
  },
  "discovery": {
    "portRangeStart": 4096,
    "portRangeEnd": 4115,
    "scanOnStartup": true
  },
  "tmux": {
    "prefix": "C-a",
    "sessionPrefix": "ocm"
  }
}
```

All overridable. Missing file = all defaults. No conflicts with user's tmux config (prefix `C-a`, ocm bindings are unprefixed single keys inside the Ink TUI).

---

## Implementation Tasks

### Task 0: Spike — Validate Assumptions

**Files:**
- Create: `test/spike-status.ts`
- Create: `test/spike-attach.ts`
- Create: `test/spike-chat.ts`

**Step 1: Set up minimal project**

```bash
bun init -y
bun add @opencode-ai/sdk ink react zustand ink-text-input
bun add -d @types/react typescript
```

Create `tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "outDir": "dist",
    "rootDir": "src",
    "esModuleInterop": true,
    "skipLibCheck": true
  },
  "include": ["src/**/*", "test/**/*"]
}
```

**Step 2: Spike event stream status detection**

Start opencode manually: `opencode serve --port 4096` (in some project dir).

Create `test/spike-status.ts`:
```typescript
import Opencode from "@opencode-ai/sdk"

const client = new Opencode({ baseURL: "http://localhost:4096" })

const sessions = await client.session.list()
console.log("Sessions:", sessions.map(s => ({ id: s.id, title: s.title })))

const stream = await client.event.list()
console.log("Listening for events...")

for await (const event of stream) {
  console.log(`[${event.type}]`, JSON.stringify(event.properties, null, 2))
}
```

Run: `bun test/spike-status.ts`

Verify:
- `session.idle` fires when agent finishes and waits for input
- `permission.updated` fires when agent needs approval
- `message.part.updated` fires during generation with `TextPart` updates

**Step 3: Spike sending messages via SDK**

Create `test/spike-chat.ts`:
```typescript
import Opencode from "@opencode-ai/sdk"

const client = new Opencode({ baseURL: "http://localhost:4096" })

const sessions = await client.session.list()
const session = sessions[0]
if (!session) { console.log("No sessions."); process.exit(1) }

const providers = await client.app.providers()
console.log("Providers:", JSON.stringify(providers, null, 2))

// Adjust providerID/modelID based on providers output
const result = await client.session.chat(session.id, {
  providerID: "anthropic",
  modelID: "claude-sonnet-4-20250514",
  parts: [{ type: "text", text: "What files are in the current directory?" }],
})
console.log("Response:", JSON.stringify(result, null, 2))
```

Run: `bun test/spike-chat.ts`

Verify: Get a response. Check if opencode TUI also shows the message.

**Step 4: Spike Ink recovery after tmux attach**

Pre-req: `tmux new-session -d -s spike-test "bash"`

Create `test/spike-attach.ts`:
```typescript
import React from "react"
import { render, Text, Box, useInput } from "ink"
import { execSync } from "child_process"

function App() {
  const [status, setStatus] = React.useState("Ready. Press 'a' to attach, 'q' to quit.")

  useInput((input) => {
    if (input === "a") {
      setStatus("Attaching...")
      setTimeout(() => {
        try {
          execSync("tmux attach-session -t spike-test", { stdio: "inherit" })
        } catch {}
        setStatus("Returned from tmux. Press 'a' again or 'q' to quit.")
      }, 100)
    }
    if (input === "q") process.exit(0)
  })

  return (
    <Box flexDirection="column">
      <Text>{status}</Text>
    </Box>
  )
}

render(<App />)
```

Run: `bun test/spike-attach.ts`

Verify: press `a`, tmux takes over, `Ctrl-A D` detach, Ink resumes.
If broken: test unmount/remount approach.

**Step 5: Record findings and commit**

```bash
git add -A && git commit -m "spike: validate event stream, chat API, and tmux attach recovery"
```

---

### Task 1: Project Scaffolding

**Files:**
- Create: `package.json` (finalize with bin entry)
- Create: `tsconfig.json`
- Create: `src/index.tsx`
- Create: `src/app.tsx`
- Create: `src/store.ts`
- Create: `src/config.ts`

**Step 1: Finalize `package.json`**

Add bin entry and scripts:
```json
{
  "name": "opencode-manager",
  "bin": { "ocm": "./src/index.tsx" },
  "scripts": {
    "dev": "bun src/index.tsx",
    "build": "bun build src/index.tsx --outdir dist --target bun"
  }
}
```

**Step 2: Write `src/config.ts`**

Read `~/.config/opencode-manager/config.json`, deep-merge with defaults, export merged config and `Config` type. Handle missing file (return all defaults).

**Step 3: Write `src/store.ts`**

Zustand store as described above. All initial values are empty maps/null. No business logic — just setters.

**Step 4: Write `src/app.tsx`**

Root component renders view from `store.view`. Placeholder `<Text>` for each view.

**Step 5: Write `src/index.tsx`**

```typescript
#!/usr/bin/env bun
import React from "react"
import { render } from "ink"
import { App } from "./app.js"

render(<App />)
```

**Step 6: Verify and commit**

```bash
bun src/index.tsx  # Should show placeholder dashboard text
git add -A && git commit -m "feat: project scaffolding with Ink, Zustand, and SDK"
```

---

### Task 2: Instance Registry

**Files:**
- Create: `src/registry/registry.ts`
- Create: `src/registry/spawner.ts`
- Create: `src/registry/discovery.ts`

**Step 1: Write `registry.ts`**

- Persistence: `~/.config/opencode-manager/instances.json`
- `load()` — read from disk, verify each PID alive (`process.kill(pid, 0)` in try/catch), remove dead, create SDK clients for live ones
- `save()` — write to disk
- `register(instance)` / `deregister(id)` — update store + save
- `getClient(instanceId)` — return `Opencode` SDK client (stored in separate `Map`, not Zustand)

**Step 2: Write `spawner.ts`**

`spawn(cwd: string)`:
1. Find next available port (start `config.discovery.portRangeStart`, skip used)
2. tmux session name: `${config.tmux.sessionPrefix}-${port}`
3. Run: `tmux new-session -d -s {name} -c {cwd} "opencode serve --port {port}"`
4. Poll `http://localhost:{port}/doc` every 500ms until ready (15s timeout)
5. Create SDK client: `new Opencode({ baseURL: "http://localhost:{port}" })`
6. Get PID: `tmux list-panes -t {name} -F "#{pane_pid}"`
7. Register instance, return it

**Step 3: Write `discovery.ts`**

`discover()`:
1. Scan ports in config range, `fetch("http://localhost:{port}/doc")` with 1s timeout
2. Skip already-registered ports
3. For hits: create client, infer cwd from API, find matching tmux session
4. Register discovered instances

**Step 4: Verify spawner works**

Temporarily test from `index.tsx`: spawn an instance, list sessions.

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: instance registry with spawn, persistence, and discovery"
```

---

### Task 3: Event Subscriber

**Files:**
- Create: `src/events/subscriber.ts`

**Step 1: Write `subscriber.ts`**

`subscribe(instanceId, client)`:
1. `client.event.list()` → get SSE stream
2. `for await (const event of stream)` dispatch to Zustand:
   - `session.idle` → status `"needs-input"`
   - `session.error` → status `"error"`, store error
   - `session.updated` → update session info
   - `message.updated` → append/update message, extract preview for dashboard
   - `message.part.updated` → update part (live text streaming), set status `"working"`
   - `permission.updated` → status `"permission"`, store title
3. Retry on disconnect: exponential backoff (1s, 2s, 4s, max 30s)

`unsubscribe(instanceId)` — abort stream, clean up.

On startup:
1. Load registry
2. For each live instance: fetch initial session list (`client.session.list()`), then `subscribe()`

**Step 2: Verify events flow**

Start an opencode instance, run ocm, interact with opencode, verify events update the store.

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: SSE event subscriber with real-time status updates"
```

---

### Task 4: Dashboard View

**Files:**
- Create: `src/views/dashboard.tsx`
- Create: `src/hooks/use-keybindings.ts`
- Modify: `src/app.tsx` (wire in real dashboard)

**Step 1: Write `use-keybindings.ts`**

Read keybinding config, return `useInput` handler mapping keys to callback actions.

**Step 2: Write `dashboard.tsx`**

Layout:
```
 ocm — 3 instances, 8 sessions — 2 need attention
 ──────────────────────────────────────────────────
  project-api
    ◐ refactor-auth        claude-4   "Should I mock the DB layer?"
    ● add-unit-tests       claude-4   working...
    ○ idle-session         claude-4   idle
  project-web
    ● migrate-to-react-19  claude-4   working...
  project-infra
    ◐ fix-deploy-script    gemini     "The AWS creds are expired..."
 ──────────────────────────────────────────────────
  j/k: navigate  Enter: open  a: attach  n: new  Tab: next alert  ?: help
```

- Group by `instance.repoName`
- Sort within groups: `needs-input`/`permission` first, then `working`, then `idle`
- Status: `●` green (working), `◐` yellow (needs-input/permission), `○` gray (idle), `✗` red (error)
- `Tab` cycles through needs-input sessions
- Truncate preview to terminal width

**Step 3: Wire into `app.tsx`, verify with live data**

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: dashboard view with real-time status and keyboard navigation"
```

---

### Task 5: Conversation View & Inline Reply

**Files:**
- Create: `src/views/conversation.tsx`

**Step 1: Write `conversation.tsx`**

- On mount: `client.session.messages(id)` → populate store
- Render: `UserMessage` text, `AssistantMessage` text + tool summaries
- `ToolPart` one-liner: tool name + status icon
- Live updates via event stream (already dispatching to store)
- Input box: `ink-text-input`
- On Enter: reuse `providerID`/`modelID` from last `AssistantMessage`, call `client.session.chat()`
- Scrolling: `up`/`down` when input empty

**Step 2: Verify: open session, read conversation, reply, watch response stream in**

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: conversation view with message history and inline reply"
```

---

### Task 6: Full Attach & tmux Integration

**Files:**
- Create: `src/hooks/use-attach.ts`

**Step 1: Write `use-attach.ts`**

Use whichever approach the spike validated (execSync or unmount/remount).

**Step 2: Wire `a` key in dashboard and conversation**

**Step 3: Show detach instruction using configured tmux prefix**

**Step 4: Verify full cycle: dashboard → attach → detach → back in dashboard**

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: full-attach to tmux with Ink recovery"
```

---

### Task 7: Spawn View

**Files:**
- Create: `src/views/spawn.tsx`

**Step 1: Write `spawn.tsx`**

1. Shell out to `fzf` for folder selection (configurable search paths)
2. If fzf not installed, fall back to `ink-text-input` for manual path
3. Call `spawner.spawn(path)`
4. Subscribe to new instance's event stream
5. Navigate back to dashboard

**Step 2: Verify: press `n`, select folder, instance appears in dashboard**

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: spawn view with fzf folder picker"
```

---

### Task 8: Error Handling & Edge Cases

**Files:**
- Modify: `src/events/subscriber.ts`
- Modify: `src/registry/registry.ts`
- Modify: `src/views/dashboard.tsx`

**Step 1:** Disconnected instances — "disconnected" status, retry backoff, deregister after 5 failures

**Step 2:** Instance deletion — `d` with confirm, abort sessions, kill tmux, deregister

**Step 3:** Stale instances on startup — verify PID + port + tmux, reap dead

**Step 4:** Port conflicts — TCP check before spawning

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: error handling for disconnected instances and edge cases"
```

---

### Task 9: Help Overlay & Polish

**Files:**
- Create: `src/views/help.tsx`
- Modify: `src/app.tsx`

**Step 1:** Help overlay — `?` toggles bordered overlay showing keybindings

**Step 2:** Header — `ocm — {n} instances, {m} sessions — {k} need attention`

**Step 3:** Colors — green/yellow/red/gray status, bold groups, cursor indicator

**Step 4:** Graceful shutdown — unsubscribe streams, save registry, don't kill instances

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: help overlay, polish, and graceful shutdown"
```

---

## Task Order & Estimates

| Task | Description | Depends On | Est. Time |
|------|------------|------------|-----------|
| 0 | Spike — validate assumptions | none | 2-3 hours |
| 1 | Project scaffolding | Task 0 | 30 min |
| 2 | Instance registry | Task 1 | 2-3 hours |
| 3 | Event subscriber | Task 1, 2 | 2 hours |
| 4 | Dashboard view | Task 1, 2, 3 | 3 hours |
| 5 | Conversation view & reply | Task 3, 4 | 3 hours |
| 6 | Full-attach (tmux) | Task 0 spike, 4 | 1 hour |
| 7 | Spawn view (fzf) | Task 2, 4 | 1.5 hours |
| 8 | Error handling | Task 2, 3, 4 | 2 hours |
| 9 | Polish | Task 4, 5 | 1.5 hours |

**Total: ~18-20 hours (3-4 focused days)**

## Key Risks & Mitigations

| Risk | Mitigation | Validated By |
|------|-----------|-------------|
| Event stream doesn't expose "needs input" | `session.idle` event exists in SDK types | Task 0 spike |
| Ink doesn't recover after tmux attach | Test execSync vs unmount/remount | Task 0 spike |
| SDK chat doesn't reflect in opencode TUI | Document as separate view if needed | Task 0 spike |
| Event stream disconnects | Exponential backoff reconnect | Task 3 |
| Port conflicts on spawn | TCP connect check before spawning | Task 2 |
