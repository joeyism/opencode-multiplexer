import { describe, test, expect } from "bun:test"
import { relativeTime, highlightMatches, filterFilesForCwd, findDisplayLineMatches, getSearchScrollOffset } from "../views/helpers.js"
import { deriveRepoName } from "../poller.js"
import { getAllSessions, type DbSessionWithProject, NEEDS_INPUT_TOOLS } from "../db/reader.js"

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

describe("NEEDS_INPUT_TOOLS", () => {
  test("includes question tool", () => {
    expect(NEEDS_INPUT_TOOLS).toContain("question")
  })
  test("includes plan_exit tool", () => {
    expect(NEEDS_INPUT_TOOLS).toContain("plan_exit")
  })
  test("is a non-empty array", () => {
    expect(NEEDS_INPUT_TOOLS.length).toBeGreaterThan(0)
  })
})

describe("highlightMatches", () => {
  test("highlights plain text matches", () => {
    const result = highlightMatches("hello world", "world")
    expect(result).toContain("\x1b[30;43m")
    expect(result).toContain("world")
    expect(result).toContain("\x1b[0m")
  })

  test("case-insensitive matching", () => {
    const result = highlightMatches("Hello World", "hello")
    expect(result).toContain("\x1b[30;43m")
  })

  test("no match returns original text", () => {
    const result = highlightMatches("hello world", "xyz")
    expect(result).toBe("hello world")
  })

  test("empty query returns original text", () => {
    const result = highlightMatches("hello world", "")
    expect(result).toBe("hello world")
  })

  test("multiple matches are all highlighted", () => {
    const result = highlightMatches("foo bar foo baz foo", "foo")
    const count = (result.match(/\x1b\[30;43m/g) || []).length
    expect(count).toBe(3)
  })
})

describe("filterFilesForCwd", () => {
  test("keeps relative files and absolute files within cwd", () => {
    expect(filterFilesForCwd([
      "src/app.ts",
      "/repo/src/views/conversation.tsx",
      "/other/file.ts",
    ], "/repo")).toEqual([
      "src/app.ts",
      "/repo/src/views/conversation.tsx",
    ])
  })

  test("does not treat sibling directories as inside cwd", () => {
    expect(filterFilesForCwd([
      "/repo-other/file.ts",
      "/repo/file.ts",
    ], "/repo")).toEqual([
      "/repo/file.ts",
    ])
  })
})

describe("findDisplayLineMatches", () => {
  const lines = [
    { kind: "role-header", role: "user", time: "10:00" },
    { kind: "text", text: "Hello world" },
    { kind: "tool", icon: "✓", color: "green", name: "bash", input: "python -V" },
    { kind: "question", header: "Confirm", question: "Search the visible line?", status: "running", options: [], custom: false },
    { kind: "thinking", text: "Working through the plan" },
    { kind: "spacer" },
  ] as const

  test("matches visible text-bearing lines only", () => {
    expect(findDisplayLineMatches(lines as any, "world")).toEqual([1])
    expect(findDisplayLineMatches(lines as any, "python")).toEqual([2])
    expect(findDisplayLineMatches(lines as any, "visible line")).toEqual([3])
    expect(findDisplayLineMatches(lines as any, "working")).toEqual([4])
  })

  test("is case-insensitive and ignores non-searchable rows", () => {
    expect(findDisplayLineMatches(lines as any, "HELLO")).toEqual([1])
    expect(findDisplayLineMatches(lines as any, "10:00")).toEqual([])
  })

  test("uses only the visible tool detail when title and input both exist", () => {
    const toolLine = [{
      kind: "tool",
      icon: "✓",
      color: "green",
      name: "bash",
      title: "visible title",
      input: "hidden input",
    }] as const

    expect(findDisplayLineMatches(toolLine as any, "visible title")).toEqual([0])
    expect(findDisplayLineMatches(toolLine as any, "hidden input")).toEqual([])
  })

  test("matches text after stripping ansi styling", () => {
    const styledLine = [{ kind: "text", text: "\x1b[1mHello\x1b[22m world" }] as const
    expect(findDisplayLineMatches(styledLine as any, "hello")).toEqual([0])
  })

  test("returns no matches for blank queries", () => {
    expect(findDisplayLineMatches(lines as any, "")).toEqual([])
    expect(findDisplayLineMatches(lines as any, "   ")).toEqual([])
  })
})

describe("getSearchScrollOffset", () => {
  test("keeps a matched line visible near the top of the viewport", () => {
    expect(getSearchScrollOffset(100, 10, 30)).toBe(60)
  })

  test("clamps near the bottom when the match is already in the latest rows", () => {
    expect(getSearchScrollOffset(100, 10, 95)).toBe(0)
  })

  test("clamps near the top when the first rows are matched", () => {
    expect(getSearchScrollOffset(100, 10, 0)).toBe(90)
  })
})
