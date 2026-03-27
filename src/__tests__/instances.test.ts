import { describe, test, expect, mock, beforeEach } from "bun:test"
import { findNextPort } from "../registry/instances.js"
import * as fs from "fs"

// Mock the dependencies used by findNextPort
mock.module("../registry/instances.js", () => ({
  ...require("../registry/instances.js"),
  loadSpawnedInstances: mock(() => []),
  saveSpawnedInstances: mock(() => {}),
  isPidAlive: mock(() => true),
  isPortAlive: mock(async () => false),
}))

// We need to re-import after mocking
import { 
  loadSpawnedInstances, 
  saveSpawnedInstances, 
  isPidAlive, 
  isPortAlive 
} from "../registry/instances.js"

describe("findNextPort", () => {
  beforeEach(() => {
    mock.restore()
  })

  test("returns first available port when none are in use", async () => {
    // @ts-ignore
    loadSpawnedInstances.mockReturnValue([])
    // @ts-ignore
    isPortAlive.mockResolvedValue(false)

    const port = await findNextPort(4000, 4005)
    expect(port).toBe(4000)
  })

  test("skips ports claimed by live processes", async () => {
    // @ts-ignore
    loadSpawnedInstances.mockReturnValue([
      { port: 4000, pid: 123, cwd: "/tmp", sessionId: null }
    ])
    // @ts-ignore
    isPidAlive.mockImplementation((pid: number) => pid === 123)
    // @ts-ignore
    isPortAlive.mockResolvedValue(false)

    const port = await findNextPort(4000, 4005)
    expect(port).toBe(4001)
  })

  test("uses port if process is dead (self-healing)", async () => {
    // @ts-ignore
    loadSpawnedInstances.mockReturnValue([
      { port: 4000, pid: 999, cwd: "/tmp", sessionId: null }
    ])
    // @ts-ignore
    isPidAlive.mockImplementation((pid: number) => pid !== 999) // 999 is dead
    // @ts-ignore
    isPortAlive.mockResolvedValue(false)

    const port = await findNextPort(4000, 4005)
    expect(port).toBe(4000)
    
    // Should have saved the pruned list
    expect(saveSpawnedInstances).toHaveBeenCalled()
    const saved = (saveSpawnedInstances as any).mock.calls[0][0]
    expect(saved).toEqual([])
  })

  test("throws error if all ports are truly in use", async () => {
    // @ts-ignore
    loadSpawnedInstances.mockReturnValue([])
    // @ts-ignore
    isPortAlive.mockResolvedValue(true) // Something else is listening on all ports

    expect(findNextPort(4000, 4001)).rejects.toThrow("No available ports in range 4000-4001")
  })

  test("prunes dead PIDs even if all ports are taken", async () => {
    // @ts-ignore
    loadSpawnedInstances.mockReturnValue([
      { port: 4000, pid: 111, cwd: "/tmp", sessionId: null },
      { port: 4001, pid: 222, cwd: "/tmp", sessionId: null }
    ])
    // @ts-ignore
    isPidAlive.mockReturnValue(false) // Both dead
    // @ts-ignore
    isPortAlive.mockResolvedValue(true) // But something else (non-ocmux) is on those ports

    try {
      await findNextPort(4000, 4001)
    } catch (e) {
      // Expected
    }

    // Should still have pruned instances.json
    expect(saveSpawnedInstances).toHaveBeenCalledWith([])
  })
})
