import type { OcmInstance, OcmSession } from "../store.js"

export type InstanceRow = { kind: "instance"; instance: OcmInstance }
export type ChildRow = {
  kind: "child"
  session: OcmSession
  agentType: string
  cleanedTitle: string
  depth: number
  isLast: boolean
  parentSessionId: string
}
export type ScrollIndicatorRow = {
  kind: "scroll-indicator"
  direction: "above" | "below"
  count: number
  depth: number
  parentSessionId: string
}
export type VisibleRow = InstanceRow | ChildRow | ScrollIndicatorRow

export function extractAgentType(title: string): string {
  const m = title.match(/\[([a-z-]+)\]/i)
  return m ? m[1]! : "task"
}

export function cleanTitle(title: string): string {
  return title.replace(/^\[[a-z-]+\]\s*/i, "")
}

export function buildRows(
  instances: OcmInstance[],
  expandedSessions: Set<string>,
  childSessions: Map<string, { children: OcmSession[]; totalCount: number }>,
  childScrollOffsets: Map<string, number>,
): VisibleRow[] {
  const rows: VisibleRow[] = []

  for (const instance of instances) {
    rows.push({ kind: "instance", instance })

    if (expandedSessions.has(instance.sessionId)) {
      insertChildren(rows, instance.sessionId, 1, expandedSessions, childSessions, childScrollOffsets)
    }
  }

  return rows
}

function insertChildren(
  rows: VisibleRow[],
  parentSessionId: string,
  depth: number,
  expandedSessions: Set<string>,
  childSessions: Map<string, { children: OcmSession[]; totalCount: number }>,
  childScrollOffsets: Map<string, number>,
): void {
  const data = childSessions.get(parentSessionId)
  if (!data) return

  const { children, totalCount } = data
  const offset = childScrollOffsets.get(parentSessionId) ?? 0

  if (offset > 0) {
    rows.push({ kind: "scroll-indicator", direction: "above", count: offset, depth, parentSessionId })
  }

  children.forEach((child, i) => {
    const isLast = i === children.length - 1 && offset + children.length >= totalCount
    rows.push({
      kind: "child",
      session: child,
      agentType: extractAgentType(child.title),
      cleanedTitle: cleanTitle(child.title),
      depth,
      isLast,
      parentSessionId,
    })
    if (expandedSessions.has(child.id)) {
      insertChildren(rows, child.id, depth + 1, expandedSessions, childSessions, childScrollOffsets)
    }
  })

  const remaining = totalCount - offset - children.length
  if (remaining > 0) {
    rows.push({ kind: "scroll-indicator", direction: "below", count: remaining, depth, parentSessionId })
  }
}

export function getNavigableIndices(rows: VisibleRow[]): number[] {
  return rows
    .map((r, i) => (r.kind === "scroll-indicator" ? -1 : i))
    .filter((i) => i >= 0)
}
