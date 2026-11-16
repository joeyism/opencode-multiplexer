---
name: ocmux-playbook
description: Create OCMux playbook YAML files that automate multi-agent workflows. Use when the user wants to automate a sequence of agent prompts, shell checks, and control flow across OpenCode agents (chat, plan, build, orchestrator) within OCMux.
---

# OCMux Playbook Author

Write YAML playbooks that OCMux runs against the current OpenCode session.

## Where to write

`~/.config/ocmux/playbooks/<kebab-name>.yaml`

The filename (without `.yaml`) is the playbook id shown in the picker.

## Schema

```yaml
name: pr-comments
description: Review, plan, fix, resolve PR comments
params:
  GITHUB_PR_URL:
    prompt: GitHub PR URL
    required: true
  BRANCH:
    prompt: Branch name
    required: false
    default: main
steps:
  - run: |
      gh api "$GITHUB_PR_URL" --jq '.comments | length'
    register: COMMENTS

  - until: test "$COMMENTS" = "0"
    max: 5
    steps:
      - send:
          agent: chat
          message: |
            review comments from ${GITHUB_PR_URL}
            don't fix anything, just summarize it and tell me what they mean, and are they worth fixing
      - send:
          agent: plan
          message: make a plan to fix this
      - send:
          agent: build
          message: Proceed with implementation
      - send:
          agent: orchestrator
          message: reply in thread and resolve comments
      - run: |
          gh api "$GITHUB_PR_URL" --jq '.comments | length'
        register: COMMENTS
```

## Allowed keys

| Key | Type | Description |
|-----|------|-------------|
| `name` | string | Display name (required) |
| `description` | string | One-line summary (optional) |
| `params` | map | Parameters the user fills in at runtime |
| `steps` | list | Steps to execute (required) |

### Params

Each param key is the variable name. Value fields:
- `prompt` (string, required): label shown in the form
- `required` (bool, default `true`): if true, form won't submit until filled
- `default` (string, optional): pre-filled value

### Steps

Each step is one of `send`, `run`, `until`, or `if`.

#### `send`

```yaml
- send:
    agent: chat          # OpenCode agent name
    message: |
      Prompt text with ${PARAMS} interpolated
    timeout_secs: 1800   # optional, default 30 minutes
```

Sends a prompt to the session as the named agent via OpenCode serve HTTP API. Blocks until the turn finishes.

#### `run`

```yaml
- run: |
    gh api "$GITHUB_PR_URL" --jq '.comments | length'
  register: COMMENTS
```

Runs a bash script in the session's working directory. Exit code must be 0 (otherwise the playbook fails). `register: NAME` stores trimmed stdout as a variable for later steps.

#### `until` (loop)

```yaml
- until: test "$COMMENTS" = "0"
  max: 5
  steps:
    - send: ...
    - run: ...
```

Bash-like semantics: **checks the condition first**. If exit code is 0, the loop body is skipped. Otherwise the body runs and the check repeats. After `max` body executions, the playbook fails with "loop limit reached". `max` is **required**.

#### `if` (conditional)

```yaml
- if: test -n "$COMMENTS"
  then:
    - send: ...
  else:                 # optional
    - send: ...
```

Runs the shell condition. Exit 0 → `then` branch. Non-zero → `else` branch. The condition itself never fails the playbook.

## Variable rules

- `${VAR}` in `message:` is **string interpolation** (replaced before sending)
- `$VAR` and `"$VAR"` in `run`/`if`/`until` are **shell environment variables** (passed as env, never interpolated)
- Built-in env vars: `SESSION_ID`, `SESSION_CWD`
- `register: NAME` makes trimmed stdout available as `$NAME` and `${NAME}`
- **Never** splice untrusted input into `run` strings — always use `"$VAR"`

## Agent names

Use OpenCode agent names: `chat`, `plan`, `build`, `orchestrator`, or custom agents defined in the user's OpenCode config. Don't invent agent names.

## How the user runs it

1. Press `p` in OCMux to open the playbook picker
2. Fuzzy-search and select a playbook
3. Fill in params (if any) in the form overlay
4. Press Enter to start
5. The playbook runs in the background; the footer shows progress
6. Press `p` again to open the abort panel if needed

Sessions must be managed (have a `serve` port) to run playbooks.

## Example: PR review loop

```yaml
name: pr-review
description: Check for PR comments, review, fix, and resolve
params:
  GITHUB_PR_URL:
    prompt: GitHub PR URL
    required: true
steps:
  - run: |
      gh api "repos/$(basename $(dirname "$GITHUB_PR_URL"))/pulls/$(basename "$GITHUB_PR_URL")/comments" --jq 'length' 2>/dev/null || echo 0
    register: COMMENT_COUNT
  - if: test "$COMMENT_COUNT" -gt 0
    then:
      - send:
          agent: chat
          message: |
            review comments from ${GITHUB_PR_URL}
            don't fix anything, just summarize it and tell me what they mean, and are they worth fixing
      - send:
          agent: plan
          message: make a plan to fix this
      - send:
          agent: build
          message: Proceed with implementation
      - send:
          agent: orchestrator
          message: reply in thread and resolve comments
    else:
      - send:
          agent: chat
          message: No review comments found on ${GITHUB_PR_URL}. Nothing to do.
```
