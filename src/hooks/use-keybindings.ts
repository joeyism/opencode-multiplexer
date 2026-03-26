import { useInput } from "ink"
import { config } from "../config.js"

type DashboardActions = {
  onUp?: () => void
  onDown?: () => void
  onOpen?: () => void
  onAttach?: () => void
  onSpawn?: () => void
  onWorktree?: () => void
  onExpand?: () => void
  onCollapse?: () => void
  onNextNeedsInput?: () => void
  onKill?: () => void
  onRenameTitle?: () => void
  onQuit?: () => void
  onHelp?: () => void
  onRescan?: () => void
  onSessions?: () => void
  onTogglePin?: () => void
}

type ConversationActions = {
  onBack?: () => void
  onAttach?: () => void
  onSend?: () => void
  onScrollUp?: () => void
  onScrollDown?: () => void
  onScrollHalfPageUp?: () => void
  onScrollHalfPageDown?: () => void
  onScrollPageUp?: () => void
  onScrollPageDown?: () => void
  onScrollBottom?: () => void
  onScrollTop?: () => void
  onShell?: () => void
}

type SpawnActions = {
  onCancel?: () => void
  onConfirm?: () => void
}

function matchKey(key: string, input: string, inkKey: any): boolean {
  switch (key) {
    case "return":
      return inkKey.return
    case "escape":
      return inkKey.escape
    case "tab":
      return inkKey.tab && !inkKey.shift
    case "shift-tab":
      return inkKey.tab && inkKey.shift
    case "up":
      return inkKey.upArrow
    case "down":
      return inkKey.downArrow
    default:
      // Handle ctrl- prefixed keys like "ctrl-n"
      if (key.startsWith("ctrl-")) {
        const letter = key.slice(5)
        return inkKey.ctrl && !inkKey.tab && !inkKey.return && !inkKey.escape && input === letter
      }
      return input === key && !inkKey.ctrl && !inkKey.meta
  }
}

export function useDashboardKeys(actions: DashboardActions, isActive = true) {
  const kb = config.keybindings.dashboard
  useInput(
    (input, key) => {
      if (matchKey(kb.up, input, key) || key.upArrow) actions.onUp?.()
      else if (matchKey(kb.down, input, key) || key.downArrow) actions.onDown?.()
      else if (matchKey(kb.open, input, key)) actions.onOpen?.()
      else if (matchKey(kb.attach, input, key)) actions.onAttach?.()
      else if (matchKey(kb.worktree, input, key)) actions.onWorktree?.()
      else if (matchKey(kb.spawn, input, key)) actions.onSpawn?.()
      else if (matchKey(kb.expand, input, key)) actions.onExpand?.()
      else if (matchKey(kb.collapse, input, key)) actions.onCollapse?.()
      else if (matchKey(kb.nextNeedsInput, input, key)) actions.onNextNeedsInput?.()
      else if (matchKey(kb.kill, input, key)) actions.onKill?.()
      else if (matchKey(kb.renameTitle, input, key)) actions.onRenameTitle?.()
      else if (matchKey(kb.quit, input, key)) actions.onQuit?.()
      else if (matchKey(kb.help, input, key)) actions.onHelp?.()
      else if (matchKey(kb.rescan, input, key)) actions.onRescan?.()
      else if (matchKey(kb.sessions, input, key)) actions.onSessions?.()
      else if (matchKey(kb.togglePin, input, key)) actions.onTogglePin?.()
    },
    { isActive },
  )
}

export function useConversationKeys(actions: ConversationActions) {
  const kb = config.keybindings.conversation
  useInput((input, key) => {
    if (matchKey(kb.back, input, key) || input === "q") actions.onBack?.()
    else if (matchKey(kb.attach, input, key)) actions.onAttach?.()
    else if (matchKey(kb.send, input, key)) actions.onSend?.()
    else if (matchKey(kb.scrollUp, input, key) || key.upArrow) actions.onScrollUp?.()
    else if (matchKey(kb.scrollDown, input, key) || key.downArrow) actions.onScrollDown?.()
    else if (matchKey(kb.scrollHalfPageUp, input, key)) actions.onScrollHalfPageUp?.()
    else if (matchKey(kb.scrollHalfPageDown, input, key)) actions.onScrollHalfPageDown?.()
    else if (matchKey(kb.scrollPageUp, input, key)) actions.onScrollPageUp?.()
    else if (matchKey(kb.scrollPageDown, input, key)) actions.onScrollPageDown?.()
    else if (matchKey(kb.scrollBottom, input, key) && key.shift) actions.onScrollBottom?.()
    else if (matchKey(kb.scrollTop, input, key) && !key.shift) actions.onScrollTop?.()
    else if (matchKey(kb.shell, input, key)) actions.onShell?.()
  })
}

export function useSpawnKeys(actions: SpawnActions) {
  const kb = config.keybindings.spawn
  useInput((input, key) => {
    if (matchKey(kb.cancel, input, key)) actions.onCancel?.()
    else if (matchKey(kb.confirm, input, key)) actions.onConfirm?.()
  })
}
