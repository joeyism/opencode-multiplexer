declare module "marked-terminal" {
  import type { Extension } from "marked"

  interface MarkedTerminalOptions {
    width?: number
    reflowText?: boolean
  }

  export function markedTerminal(options?: MarkedTerminalOptions): Extension
}
