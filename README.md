# atig

`atig` is a fast, Vim-oriented Git history viewer for reviewing commits from
the terminal. It keeps the history-first workflow and visual style of `tig`,
then adds a focused review flow for changes produced by coding agents.

The project is intentionally small: browse history, inspect a commit, attach
comments to diff lines, and send the complete review to an existing Codex or
Claude session.

## Features

- Familiar `tig`-style history with dates, authors, refs, graph lanes, and
  commit subjects.
- Streaming `git log` output, bounded event batches, and cancellation of stale
  loads to stay responsive in repositories with long histories.
- Vim-style movement, search, and command input.
- Colorized commit diffs with direct file, hunk, page, and line-number jumps.
- Diff-line review comments with a list for editing and deletion.
- Repository-scoped Codex and Claude session discovery with an in-TUI picker.
- Explicitly confirmed, batched delivery to an existing Codex or Claude
  session.
- In-app key reference with `?`.

## Requirements

- Git
- A terminal with color support
- Rust 1.85 or newer when building from source
- Optional: an installed and authenticated `codex` or `claude` CLI for agent
  delivery

## Install from source

```sh
git clone https://github.com/sott0n/atig.git
cd atig
cargo install --path .
```

Alternatively, build and run the local checkout:

```sh
cargo run
```

Run `atig` from inside the Git repository you want to inspect:

```sh
cd path/to/repository
atig
```

## Basic workflow

1. Move through Git history with `j` and `k`.
2. Press `Enter` to open the selected commit.
3. Navigate the diff or press `a` on a changed line to add a review comment.
4. Press `v` to inspect, edit, or delete accumulated comments.
5. Press `s`, select a repository session with `j`/`k`, and press `Enter`.
6. Press `S`, verify the destination, and press `y` to deliver the review.

The selected destination remains visible as
`agent session <provider>:<session>` in the status bar. The active session is
also marked with `*` in the picker.

Press `?` at any time in a normal view to see the key reference.

When adding a comment, the editor opens directly below the selected diff line.
Press `Enter` to save it or `Esc` to cancel. Saved comments remain visible
below their exact diff lines and long comments wrap inside the review block.
The saved comment remains selected, so press `S` next to send that comment to
the selected agent session.

## Key bindings

### History

| Key | Action |
| --- | --- |
| `j`, `Down` | Select the next commit |
| `k`, `Up` | Select the previous commit |
| `Ctrl-d`, `Ctrl-u` | Move 10 commits down or up |
| `gg`, `G` | Select the first or last commit |
| `Enter` | Open the selected commit diff |
| `/` | Search commit metadata and subjects |
| `n`, `N` | Repeat the search forward or backward |
| `r` | Reload history |
| `s` | Open the repository agent-session picker |
| `:` | Open command input |
| `?` | Open the key reference |
| `q` | Quit `atig` |

### Commit diff

| Key | Action |
| --- | --- |
| `j`, `Down` | Move to the next diff line or inline comment |
| `k`, `Up` | Move to the previous diff line or inline comment |
| `J`, `K` | Move down or up 10 lines |
| `Ctrl-d`, `Ctrl-u` | Move down or up half a screen |
| `Ctrl-f`, `Ctrl-b` | Move down or up a full screen |
| `PageDown`, `PageUp` | Move down or up a full screen |
| `gg`, `G` | Jump to the beginning or end |
| `Home`, `End` | Jump to the beginning or end |
| `]f`, `[f` | Jump to the next or previous changed file |
| `Tab`, `Shift-Tab` | Jump to the next or previous changed file |
| `]c`, `[c` | Jump to the next or previous diff hunk |
| `}`, `{` | Jump to the next or previous diff hunk |
| `]i`, `[i` | Jump to the next or previous Agent Intent |
| `/` | Search diff text |
| `n`, `N` | Repeat the search forward or backward |
| `a` | Add a comment to the selected diff line |
| `V`, then `j`/`k`, `a` | Select a line range and add a range comment |
| `F` | Add a comment for the current file |
| `C` | Add a comment for the whole commit |
| `Enter` | Edit a review comment, or reply to a selected thread message |
| `v` | Open the review comment list |
| `t` | Open the changed-file navigator (`Δ` Agent change, `◆` Intent count, `●` review count) |
| `i` | Show or hide Agent Intent cards |
| `S` | Send the selected inline comment, or all comments when a code line is selected |
| `X` | Open the working-tree diff produced by the last Agent run |
| `s` | Open the repository agent-session picker |
| `r` | Reload the current commit diff |
| `:` | Open command input |
| `Esc`, `q` | Return to history |

### Review comments

| Key | Action |
| --- | --- |
| `j`, `Down` | Select the next comment |
| `k`, `Up` | Select the previous comment |
| `e` | Edit the selected comment |
| `d` | Delete the selected comment |
| `S` | Confirm and send all comments |
| `s` | Open the repository agent-session picker |
| `Esc`, `q` | Return to the diff |

Comments are anchored to a commit, file, line, or line range. Review threads
and captured Agent responses are persisted under `.git/atig/`.

## Commands

| Command | Action |
| --- | --- |
| `:log <revision...>` | Reload history using a Git revision specification |
| `:reload` | Reload Git history |
| `:goto <line>` | Jump to a one-based line number in the diff |
| `:next-file`, `:prev-file` | Jump between changed files |
| `:next-change`, `:prev-change` | Jump between diff hunks |
| `:session codex <id>` | Select an existing Codex session |
| `:session claude <id>` | Select an existing Claude session |
| `:session other <id>` | Select an extension target; delivery requires a custom adapter |
| `:session clear` | Clear the selected agent session |
| `:sessions` | Discover and select sessions associated with this repository |
| `:worktree` | Review unstaged working-tree changes |
| `:staged` | Review staged changes |
| `:help` | Open the key reference |
| `:q`, `:quit` | Quit `atig` |

Git revision arguments are passed directly to `git log`. For example:

```text
:log --all
:log main..feature
:log --author Alice --since 2.weeks
```

## Agent review delivery

`atig` currently includes adapters for Codex CLI and Claude Code:

- Pressing `s` discovers sessions associated with the current Git repository.
- Codex sessions are read through the official app-server `thread/list`
  protocol and matched using Git origin or working directory.
- Claude sessions are matched by reading only session ID, working directory,
  and file modification time from the local Claude project history.
- Codex delivery forks the selected interactive session through
  `codex exec fork`, then runs the review in that child session. This avoids
  trying to resume an active app thread as a non-interactive exec session.
- Claude delivery resumes the target through non-interactive
  `claude --resume` with unattended permission prompts denied.
- `other` is reserved for future/custom adapters and does not execute a
  command today.

When an inline comment is selected, `S` sends that comment alone. When a code
line is selected, `S` sends all accumulated comments as one review request.
Comments can target a specific line (`a`), a line range (`V`, move, `a`), the
current file (`F`), or the whole commit (`C`); that scope is included
explicitly in the Agent request together with bounded diff context.
The request asks the target agent to use a sub-agent when supported, make only
the reviewed changes, run relevant tests, and avoid committing or pushing.
Sub-agent creation is ultimately controlled by the target agent and its
configuration.

After the CLI finishes, its response is appended to the inline review thread.
For code changes, atig asks the Agent for a structured intent for each logical
change. Open the resulting working-tree diff with `X`; each reported intent is
shown directly below its first added or modified line as `◆ AGENT INTENT`.
atig only displays intentions explicitly returned by the Agent and does not
guess when the response omits the intent manifest or returns a malformed one.
Use `]i`/`[i` to jump between reported intentions, `i` to show or hide their
inline cards, and `t` to see the Intent count for each changed file.
Use `j`/`k` to select the Agent response, press `Enter` to write a follow-up,
then press `S` on that thread to send the complete conversation context back
to the selected Agent session.
While the Agent is running, output is reflected in the status line; press
`Esc` to request cancellation. On completion, atig compares the working tree
with its pre-send snapshot and appends the changed-file verification to the
Agent response. Press `X` to open those changes as a working-tree diff.

Review threads are restored across `atig` runs from
`.git/atig/reviews.json`. Saved line and range reviews are re-anchored from
their file and line scope when the diff is loaded again.

Delivery is deliberately conservative:

- A destination must be configured explicitly.
- Session IDs accept only letters, numbers, `-`, `_`, `.`, and `:`.
- `S` always shows a destination confirmation before delivery.
- Send progress and CLI errors are shown in the diff or comment-list status
  bar instead of being hidden behind the normal navigation status.
- Agent CLIs are launched without a shell and without sandbox or approval
  bypass flags.
- Comments remain visible after delivery, and responses are attached only to
  the exact comment snapshot that was sent.
- Only one delivery can run at a time.

The selected CLI must already be installed, authenticated, and able to resolve
the supplied session ID.

## Performance model

History and diff output are produced by background `git` processes. `atig`
renders the first available results instead of waiting for the complete
history, consumes results in bounded batches so input remains responsive, and
cancels the previous load when a view is reloaded. The complete history is
still streamed; there is no artificial commit limit.

## Current limitations

- Claude's non-interactive CLI may buffer output even though atig consumes
  Agent output incrementally.
- The generic `other` agent adapter is not implemented.
- The Git graph targets the common `tig` experience but is not yet guaranteed
  to reproduce every complex octopus or criss-cross graph exactly.

## Development

```sh
cargo run
cargo test
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
```

The source is split by responsibility:

- `src/git.rs`: streaming Git history and commit diffs
- `src/app.rs`: application state, navigation, comments, and commands
- `src/ui.rs`: `ratatui` rendering and colors
- `src/agent.rs`: validated session targets and delivery adapters
- `src/review_store.rs`: repository-local review thread persistence
- `src/main.rs`: terminal lifecycle and key event routing

## License

MIT
