use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::git::{self, DiffEvent, HistoryEvent, HistoryLine};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
    Comment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    History,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffTarget {
    File,
    Change,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub commit: String,
    pub file: String,
    pub line: usize,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommentAnchor {
    commit: String,
    file: String,
    line: usize,
}

pub struct App {
    pub history: Vec<HistoryLine>,
    pub selected: Option<usize>,
    pub input_mode: InputMode,
    pub command: String,
    pub message: String,
    pub loading: bool,
    pub should_quit: bool,
    pub pending_g: bool,
    pub revisions: Vec<String>,
    pub view: View,
    pub diff: Vec<String>,
    pub diff_oid: Option<String>,
    pub diff_loading: bool,
    pub diff_scroll: usize,
    pub diff_jump_direction: Option<isize>,
    pub comments: Vec<ReviewComment>,
    pub comment_buffer: String,
    comment_anchor: Option<CommentAnchor>,
    history_rx: Option<Receiver<HistoryEvent>>,
    diff_rx: Option<Receiver<DiffEvent>>,
    pending_graph: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            selected: None,
            input_mode: InputMode::Normal,
            command: String::new(),
            message: "Loading Git history...".into(),
            loading: false,
            should_quit: false,
            pending_g: false,
            revisions: Vec::new(),
            view: View::History,
            diff: Vec::new(),
            diff_oid: None,
            diff_loading: false,
            diff_scroll: 0,
            diff_jump_direction: None,
            comments: Vec::new(),
            comment_buffer: String::new(),
            comment_anchor: None,
            history_rx: None,
            diff_rx: None,
            pending_graph: None,
        }
    }

    pub fn open_selected_commit(&mut self) {
        let Some(commit) = self
            .selected
            .and_then(|selected| self.history.get(selected))
            .and_then(HistoryLine::commit)
        else {
            return;
        };
        let oid = commit.oid.clone();
        let (tx, rx) = mpsc::channel();
        git::stream_diff(oid.clone(), tx);
        self.view = View::Diff;
        self.diff.clear();
        self.diff_oid = Some(oid);
        self.diff_loading = true;
        self.diff_scroll = 0;
        self.diff_jump_direction = None;
        self.diff_rx = Some(rx);
        self.message = "Loading commit diff...".into();
    }

    pub fn close_diff(&mut self) {
        self.view = View::History;
        self.pending_g = false;
        self.diff_jump_direction = None;
        self.diff_rx = None;
        self.diff_loading = false;
    }

    pub fn move_diff(&mut self, amount: isize) {
        self.pending_g = false;
        if amount < 0 {
            self.diff_scroll = self.diff_scroll.saturating_sub(amount.unsigned_abs());
        } else {
            self.diff_scroll = self
                .diff_scroll
                .saturating_add(amount as usize)
                .min(self.diff.len().saturating_sub(1));
        }
    }

    pub fn handle_diff_g(&mut self) {
        if self.pending_g {
            self.diff_scroll = 0;
            self.pending_g = false;
        } else {
            self.pending_g = true;
        }
    }

    pub fn select_diff_end(&mut self) {
        self.pending_g = false;
        self.diff_scroll = self.diff.len().saturating_sub(1);
    }

    pub fn begin_diff_jump(&mut self, direction: isize) {
        self.pending_g = false;
        self.diff_jump_direction = Some(direction.signum());
    }

    pub fn complete_diff_jump(&mut self, target: DiffTarget) {
        let Some(direction) = self.diff_jump_direction.take() else {
            return;
        };
        let matches = |line: &&String| match target {
            DiffTarget::File => line.starts_with("diff --git "),
            DiffTarget::Change => line.starts_with("@@"),
        };
        let destination = if direction > 0 {
            self.diff
                .iter()
                .enumerate()
                .skip(self.diff_scroll.saturating_add(1))
                .find(|(_, line)| matches(line))
                .map(|(index, _)| index)
        } else {
            self.diff[..self.diff_scroll.min(self.diff.len())]
                .iter()
                .enumerate()
                .rev()
                .find(|(_, line)| matches(line))
                .map(|(index, _)| index)
        };

        if let Some(destination) = destination {
            self.diff_scroll = destination;
        } else {
            let direction = if direction > 0 { "next" } else { "previous" };
            let target = match target {
                DiffTarget::File => "file",
                DiffTarget::Change => "change",
            };
            self.message = format!("No {direction} {target}");
        }
    }

    pub fn start_comment(&mut self) {
        let Some((file, line)) = diff_location(&self.diff, self.diff_scroll) else {
            self.message = "Select an added, removed, or context line".into();
            return;
        };
        let Some(commit) = self.diff_oid.clone() else {
            return;
        };
        self.comment_anchor = Some(CommentAnchor { commit, file, line });
        self.comment_buffer.clear();
        self.input_mode = InputMode::Comment;
    }

    pub fn comment_push(&mut self, character: char) {
        self.comment_buffer.push(character);
    }

    pub fn comment_backspace(&mut self) {
        self.comment_buffer.pop();
    }

    pub fn cancel_comment(&mut self) {
        self.comment_buffer.clear();
        self.comment_anchor = None;
        self.input_mode = InputMode::Normal;
    }

    pub fn submit_comment(&mut self) {
        let body = self.comment_buffer.trim().to_string();
        if body.is_empty() {
            self.message = "Comment was empty".into();
            self.cancel_comment();
            return;
        }
        let Some(anchor) = self.comment_anchor.take() else {
            self.cancel_comment();
            return;
        };
        self.comments.push(ReviewComment {
            commit: anchor.commit,
            file: anchor.file,
            line: anchor.line,
            body,
        });
        self.comment_buffer.clear();
        self.input_mode = InputMode::Normal;
        self.message = format!("{} review comments", self.comments.len());
    }

    pub fn drain_diff_events(&mut self) -> bool {
        let mut changed = false;
        loop {
            let event = match self.diff_rx.as_ref().map(Receiver::try_recv) {
                Some(Ok(event)) => event,
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.diff_loading = false;
                    self.diff_rx = None;
                    changed = true;
                    break;
                }
            };
            changed = true;
            match event {
                DiffEvent::Line(line) => self.diff.push(line),
                DiffEvent::Done => {
                    self.diff_loading = false;
                    self.diff_rx = None;
                    self.message = format!("{} lines", self.diff.len());
                }
                DiffEvent::Error(error) => {
                    self.diff_loading = false;
                    self.diff_rx = None;
                    self.message = error;
                }
            }
        }
        changed
    }

    pub fn reload(&mut self) {
        let (tx, rx) = mpsc::channel();
        git::stream_history(self.revisions.clone(), tx);
        self.history.clear();
        self.selected = None;
        self.pending_graph = None;
        self.history_rx = Some(rx);
        self.loading = true;
        self.message = if self.revisions.is_empty() {
            "Loading Git history...".into()
        } else {
            format!("Loading: {}", self.revisions.join(" "))
        };
    }

    pub fn drain_history_events(&mut self) -> bool {
        let mut changed = false;
        loop {
            let event = match self.history_rx.as_ref().map(Receiver::try_recv) {
                Some(Ok(event)) => event,
                Some(Err(TryRecvError::Empty)) | None => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.loading = false;
                    self.history_rx = None;
                    changed = true;
                    break;
                }
            };
            changed = true;

            match event {
                HistoryEvent::Line(HistoryLine::Commit(mut commit)) => {
                    commit.preceding_graph = self.pending_graph.take();
                    let index = self.history.len();
                    if self.selected.is_none() {
                        self.selected = Some(index);
                    }
                    self.history.push(HistoryLine::Commit(commit));
                }
                HistoryEvent::Line(HistoryLine::Graph(graph)) => {
                    self.pending_graph = Some(graph);
                }
                HistoryEvent::Done => {
                    self.loading = false;
                    self.message = format!("{} commits", self.commit_count());
                }
                HistoryEvent::Error(error) => {
                    self.loading = false;
                    self.message = error;
                }
            }
        }
        changed
    }

    pub fn move_selection(&mut self, amount: isize) {
        self.pending_g = false;
        if amount == 0 || self.history.is_empty() {
            return;
        }

        let direction = amount.signum();
        let mut remaining = amount.unsigned_abs();
        let mut cursor = self.selected.unwrap_or(if direction > 0 {
            0
        } else {
            self.history.len().saturating_sub(1)
        });

        while remaining > 0 {
            let Some(next) = self.next_commit_index(cursor, direction) else {
                break;
            };
            cursor = next;
            remaining -= 1;
        }

        if self
            .history
            .get(cursor)
            .and_then(HistoryLine::commit)
            .is_some()
        {
            self.selected = Some(cursor);
        }
    }

    pub fn handle_g(&mut self) {
        if self.pending_g {
            self.select_first();
            self.pending_g = false;
        } else {
            self.pending_g = true;
        }
    }

    pub fn select_first(&mut self) {
        self.selected = self.history.iter().position(|line| line.commit().is_some());
    }

    pub fn select_last(&mut self) {
        self.pending_g = false;
        self.selected = self
            .history
            .iter()
            .rposition(|line| line.commit().is_some());
    }

    pub fn start_command(&mut self) {
        self.pending_g = false;
        self.input_mode = InputMode::Command;
        self.command.clear();
    }

    pub fn cancel_command(&mut self) {
        self.input_mode = InputMode::Normal;
        self.command.clear();
    }

    pub fn command_push(&mut self, character: char) {
        self.command.push(character);
    }

    pub fn command_backspace(&mut self) {
        if self.command.pop().is_none() {
            self.cancel_command();
        }
    }

    pub fn submit_command(&mut self) {
        let command = std::mem::take(&mut self.command);
        self.input_mode = InputMode::Normal;
        let mut parts = command.split_whitespace();

        match parts.next() {
            Some("q" | "quit") => self.should_quit = true,
            Some("reload") => self.reload(),
            Some("log") => {
                self.revisions = parts.map(str::to_owned).collect();
                self.reload();
            }
            Some("help") => {
                self.message = "j/k move  gg/G ends  Ctrl-d/u page  :log <rev>  :q quit".into();
            }
            Some(other) => self.message = format!("Unknown command: {other}"),
            None => {}
        }
    }

    pub fn commit_count(&self) -> usize {
        self.history
            .iter()
            .filter(|line| line.commit().is_some())
            .count()
    }

    pub fn current_branch(&self) -> &str {
        self.history
            .iter()
            .filter_map(HistoryLine::commit)
            .flat_map(|commit| commit.refs.split(", "))
            .find_map(|reference| reference.strip_prefix("HEAD -> "))
            .unwrap_or("HEAD")
    }

    pub fn selected_commit_position(&self) -> Option<usize> {
        let selected = self.selected?;
        Some(
            self.history[..=selected]
                .iter()
                .filter(|line| line.commit().is_some())
                .count(),
        )
    }

    fn next_commit_index(&self, current: usize, direction: isize) -> Option<usize> {
        if direction > 0 {
            ((current + 1)..self.history.len())
                .find(|&index| self.history[index].commit().is_some())
        } else {
            (0..current)
                .rev()
                .find(|&index| self.history[index].commit().is_some())
        }
    }
}

fn diff_location(diff: &[String], cursor: usize) -> Option<(String, usize)> {
    let mut file = None;
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    let mut in_hunk = false;

    for (index, text) in diff.iter().enumerate().take(cursor.saturating_add(1)) {
        if let Some(path) = text.strip_prefix("+++ b/") {
            file = Some(path.to_string());
            continue;
        }
        if let Some((old, new)) = parse_hunk_start(text) {
            old_line = old;
            new_line = new;
            in_hunk = true;
            continue;
        }
        if !in_hunk || text.starts_with("\\ No newline") {
            continue;
        }

        let location = if text.starts_with('+') {
            let line = new_line;
            new_line += 1;
            line
        } else if text.starts_with('-') {
            let line = old_line;
            old_line += 1;
            line
        } else if text.starts_with(' ') {
            let line = new_line;
            old_line += 1;
            new_line += 1;
            line
        } else {
            continue;
        };

        if index == cursor {
            return Some((file.clone()?, location));
        }
    }
    None
}

fn parse_hunk_start(line: &str) -> Option<(usize, usize)> {
    let header = line.strip_prefix("@@ -")?;
    let (old, rest) = header.split_once(" +")?;
    let new = rest.split_whitespace().next()?;
    Some((
        old.split(',').next()?.parse().ok()?,
        new.split(',').next()?.parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::parse_history_line;

    fn app_with_history() -> App {
        let mut app = App::new();
        app.history = vec![
            parse_history_line(
                "* \u{1e}a\u{1f}a\u{1f}parent\u{1f}2026-01-01 00:00 +0000\u{1f}Alice\u{1f}\u{1f}one",
            ),
            parse_history_line("|\\"),
            parse_history_line(
                "| * \u{1e}b\u{1f}b\u{1f}parent\u{1f}2026-01-02 00:00 +0000\u{1f}Bob\u{1f}\u{1f}two",
            ),
        ];
        app.select_first();
        app
    }

    #[test]
    fn navigation_skips_graph_only_lines() {
        let mut app = app_with_history();
        assert_eq!(app.selected, Some(0));

        app.move_selection(1);

        assert_eq!(app.selected, Some(2));
    }

    #[test]
    fn log_command_replaces_revision_arguments() {
        let mut app = App::new();
        app.input_mode = InputMode::Command;
        app.command = "log --all main..feature".into();

        app.submit_command();

        assert_eq!(app.revisions, ["--all", "main..feature"]);
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn diff_navigation_is_bounded_and_supports_gg() {
        let mut app = App::new();
        app.diff = (0..20).map(|line| line.to_string()).collect();

        app.move_diff(10);
        app.move_diff(100);
        assert_eq!(app.diff_scroll, 19);

        app.handle_diff_g();
        app.handle_diff_g();
        assert_eq!(app.diff_scroll, 0);

        app.move_diff(-1);
        assert_eq!(app.diff_scroll, 0);
    }

    #[test]
    fn diff_jumps_between_files_and_changes() {
        let mut app = App::new();
        app.diff = vec![
            "commit abc".into(),
            "diff --git a/one b/one".into(),
            "@@ -1 +1 @@".into(),
            "+one".into(),
            "diff --git a/two b/two".into(),
            "@@ -2 +2 @@".into(),
        ];

        app.begin_diff_jump(1);
        app.complete_diff_jump(DiffTarget::File);
        assert_eq!(app.diff_scroll, 1);

        app.begin_diff_jump(1);
        app.complete_diff_jump(DiffTarget::Change);
        assert_eq!(app.diff_scroll, 2);

        app.begin_diff_jump(-1);
        app.complete_diff_jump(DiffTarget::File);
        assert_eq!(app.diff_scroll, 1);
    }

    #[test]
    fn comment_records_commit_file_and_new_line() {
        let mut app = App::new();
        app.diff_oid = Some("abc123".into());
        app.diff = vec![
            "diff --git a/src/main.rs b/src/main.rs".into(),
            "--- a/src/main.rs".into(),
            "+++ b/src/main.rs".into(),
            "@@ -10,2 +10,3 @@".into(),
            " context".into(),
            "+added".into(),
        ];
        app.diff_scroll = 5;

        app.start_comment();
        app.comment_buffer = "Please simplify".into();
        app.submit_comment();

        assert_eq!(
            app.comments,
            [ReviewComment {
                commit: "abc123".into(),
                file: "src/main.rs".into(),
                line: 11,
                body: "Please simplify".into(),
            }]
        );
        assert_eq!(app.input_mode, InputMode::Normal);
    }
}
