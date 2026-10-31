# atig

atig is a fast, Vim-oriented Git history viewer. It keeps the familiar
history-first workflow of tig and is being built to support review comments
that can be queued to an existing coding-agent session.

The first development slice provides a streaming Git history view:

- Git history is rendered as soon as git log produces output.
- j/k, arrow keys, gg, G, Ctrl-d, and Ctrl-u navigate commits.
- :log <revision...> reloads the view with a Git revision specification.
- :reload, :help, and :q provide the initial command interface.
- Enter opens the selected commit; Esc or q returns to history.
- The same j/k, gg/G, and Ctrl-d/Ctrl-u navigation works in commit diffs.
- Diff headers, hunks, additions, and deletions use tig-style colors.

## Development

    cargo run
    cargo test

Run atig from inside a Git repository.
