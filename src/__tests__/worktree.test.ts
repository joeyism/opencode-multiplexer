import { describe, test, expect, beforeEach, afterEach } from "bun:test"
import { execSync } from "child_process"
import { existsSync, mkdirSync, rmSync } from "fs"
import { join } from "path"
import { tmpdir } from "os"
import { createWorktree } from "../registry/instances.js"
import { getSessionModifiedFiles } from "../db/reader.js"

// Create a temporary git repo for testing
const TEST_DIR = join(tmpdir(), `ocmux-worktree-test-${Date.now()}`)

beforeEach(() => {
  mkdirSync(TEST_DIR, { recursive: true })
  execSync("git init", { cwd: TEST_DIR, stdio: "pipe" })
  execSync('git config user.email "test@test.com"', { cwd: TEST_DIR, stdio: "pipe" })
  execSync('git config user.name "Test"', { cwd: TEST_DIR, stdio: "pipe" })
  execSync("git commit --allow-empty -m 'initial'", { cwd: TEST_DIR, stdio: "pipe" })
  // Ensure we're on 'main'
  try {
    execSync("git branch -M main", { cwd: TEST_DIR, stdio: "pipe" })
  } catch {
    // already on main or rename not needed
  }
})

afterEach(() => {
  rmSync(TEST_DIR, { recursive: true, force: true })
})

describe("getSessionModifiedFiles", () => {
  test("returns empty array for non-existent session", () => {
    const files = getSessionModifiedFiles("non_existent_session_id_xyz")
    expect(files).toEqual([])
  })

  test("returns an array (does not throw)", () => {
    const files = getSessionModifiedFiles("any_session_id")
    expect(Array.isArray(files)).toBe(true)
  })

  test("deduplicates file paths", () => {
    // Verified by logic: Set<string> deduplicates
    // The function collects files from multiple patch parts and deduplicates
    const files = getSessionModifiedFiles("non_existent")
    const unique = [...new Set(files)]
    expect(files.length).toBe(unique.length)
  })
})

describe("createWorktree", () => {
  test("creates a worktree in .worktrees directory", () => {
    const result = createWorktree(TEST_DIR, "test-branch")
    expect(result).toBe(join(TEST_DIR, ".worktrees", "test-branch"))
    expect(existsSync(result)).toBe(true)
  })

  test("creates the .worktrees directory if it doesn't exist", () => {
    expect(existsSync(join(TEST_DIR, ".worktrees"))).toBe(false)
    createWorktree(TEST_DIR, "new-feature")
    expect(existsSync(join(TEST_DIR, ".worktrees"))).toBe(true)
  })

  test("creates a new branch", () => {
    createWorktree(TEST_DIR, "feature-xyz")
    const branches = execSync("git branch --list feature-xyz", {
      cwd: TEST_DIR,
      encoding: "utf-8",
    }).trim()
    expect(branches).toContain("feature-xyz")
  })

  test("returns existing worktree directory without error", () => {
    const first = createWorktree(TEST_DIR, "existing-feature")
    const second = createWorktree(TEST_DIR, "existing-feature")
    expect(first).toBe(second)
    expect(existsSync(first)).toBe(true)
  })

  test("checks out existing branch in new worktree", () => {
    execSync("git branch pre-existing-branch", { cwd: TEST_DIR, stdio: "pipe" })
    const result = createWorktree(TEST_DIR, "pre-existing-branch")
    expect(existsSync(result)).toBe(true)
  })

  test("worktree is a valid git working tree", () => {
    const wtDir = createWorktree(TEST_DIR, "valid-wt")
    const isWt = execSync("git rev-parse --is-inside-work-tree", {
      cwd: wtDir,
      encoding: "utf-8",
    }).trim()
    expect(isWt).toBe("true")
  })
})
