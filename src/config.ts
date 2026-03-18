import { readFileSync } from "fs"
import { homedir } from "os"
import { join } from "path"

export interface KeybindingsConfig {
  dashboard: {
    up: string
    down: string
    open: string
    attach: string
    spawn: string
    expand: string
    collapse: string
    nextNeedsInput: string
    kill: string
    quit: string
    help: string
    rescan: string
  }
  conversation: {
    back: string
    attach: string
    send: string
    scrollUp: string
    scrollDown: string
    scrollHalfPageUp: string
    scrollHalfPageDown: string
    scrollPageUp: string
    scrollPageDown: string
    scrollBottom: string
    scrollTop: string
  }
  spawn: {
    cancel: string
    confirm: string
  }
}

export interface Config {
  keybindings: KeybindingsConfig
  pollIntervalMs: number
  dbPath: string
}

const DEFAULTS: Config = {
  keybindings: {
    dashboard: {
      up: "k",
      down: "j",
      open: "return",
      attach: "a",
      spawn: "n",
      expand: "tab",
      collapse: "shift-tab",
      nextNeedsInput: "ctrl-n",
      kill: "x",
      quit: "q",
      help: "?",
      rescan: "r",
    },
    conversation: {
      back: "escape",
      attach: "a",
      send: "return",
      scrollUp: "k",
      scrollDown: "j",
      scrollHalfPageUp: "ctrl-u",
      scrollHalfPageDown: "ctrl-d",
      scrollPageUp: "ctrl-b",
      scrollPageDown: "ctrl-f",
      scrollBottom: "G",
      scrollTop: "g",
    },
    spawn: {
      cancel: "escape",
      confirm: "return",
    },
  },
  pollIntervalMs: 2000,
  dbPath: join(homedir(), ".local", "share", "opencode", "opencode.db"),
}

function deepMerge<T extends object>(defaults: T, overrides: Partial<T>): T {
  const result = { ...defaults }
  for (const key of Object.keys(overrides) as Array<keyof T>) {
    const val = overrides[key]
    if (val !== undefined && val !== null) {
      if (
        typeof val === "object" &&
        !Array.isArray(val) &&
        typeof defaults[key] === "object"
      ) {
        result[key] = deepMerge(defaults[key] as object, val as object) as T[keyof T]
      } else {
        result[key] = val as T[keyof T]
      }
    }
  }
  return result
}

function loadConfig(): Config {
  const configPath = join(
    homedir(),
    ".config",
    "ocmux",
    "config.json",
  )
  try {
    const raw = readFileSync(configPath, "utf-8")
    const parsed = JSON.parse(raw) as Partial<Config>
    return deepMerge(DEFAULTS, parsed)
  } catch {
    return DEFAULTS
  }
}

export const config = loadConfig()
