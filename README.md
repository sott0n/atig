# atig

atig is a fast, Vim-oriented Git history viewer. It keeps the familiar
history-first workflow of tig and is being built to support review comments
that can be queued to an existing coding-agent session.

The first development slice provides a streaming Git history view:

- Git history is rendered as soon as git log produces output.
- Large histories are consumed in bounded batches so input and drawing stay responsive.
- j/k, arrow keys, gg, G, Ctrl-d, and Ctrl-u navigate commits.
- :log <revision...> reloads the view with a Git revision specification.
- :reload, :help, and :q provide the initial command interface.
- Enter opens the selected commit; Esc or q returns to history.
- The same j/k, gg/G, and Ctrl-d/Ctrl-u navigation works in commit diffs.
- Diff headers, hunks, additions, and deletions use tig-style colors.
- [f/]f jump between changed files; [c/]c jump between diff hunks.
- a adds a review comment to the selected diff line.
- v opens the comment list; j/k select, e edits, and d deletes comments.
- :session <codex|claude|other> <id> selects a validated agent session target.
- S previews and confirms one batched review delivery to the selected session.

## Development

    cargo run
    cargo test

Run atig from inside a Git repository.
