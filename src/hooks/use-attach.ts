import { execSync } from "child_process"
import { writeFileSync, readFileSync, unlinkSync } from "fs"
import { tmpdir } from "os"
import { join } from "path"
import { render } from "ink"
import React from "react"

const EXIT_ALT_SCREEN  = "\x1b[?1049l"
const ENTER_ALT_SCREEN = "\x1b[?1049h"
const CLEAR_SCREEN     = "\x1b[2J\x1b[H"

// We store the Ink instance so we can unmount and remount it
let _inkInstance: ReturnType<typeof render> | null = null

// Module-level side-channel for passing editor results across the remount boundary.
// onResult callbacks are stale closures after remount — use this instead.
let _pendingEditorResult: string | null = null

export function consumePendingEditorResult(): string | null {
  const result = _pendingEditorResult
  _pendingEditorResult = null
  return result
}

export function setInkInstance(instance: ReturnType<typeof render>): void {
  _inkInstance = instance
}

/**
 * Yield terminal control to opencode for a specific session.
 * Exits alt screen, unmounts Ink, runs opencode, re-enters alt screen and remounts.
 */
export function yieldToOpencode(sessionId: string, cwd: string, port?: number | null): void {
  if (!_inkInstance) return

  _inkInstance.unmount()
  _inkInstance = null

  // Exit alt screen so opencode gets a clean normal terminal
  if (process.stdout.isTTY) process.stdout.write(EXIT_ALT_SCREEN)

  try {
    const cmd = port
      ? `opencode attach http://localhost:${port} --session ${sessionId} --dir ${cwd}`
      : `opencode -s ${sessionId}`
    execSync(cmd, {
      stdio: "inherit",
      cwd,
    })
  } catch {
    // User quit opencode (Ctrl-C or q) — normal exit, ignore error
  }

  // Re-enter alt screen and remount ocm
  if (process.stdout.isTTY) process.stdout.write(ENTER_ALT_SCREEN + CLEAR_SCREEN)
  _remountOcm()
}

/**
 * Yield terminal control to opencode in a specific directory.
 * Used for spawning new instances — opencode will create/resume session there.
 */
export function yieldToNewOpencode(cwd: string): void {
  if (!_inkInstance) return

  _inkInstance.unmount()
  _inkInstance = null

  if (process.stdout.isTTY) process.stdout.write(EXIT_ALT_SCREEN)

  try {
    execSync("opencode", {
      stdio: "inherit",
      cwd,
    })
  } catch {
    // User quit opencode — normal exit
  }

  if (process.stdout.isTTY) process.stdout.write(ENTER_ALT_SCREEN + CLEAR_SCREEN)
  _remountOcm()
}

/**
 * Open current input text in $EDITOR (Ctrl-X E pattern).
 * Unmounts Ink, opens editor with text in a temp file,
 * reads back the result, remounts OCMux, and calls onResult with the edited text.
 */
export function openInEditor(currentText: string, onResult: (text: string) => void): void {
  if (!_inkInstance) return

  const editor = process.env.EDITOR || process.env.VISUAL || "vi"
  const tmpFile = join(tmpdir(), `ocmux-msg-${Date.now()}.md`)

  _inkInstance.unmount()
  _inkInstance = null

  if (process.stdout.isTTY) process.stdout.write(EXIT_ALT_SCREEN)

  // Write current text to temp file
  try { writeFileSync(tmpFile, currentText) } catch { /* ignore */ }

  // Open editor
  try {
    execSync(`${editor} ${JSON.stringify(tmpFile)}`, { stdio: "inherit" })
  } catch { /* non-zero exit is fine */ }

  // Read back
  let edited = currentText
  try { edited = readFileSync(tmpFile, "utf-8").trimEnd() } catch { /* ignore */ }
  try { unlinkSync(tmpFile) } catch { /* ignore */ }

  if (process.stdout.isTTY) process.stdout.write(ENTER_ALT_SCREEN + CLEAR_SCREEN)

  // Store result in module-level slot — the remounted Conversation component
  // will read this on mount (onResult callback is a stale closure after remount)
  _pendingEditorResult = edited

  // Remount OCMux
  import("../app.js").then(({ App }) => {
    _inkInstance = render(React.createElement(App), { exitOnCtrlC: false })
    setInkInstance(_inkInstance)
  }).catch(console.error)
}

/**
 * Open one or more project files in $EDITOR, unmounting Ink during the edit.
 * Unlike openInEditor, this opens actual file paths — no temp file or content capture.
 */
export function openFileInEditor(filePaths: string | string[]): void {
  if (!_inkInstance) return

  const editor = process.env.EDITOR || process.env.VISUAL || "vi"
  const paths = Array.isArray(filePaths) ? filePaths : [filePaths]
  const quotedPaths = paths.map((p) => JSON.stringify(p)).join(" ")

  _inkInstance.unmount()
  _inkInstance = null

  if (process.stdout.isTTY) process.stdout.write(EXIT_ALT_SCREEN)

  try {
    execSync(`${editor} ${quotedPaths}`, { stdio: "inherit" })
  } catch { /* non-zero exit is fine */ }

  if (process.stdout.isTTY) process.stdout.write(ENTER_ALT_SCREEN + CLEAR_SCREEN)

  // Remount OCMux
  import("../app.js").then(({ App }) => {
    _inkInstance = render(React.createElement(App), { exitOnCtrlC: false })
    setInkInstance(_inkInstance)
  }).catch(console.error)
}

/**
 * Check if opencode binary is available on PATH.
 */
export function isOpencodeAvailable(): boolean {
  try {
    execSync("which opencode", { stdio: "pipe" })
    return true
  } catch {
    return false
  }
}

// Remount Ink after yielding — lazy import to avoid circular deps
function _remountOcm(): void {
  // Use dynamic import to get App without circular dependency
  import("../app.js")
    .then(({ App }) => {
      _inkInstance = render(React.createElement(App), { exitOnCtrlC: false })
      setInkInstance(_inkInstance)
    })
    .catch(console.error)
}
