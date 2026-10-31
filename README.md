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

Press `?` at any time in a normal view to see the key reference.

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
| `j`, `Down` | Move down one line |
| `k`, `Up` | Move up one line |
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
| `/` | Search diff text |
| `n`, `N` | Repeat the search forward or backward |
| `a` | Add a comment to the selected diff line |
| `v` | Open the review comment list |
| `S` | Confirm and send all review comments |
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

Comments are anchored to the selected commit, file, and diff line. They are
kept in memory for the current `atig` process; quitting the application
discards unsent comments.

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
- Codex delivery resumes the target through `codex exec resume`.
- Claude delivery resumes the target through non-interactive
  `claude --resume` with unattended permission prompts denied.
- `other` is reserved for future/custom adapters and does not execute a
  command today.

All accumulated comments are formatted into one review request. The request
asks the target agent to use a sub-agent when supported, make only the reviewed
changes, run relevant tests, and avoid committing or pushing. Sub-agent
creation is ultimately controlled by the target agent and its configuration.

Delivery is deliberately conservative:

- A destination must be configured explicitly.
- Session IDs accept only letters, numbers, `-`, `_`, `.`, and `:`.
- `S` always shows a destination confirmation before delivery.
- Agent CLIs are launched without a shell and without sandbox or approval
  bypass flags.
- Comments are cleared only after successful delivery and remain available if
  the adapter reports an error.
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

- Only committed changes can be reviewed; working-tree and staged diffs do not
  yet have dedicated views.
- Review comments are not persisted between `atig` processes.
- Agent output is not streamed into the TUI; the status bar reports delivery
  success or failure.
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
- `src/main.rs`: terminal lifecycle and key event routing

## License

MIT
