#!/usr/bin/env bun
import React from "react"
import { render } from "ink"
import { App } from "./app.js"
import { setInkInstance } from "./hooks/use-attach.js"
import { startPoller, stopPoller } from "./poller.js"
import { config } from "./config.js"
import { cleanDeadInstances } from "./registry/instances.js"

// Enter alternate screen buffer — keeps our TUI isolated from terminal history.
// On resize, we clear the alternate screen so Ink always redraws from a clean slate.
const ENTER_ALT_SCREEN = "\x1b[?1049h"
const EXIT_ALT_SCREEN  = "\x1b[?1049l"
const CLEAR_SCREEN     = "\x1b[2J\x1b[H"

function enterAltScreen() {
  process.stdout.write(ENTER_ALT_SCREEN)
}

function exitAltScreen() {
  process.stdout.write(EXIT_ALT_SCREEN)
}

function cleanup() {
  stopPoller()
  exitAltScreen()
}

async function main() {
  enterAltScreen()

  // On resize: clear the alternate screen so Ink redraws without stale lines
  if (process.stdout.isTTY) {
    process.stdout.on("resize", () => {
      process.stdout.write(CLEAR_SCREEN)
    })
  }

  // Remove stale spawned instances (dead pids / unresponsive ports)
  await cleanDeadInstances()

  startPoller(config.pollIntervalMs)

  const inkInstance = render(<App />, { exitOnCtrlC: false })
  setInkInstance(inkInstance)
}

process.on("SIGINT", () => { cleanup(); process.exit(0) })
process.on("SIGTERM", () => { cleanup(); process.exit(0) })
process.on("exit", () => { exitAltScreen() })

main().catch(console.error)
