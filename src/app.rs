use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::agent::{self, AgentTarget, DispatchEvent};
use crate::git::{self, DiffEvent, HistoryEvent, HistoryLine};

const MAX_EVENTS_PER_TICK: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
    Comment,
    ConfirmSend,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    History,
    Diff,
    Comments,
    Help,
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
    pub comment_selected: usize,
    pub agent_target: Option<AgentTarget>,
    pub dispatching: bool,
    pub search_input: String,
    pub search_query: String,
    comment_anchor: Option<CommentAnchor>,
    comment_edit_index: Option<usize>,
    dispatch_rx: Option<Receiver<DispatchEvent>>,
    help_return: View,
    history_rx: Option<Receiver<HistoryEvent>>,
    history_cancel: Option<git::Cancellation>,
    diff_rx: Option<Receiver<DiffEvent>>,
    diff_cancel: Option<git::Cancellation>,
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
            comment_selected: 0,
            agent_target: None,
            dispatching: false,
            search_input: String::new(),
            search_query: String::new(),
            comment_anchor: None,
            comment_edit_index: None,
            dispatch_rx: None,
            help_return: View::History,
            history_rx: None,
            history_cancel: None,
            diff_rx: None,
            diff_cancel: None,
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
        if let Some(cancellation) = self.diff_cancel.take() {
            cancellation.cancel();
        }
        self.diff_cancel = Some(git::stream_diff(oid.clone(), tx));
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
        if let Some(cancellation) = self.diff_cancel.take() {
            cancellation.cancel();
        }
        self.diff_loading = false;
    }

    pub fn reload_diff(&mut self) {
        let Some(oid) = self.diff_oid.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        if let Some(cancellation) = self.diff_cancel.take() {
            cancellation.cancel();
        }
        self.diff_cancel = Some(git::stream_diff(oid, tx));
        self.diff.clear();
        self.diff_scroll = 0;
        self.diff_loading = true;
        self.diff_rx = Some(rx);
        self.message = "Reloading commit diff...".into();
    }

    pub fn open_help(&mut self) {
        self.help_return = self.view;
        self.view = View::Help;
        self.input_mode = InputMode::Normal;
    }

    pub fn close_help(&mut self) {
        self.view = self.help_return;
    }

    pub fn start_search(&mut self) {
        self.search_input.clear();
        self.input_mode = InputMode::Search;
    }

    pub fn search_push(&mut self, character: char) {
        self.search_input.push(character);
    }

    pub fn search_backspace(&mut self) {
        if self.search_input.pop().is_none() {
            self.cancel_search();
        }
    }

    pub fn cancel_search(&mut self) {
        self.search_input.clear();
        self.input_mode = InputMode::Normal;
    }

    pub fn submit_search(&mut self) {
        let query = self.search_input.trim().to_string();
        self.search_input.clear();
        self.input_mode = InputMode::Normal;
        if query.is_empty() {
            return;
        }
        self.search_query = query;
        self.repeat_search(1);
    }

    pub fn repeat_search(&mut self, direction: isize) {
        if self.search_query.is_empty() {
            self.message = "No previous search".into();
            return;
        }
        let query = self.search_query.to_lowercase();
        let destination = match self.view {
            View::History => find_wrapped(
                self.history.len(),
                self.selected.unwrap_or(0),
                direction,
                |index| {
                    self.history[index].commit().is_some_and(|commit| {
                        format!(
                            "{} {} {} {} {}",
                            commit.oid, commit.date, commit.author, commit.refs, commit.subject
                        )
                        .to_lowercase()
                        .contains(&query)
                    })
                },
            ),
            View::Diff => find_wrapped(self.diff.len(), self.diff_scroll, direction, |index| {
                self.diff[index].to_lowercase().contains(&query)
            }),
            View::Comments | View::Help => None,
        };
        match (self.view, destination) {
            (View::History, Some(index)) => self.selected = Some(index),
            (View::Diff, Some(index)) => self.diff_scroll = index,
            (_, Some(_)) => {}
            (_, None) => self.message = format!("Pattern not found: {}", self.search_query),
        }
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

    pub fn jump_diff(&mut self, target: DiffTarget, direction: isize) {
        self.begin_diff_jump(direction);
        self.complete_diff_jump(target);
    }

    pub fn go_to_diff_line(&mut self, line: usize) {
        if self.diff.is_empty() {
            self.message = "No diff lines".into();
            return;
        }
        self.diff_scroll = line.saturating_sub(1).min(self.diff.len() - 1);
        self.message = format!("Line {}", self.diff_scroll + 1);
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
        self.comment_edit_index = None;
        self.input_mode = InputMode::Normal;
    }

    pub fn submit_comment(&mut self) {
        let body = self.comment_buffer.trim().to_string();
        if body.is_empty() {
            self.message = "Comment was empty".into();
            self.cancel_comment();
            return;
        }
        if let Some(index) = self.comment_edit_index.take() {
            if let Some(comment) = self.comments.get_mut(index) {
                comment.body = body;
            }
            self.comment_buffer.clear();
            self.input_mode = InputMode::Normal;
            self.message = "Comment updated".into();
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

    pub fn open_comments(&mut self) {
        self.view = View::Comments;
        self.comment_selected = self
            .comment_selected
            .min(self.comments.len().saturating_sub(1));
    }

    pub fn close_comments(&mut self) {
        self.view = View::Diff;
        self.input_mode = InputMode::Normal;
        self.comment_buffer.clear();
        self.comment_edit_index = None;
    }

    pub fn move_comment_selection(&mut self, amount: isize) {
        if amount < 0 {
            self.comment_selected = self.comment_selected.saturating_sub(amount.unsigned_abs());
        } else {
            self.comment_selected = self
                .comment_selected
                .saturating_add(amount as usize)
                .min(self.comments.len().saturating_sub(1));
        }
    }

    pub fn edit_selected_comment(&mut self) {
        let Some(comment) = self.comments.get(self.comment_selected) else {
            self.message = "No comments".into();
            return;
        };
        self.comment_buffer = comment.body.clone();
        self.comment_edit_index = Some(self.comment_selected);
        self.input_mode = InputMode::Comment;
    }

    pub fn delete_selected_comment(&mut self) {
        if self.comment_selected >= self.comments.len() {
            self.message = "No comments".into();
            return;
        }
        self.comments.remove(self.comment_selected);
        self.comment_selected = self
            .comment_selected
            .min(self.comments.len().saturating_sub(1));
        self.message = format!("{} review comments", self.comments.len());
    }

    pub fn request_send_review(&mut self) {
        if self.dispatching {
            self.message = "A review is already being delivered".into();
        } else if self.agent_target.is_none() {
            self.message = "Set a target with :session <provider> <id>".into();
        } else if self.comments.is_empty() {
            self.message = "No review comments to send".into();
        } else {
            self.input_mode = InputMode::ConfirmSend;
        }
    }

    pub fn cancel_send_review(&mut self) {
        self.input_mode = InputMode::Normal;
        self.message = "Review delivery cancelled".into();
    }

    pub fn confirm_send_review(&mut self) {
        let Some(target) = self.agent_target.clone() else {
            self.cancel_send_review();
            return;
        };
        let prompt = format_review_request(&self.comments);
        let (tx, rx) = mpsc::channel();
        agent::dispatch(target, prompt, tx);
        self.dispatch_rx = Some(rx);
        self.dispatching = true;
        self.input_mode = InputMode::Normal;
        self.message = "Review queued for delivery".into();
    }

    pub fn drain_dispatch_events(&mut self) -> bool {
        let event = match self.dispatch_rx.as_ref().map(Receiver::try_recv) {
            Some(Ok(event)) => event,
            Some(Err(TryRecvError::Empty)) | None => return false,
            Some(Err(TryRecvError::Disconnected)) => {
                DispatchEvent::Error("Agent delivery ended unexpectedly".into())
            }
        };
        self.dispatch_rx = None;
        self.dispatching = false;
        match event {
            DispatchEvent::Done => {
                self.comments.clear();
                self.comment_selected = 0;
                self.message = "Review delivered".into();
            }
            DispatchEvent::Error(error) => self.message = error,
        }
        true
    }

    pub fn drain_diff_events(&mut self) -> bool {
        let mut changed = false;
        for _ in 0..MAX_EVENTS_PER_TICK {
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
                    self.diff_cancel = None;
                    self.message = format!("{} lines", self.diff.len());
                }
                DiffEvent::Error(error) => {
                    self.diff_loading = false;
                    self.diff_rx = None;
                    self.diff_cancel = None;
                    self.message = error;
                }
            }
        }
        changed
    }

    pub fn reload(&mut self) {
        if let Some(cancellation) = self.history_cancel.take() {
            cancellation.cancel();
        }
        let (tx, rx) = mpsc::channel();
        self.history_cancel = Some(git::stream_history(self.revisions.clone(), tx));
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
        for _ in 0..MAX_EVENTS_PER_TICK {
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
                    self.history_cancel = None;
                    self.message = format!("{} commits", self.commit_count());
                }
                HistoryEvent::Error(error) => {
                    self.loading = false;
                    self.history_cancel = None;
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
            Some("session") => match (parts.next(), parts.next(), parts.next()) {
                (Some("clear"), None, None) => {
                    self.agent_target = None;
                    self.message = "Agent session cleared".into();
                }
                (Some(provider), Some(session_id), None) => {
                    match AgentTarget::parse(provider, session_id) {
                        Ok(target) => {
                            self.message = format!("Agent session: {}", target.label());
                            self.agent_target = Some(target);
                        }
                        Err(error) => self.message = error,
                    }
                }
                _ => self.message = "Usage: :session <codex|claude|other> <id>".into(),
            },
            Some("goto") => match (parts.next(), parts.next()) {
                (Some(line), None) if self.view == View::Diff => match line.parse::<usize>() {
                    Ok(line) if line > 0 => self.go_to_diff_line(line),
                    _ => self.message = "Usage: :goto <positive-line-number>".into(),
                },
                _ => self.message = "Usage: :goto <line> (diff view only)".into(),
            },
            Some("next-file") if self.view == View::Diff => self.jump_diff(DiffTarget::File, 1),
            Some("prev-file") if self.view == View::Diff => self.jump_diff(DiffTarget::File, -1),
            Some("next-change") if self.view == View::Diff => self.jump_diff(DiffTarget::Change, 1),
            Some("prev-change") if self.view == View::Diff => {
                self.jump_diff(DiffTarget::Change, -1)
            }
            Some("help") => {
                self.open_help();
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

impl Drop for App {
    fn drop(&mut self) {
        if let Some(cancellation) = self.history_cancel.take() {
            cancellation.cancel();
        }
        if let Some(cancellation) = self.diff_cancel.take() {
            cancellation.cancel();
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

fn find_wrapped(
    length: usize,
    current: usize,
    direction: isize,
    matches: impl Fn(usize) -> bool,
) -> Option<usize> {
    if length == 0 {
        return None;
    }
    let current = current.min(length - 1);
    if direction >= 0 {
        ((current + 1)..length)
            .chain(0..=current)
            .find(|&index| matches(index))
    } else {
        (0..current)
            .rev()
            .chain((current..length).rev())
            .find(|&index| matches(index))
    }
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

fn format_review_request(comments: &[ReviewComment]) -> String {
    let mut prompt = String::from(
        "Please address the following code review comments. Use a sub-agent for the implementation when your environment supports it. Inspect the current working tree before editing, make only the requested changes, and run relevant tests. Do not commit or push.\n\n",
    );
    for (index, comment) in comments.iter().enumerate() {
        prompt.push_str(&format!(
            "{}. Commit: {}\n   Location: {}:{}\n   Comment: {}\n\n",
            index + 1,
            comment.commit,
            comment.file,
            comment.line,
            comment.body
        ));
    }
    prompt
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
    fn session_command_sets_validated_agent_target() {
        let mut app = App::new();
        app.input_mode = InputMode::Command;
        app.command = "session codex 019a-session".into();

        app.submit_command();

        assert_eq!(
            app.agent_target.as_ref().map(AgentTarget::label),
            Some("codex:019a-session".into())
        );
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

        app.jump_diff(DiffTarget::Change, 1);
        assert_eq!(app.diff_scroll, 2);
    }

    #[test]
    fn goto_command_uses_one_based_diff_lines_and_clamps() {
        let mut app = App::new();
        app.view = View::Diff;
        app.diff = (1..=20).map(|line| line.to_string()).collect();
        app.input_mode = InputMode::Command;
        app.command = "goto 12".into();

        app.submit_command();
        assert_eq!(app.diff_scroll, 11);

        app.input_mode = InputMode::Command;
        app.command = "goto 999".into();
        app.submit_command();
        assert_eq!(app.diff_scroll, 19);
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

    #[test]
    fn comments_can_be_edited_and_deleted() {
        let mut app = App::new();
        app.comments.push(ReviewComment {
            commit: "abc123".into(),
            file: "src/main.rs".into(),
            line: 11,
            body: "Old body".into(),
        });

        app.edit_selected_comment();
        app.comment_buffer = "New body".into();
        app.submit_comment();
        assert_eq!(app.comments[0].body, "New body");

        app.delete_selected_comment();
        assert!(app.comments.is_empty());
        assert_eq!(app.comment_selected, 0);
    }

    #[test]
    fn review_send_requires_comments_and_a_target() {
        let mut app = App::new();
        app.request_send_review();
        assert_eq!(app.input_mode, InputMode::Normal);

        app.agent_target = Some(AgentTarget::parse("codex", "session-1").unwrap());
        app.comments.push(ReviewComment {
            commit: "abc123".into(),
            file: "src/main.rs".into(),
            line: 4,
            body: "Handle this error".into(),
        });
        app.request_send_review();
        assert_eq!(app.input_mode, InputMode::ConfirmSend);

        let prompt = format_review_request(&app.comments);
        assert!(prompt.contains("src/main.rs:4"));
        assert!(prompt.contains("Handle this error"));
    }

    #[test]
    fn history_drain_yields_after_a_bounded_batch() {
        let mut app = App::new();
        let (tx, rx) = mpsc::channel();
        for index in 0..300 {
            let line = parse_history_line(&format!(
                "* \u{1e}{index}\u{1f}{index}\u{1f}parent\u{1f}2026-01-01 00:00 +0000\u{1f}Alice\u{1f}\u{1f}commit"
            ));
            tx.send(HistoryEvent::Line(line)).unwrap();
        }
        tx.send(HistoryEvent::Done).unwrap();
        app.history_rx = Some(rx);
        app.loading = true;

        app.drain_history_events();
        assert_eq!(app.history.len(), MAX_EVENTS_PER_TICK);
        assert!(app.loading);

        app.drain_history_events();
        assert_eq!(app.history.len(), 300);
        assert!(!app.loading);
    }

    #[test]
    fn search_wraps_in_history_and_diff() {
        let mut app = app_with_history();
        app.search_query = "two".into();
        app.repeat_search(1);
        assert_eq!(app.selected, Some(2));

        app.view = View::Diff;
        app.diff = vec!["first".into(), "needle".into(), "last".into()];
        app.diff_scroll = 2;
        app.search_query = "needle".into();
        app.repeat_search(1);
        assert_eq!(app.diff_scroll, 1);

        app.repeat_search(-1);
        assert_eq!(app.diff_scroll, 1);
    }
}
