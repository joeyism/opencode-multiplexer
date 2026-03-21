import { create } from "zustand"

// ─── Status types ─────────────────────────────────────────────────────────────

export type SessionStatus =
  | "working"
  | "needs-input"
  | "idle"
  | "error"

// ─── Instance type (one per running opencode process) ────────────────────────

export interface OcmInstance {
  id: string              // unique key: "{worktree}-{sessionId}"
  sessionId: string       // the session this process is running
  sessionTitle: string    // title from session table
  projectId: string
  worktree: string        // absolute path
  repoName: string        // basename(worktree)
  status: SessionStatus
  lastPreview: string
  lastPreviewRole: "user" | "assistant"
  hasChildren: boolean
  model: string | null    // last model used in this session (shortened)
  port: number | null     // only set for opencode serve instances (spawned via OCMux)
}

// ─── Session type (for subagent tree children) ────────────────────────────────

export interface OcmSession {
  id: string
  projectId: string
  title: string
  directory: string
  status: SessionStatus
  lastMessagePreview: string
  lastMessageRole: "user" | "assistant"
  model: string | null
  timeUpdated: number
  hasChildren?: boolean
}

// ─── Conversation message types ───────────────────────────────────────────────

export interface ConversationMessagePart {
  id: string
  type: string
  text?: string
  tool?: string
  toolStatus?: string
  callId?: string
  toolTitle?: string
  toolInput?: string
  toolHeader?: string
}

export interface ConversationMessage {
  id: string
  sessionId: string
  role: "user" | "assistant"
  timeCreated: number
  timeCompleted: number | null
  modelId: string | null
  providerId: string | null
  agent?: string
  parts: ConversationMessagePart[]
}

// ─── View types ───────────────────────────────────────────────────────────────

export type ViewName = "dashboard" | "conversation" | "spawn" | "worktree"

// ─── Store ────────────────────────────────────────────────────────────────────

interface Store {
  // Live instances (one per running opencode process)
  instances: OcmInstance[]
  setInstances: (instances: OcmInstance[]) => void

  // Expandable subagent tree
  expandedSessions: Set<string>
  childSessions: Map<string, { children: OcmSession[]; totalCount: number }>
  childScrollOffsets: Map<string, number>
  toggleExpanded: (sessionId: string) => void
  collapseSession: (sessionId: string) => void
  setChildSessions: (parentId: string, children: OcmSession[], totalCount: number) => void
  setChildScrollOffset: (sessionId: string, offset: number) => void

  // Navigation
  view: ViewName
  selectedProjectId: string | null
  selectedSessionId: string | null
  cursorIndex: number
  setCursorIndex: (index: number) => void
  navigate: (view: ViewName, projectId?: string, sessionId?: string) => void

  // Conversation (loaded on demand from SQLite)
  messages: ConversationMessage[]
  messagesLoading: boolean
  setMessages: (messages: ConversationMessage[]) => void
  setMessagesLoading: (loading: boolean) => void
}

export const useStore = create<Store>((set) => ({
  // Instances
  instances: [],
  setInstances: (instances) => set({ instances }),

  // Expandable subagent tree
  expandedSessions: new Set(),
  childSessions: new Map(),
  childScrollOffsets: new Map(),
  toggleExpanded: (sessionId) =>
    set((state) => {
      const next = new Set(state.expandedSessions)
      if (next.has(sessionId)) {
        next.delete(sessionId)
      } else {
        next.add(sessionId)
      }
      return { expandedSessions: next }
    }),
  collapseSession: (sessionId) =>
    set((state) => {
      const next = new Set(state.expandedSessions)
      next.delete(sessionId)
      return { expandedSessions: next }
    }),
  setChildSessions: (parentId, children, totalCount) =>
    set((state) => {
      const next = new Map(state.childSessions)
      next.set(parentId, { children, totalCount })
      return { childSessions: next }
    }),
  setChildScrollOffset: (sessionId, offset) =>
    set((state) => {
      const next = new Map(state.childScrollOffsets)
      next.set(sessionId, offset)
      return { childScrollOffsets: next }
    }),

  // Navigation
  view: "dashboard",
  selectedProjectId: null,
  selectedSessionId: null,
  cursorIndex: 0,
  setCursorIndex: (cursorIndex) => set({ cursorIndex }),
  navigate: (view, projectId, sessionId) =>
    set({
      view,
      selectedProjectId: projectId ?? null,
      selectedSessionId: sessionId ?? null,
      cursorIndex: 0,
      messages: [],
      messagesLoading: false,
    }),

  // Conversation
  messages: [],
  messagesLoading: false,
  setMessages: (messages) => set({ messages }),
  setMessagesLoading: (messagesLoading) => set({ messagesLoading }),
}))
