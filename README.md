# atig

`atig` is a fast, Vim-oriented Git history viewer for reading and reviewing
code written by coding agents. It keeps the history-first feel of `tig`, then
adds inline review threads, Agent delivery, and guided code reading.

## Highlights

- Familiar `tig`-style history with streaming output for large repositories
- Vim navigation, search, jumps, and command input
- Syntax-highlighted diffs and read-only surrounding source
- Line, range, file, and commit review comments
- Repository-scoped Codex and Claude session discovery
- Inline Agent responses, change intents, and follow-up conversations
- Guided reading paths that explain why changed and related code matters

## Install

Download a Linux or macOS archive from
[GitHub Releases](https://github.com/sott0n/atig/releases), then install the
binary on your `PATH`:

```sh
tar -xzf atig-vVERSION-TARGET.tar.gz
install -m 755 atig-vVERSION-TARGET/atig ~/.local/bin/atig
```

Or install from source with Rust 1.85 or newer:

```sh
git clone https://github.com/sott0n/atig.git
cd atig
cargo install --path .
```

`atig` requires Git and a color terminal. Agent integration additionally
requires an installed and authenticated `codex` or `claude` CLI.

## Quick start

Run `atig` inside a Git repository:

```sh
cd path/to/repository
atig
```

1. Select a commit with `j`/`k` and open it with `Enter`.
2. Move through the diff using Vim keys, `]f`/`[f` for files, and `]c`/`[c`
   for hunks.
3. Press `Enter` on code to inspect its surrounding source.
4. Press `s` to select a repository Agent session.
5. Add a review with `a`, then send it with `S`.

Press `?` in the application for the built-in key reference.

## Agent workflows

### Review Agent-written changes

Comments appear directly below their diff location and persist under
`.git/atig/`. Use:

- `a` for the selected line
- `V`, move, then `a` for a line range
- `F` for the current file
- `C` for the whole commit
- `S` to confirm and send the selected comment, or all comments when code is
  selected

After the Agent finishes, its response is attached to the review thread.
Select the response and press `Enter` to reply, then `S` to continue the same
conversation. If the Agent changed files, `X` opens the resulting working-tree
diff. Structured explanations returned by the Agent appear as `AGENT INTENT`
cards; use `[i`/`]i` to navigate them and `i` to hide or show them.

### Understand the code

Press `Enter` on a diff line to open the surrounding source in Context View.
From there:

- `Q` asks the selected Agent about the current line
- `V`, move, then `Q` asks about a selected block
- `R` creates an ordered reading path through related definitions, callers,
  state, and tests
- `[r`/`]r` moves through Agent references and reading-path locations
- `T` expands or collapses the complete reading path
- `Ctrl-o`/`Ctrl-i` moves backward or forward through visited code

Reading-path destinations show `WHY THIS CODE` and temporary `// ↓ L42`
annotations immediately before the lines they explain. These overlays do not
modify source files.

### Select an Agent session

Press `s` to discover Codex and Claude sessions associated with the current
repository. The selected destination remains visible in the status bar.
Sessions can also be selected explicitly:

```text
:session codex <id>
:session claude <id>
:session clear
```

Codex delivery forks the selected interactive thread. Claude delivery resumes
the selected session non-interactively with unattended permission prompts
denied. The generic `other` adapter is reserved for custom integrations and
does not execute commands yet.

## Keys

### History

| Key | Action |
| --- | --- |
| `j`/`k`, arrows | Move between commits |
| `Ctrl-d`/`Ctrl-u` | Move 10 commits |
| `gg`/`G` | First/last commit |
| `Enter` | Open commit diff |
| `/`, `n`/`N` | Search and repeat |
| `s` | Select Agent session |
| `r` | Reload |
| `:`, `?`, `q` | Command, help, quit |

### Diff

| Key | Action |
| --- | --- |
| `j`/`k`, arrows | Move by line or inline thread |
| `J`/`K` | Move 10 lines |
| `Ctrl-d`/`Ctrl-u` | Half-page movement |
| `Ctrl-f`/`Ctrl-b`, `PageDown`/`PageUp` | Full-page movement |
| `gg`/`G`, `Home`/`End` | Beginning/end |
| `[f`/`]f`, `Shift-Tab`/`Tab` | Previous/next file |
| `[c`/`]c`, `{`/`}` | Previous/next hunk |
| `[i`/`]i` | Previous/next Agent Intent |
| `/`, `n`/`N` | Search and repeat |
| `Enter` | Open code, edit review, or reply |
| `a`, `V`…`a`, `F`, `C` | Add line, range, file, or commit review |
| `S` | Send selected review or all reviews |
| `R` | Open code and create an Agent reading path |
| `v`, `t` | Review list, changed-file navigator |
| `i`, `X` | Toggle intents, open Agent changes |
| `s` | Select Agent session |
| `Esc`, `q` | Return to history |

### Context

| Key | Action |
| --- | --- |
| `j`/`k`, page keys, `gg`/`G` | Navigate source |
| `h`/`l`, `0`/`$` | Scroll long lines |
| `[c`/`]c` | Previous/next changed line |
| `w`/`W` | Select next/previous symbol |
| `gd`/`gr` | Find definitions/references |
| `I` | Map the selected symbol's impact |
| `*` | Search the reviewed repository version |
| `m`, `[m`/`]m` | Toggle/navigate bookmarks |
| `Q`, `V`…`Q` | Ask about a line or range |
| `R`, `[r`/`]r`, `T` | Create/navigate/show reading path |
| `Ctrl-o`/`Ctrl-i` | Back/forward through code locations |
| `b`, `Esc`, `q` | Return to diff |

### Review list

| Key | Action |
| --- | --- |
| `j`/`k` | Select comment |
| `e`, `d` | Edit/delete comment |
| `S` | Confirm and send comments |
| `s` | Select Agent session |
| `Esc`, `q` | Return to diff |

## Commands

| Command | Action |
| --- | --- |
| `:log <revision...>` | Reload history with a Git revision specification |
| `:reload` | Reload history |
| `:goto <line>` | Jump to a one-based diff line |
| `:next-file`, `:prev-file` | Move between changed files |
| `:next-change`, `:prev-change` | Move between hunks |
| `:sessions` | Discover repository Agent sessions |
| `:session <provider> <id>` | Select a session explicitly |
| `:session clear` | Clear the selected session |
| `:worktree`, `:staged` | Review working-tree or staged changes |
| `:help`, `:quit` | Open help or quit |

Git revision arguments are passed directly to `git log`, for example
`:log --all`, `:log main..feature`, or `:log --author Alice --since 2.weeks`.

## Safety and storage

- Sending always requires an explicit destination and confirmation.
- Agent CLIs run without a shell or approval-bypass flags.
- Code-understanding requests explicitly permit read-only inspection only.
- atig warns if a read-only request changes the working tree.
- Only safe repository-relative UTF-8 files up to 2 MiB open in Context View.
- Review threads and bookmarks are stored in `.git/atig/reviews.json`.
- Only one Agent delivery runs at a time; `Esc` requests cancellation.

## Performance

History and diffs stream from background Git processes. atig renders the
first available results, consumes bounded batches to keep input responsive,
and cancels stale loads after reloads. There is no artificial commit limit.

## Limitations

- Claude's non-interactive CLI may buffer output.
- The generic `other` Agent adapter is not implemented.
- Complex octopus or criss-cross graphs may not exactly match `tig`.

## Development

```sh
cargo run
cargo test
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
```

To publish a release, update the version in `Cargo.toml`, commit it, and push
a matching tag such as `v0.2.0`. GitHub Actions builds Linux and macOS archives
and publishes their SHA-256 checksums.

## License

MIT
