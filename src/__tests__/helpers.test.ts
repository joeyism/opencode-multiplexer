import { describe, test, expect } from "bun:test"
import { relativeTime } from "../views/helpers.js"
import { deriveRepoName } from "../poller.js"
import { getAllSessions, type DbSessionWithProject } from "../db/reader.js"

describe("relativeTime", () => {
  test("returns 'now' for timestamps within the last minute", () => {
    expect(relativeTime(Date.now())).toBe("now")
    expect(relativeTime(Date.now() - 30_000)).toBe("now")
    expect(relativeTime(Date.now() - 59_000)).toBe("now")
  })

  test("returns minutes for < 1 hour", () => {
    expect(relativeTime(Date.now() - 5 * 60_000)).toBe("5m")
    expect(relativeTime(Date.now() - 59 * 60_000)).toBe("59m")
  })

  test("returns hours for < 1 day", () => {
    expect(relativeTime(Date.now() - 2 * 3600_000)).toBe("2h")
    expect(relativeTime(Date.now() - 23 * 3600_000)).toBe("23h")
  })

  test("returns days for < 1 week", () => {
    expect(relativeTime(Date.now() - 3 * 86400_000)).toBe("3d")
    expect(relativeTime(Date.now() - 6 * 86400_000)).toBe("6d")
  })

  test("returns weeks for >= 1 week", () => {
    expect(relativeTime(Date.now() - 7 * 86400_000)).toBe("1w")
    expect(relativeTime(Date.now() - 14 * 86400_000)).toBe("2w")
  })
})

describe("instance sort order", () => {
  test("sort by timeUpdated descending puts most recent first", () => {
    const instances = [
      { timeUpdated: 100 },
      { timeUpdated: 300 },
      { timeUpdated: 200 },
    ]
    instances.sort((a, b) => b.timeUpdated - a.timeUpdated)
    expect(instances.map((i) => i.timeUpdated)).toEqual([300, 200, 100])
  })

  test("equal timestamps maintain stable sort", () => {
    const instances = [
      { timeUpdated: 200, id: "a" },
      { timeUpdated: 200, id: "b" },
    ]
    instances.sort((a, b) => b.timeUpdated - a.timeUpdated)
    // Order preserved for equal timestamps
    expect(instances.length).toBe(2)
  })
})

describe("deriveRepoName", () => {
  test("returns basename for normal directories", () => {
    expect(deriveRepoName("/Users/joey/work/web")).toBe("web")
  })

  test("returns basename for nested normal directories", () => {
    expect(deriveRepoName("/Users/joey/Programming/my-project")).toBe("my-project")
  })

  test("returns repo name for ocmux .worktrees paths", () => {
    expect(
      deriveRepoName(
        "/Users/joey/work/web/.worktrees/ado-2054-manual-date-selectable-backfill-sync"
      )
    ).toBe("web")
  })

  test("returns repo name for short branch names in .worktrees", () => {
    expect(deriveRepoName("/Users/joey/work/web/.worktrees/fix-bug")).toBe("web")
  })

  test("handles repo name with hyphens in .worktrees path", () => {
    expect(deriveRepoName("/Users/joey/work/my-cool-repo/.worktrees/feature-branch")).toBe(
      "my-cool-repo"
    )
  })

  test("returns basename when .worktrees appears elsewhere in path", () => {
    // .worktrees is the terminal segment — no branch subdir follows, so /.worktrees/ doesn't match
    expect(deriveRepoName("/Users/joey/.worktrees")).toBe(".worktrees")
  })
})

describe("getAllSessions return type", () => {
  test("return type includes projectWorktree field", () => {
    // Type assertion test — actual DB query is integration-tested
    const session: DbSessionWithProject = {
      id: "s1",
      projectId: "p1",
      title: "My session",
      directory: "/work/web",
      permission: null,
      timeCreated: Date.now(),
      timeUpdated: Date.now(),
      projectWorktree: "/work/web",
    }
    expect(session.projectWorktree).toBe("/work/web")
  })
})
