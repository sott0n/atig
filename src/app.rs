use std::sync::mpsc::{self, Receiver, TryRecvError};

use serde::{Deserialize, Serialize};

use crate::agent::{self, AgentSession, AgentTarget, DispatchEvent, SessionListEvent};
use crate::git::{self, DiffEvent, HistoryEvent, HistoryLine};
use crate::review_store::ReviewStore;

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
    Files,
    Help,
    Sessions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffTarget {
    File,
    Change,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewThreadRole {
    Human,
    Agent(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewThreadMessage {
    pub role: ReviewThreadRole,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewScope {
    Commit,
    File(String),
    Line {
        file: String,
        line: usize,
    },
    Range {
        file: String,
        start: usize,
        end: usize,
    },
}

impl ReviewScope {
    pub fn label(&self) -> String {
        match self {
            Self::Commit => "COMMIT".into(),
            Self::File(file) => format!("FILE · {file}"),
            Self::Line { file, line } => format!("LINE · {file}:{line}"),
            Self::Range { file, start, end } => format!("RANGE · {file}:{start}-{end}"),
        }
    }

    fn prompt_location(&self) -> String {
        match self {
            Self::Commit => "whole commit".into(),
            Self::File(file) => format!("whole file: {file}"),
            Self::Line { file, line } => format!("line: {file}:{line}"),
            Self::Range { file, start, end } => format!("line range: {file}:{start}-{end}"),
        }
    }

    fn file(&self) -> Option<&str> {
        match self {
            Self::File(file) | Self::Line { file, .. } | Self::Range { file, .. } => Some(file),
            Self::Commit => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewComment {
    pub commit: String,
    pub scope: ReviewScope,
    pub diff_index: usize,
    pub body: String,
    pub context: Vec<String>,
    pub thread: Vec<ReviewThreadMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFileSummary {
    pub path: String,
    pub diff_index: usize,
    pub review_count: usize,
    pub agent_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommentAnchor {
    commit: String,
    scope: ReviewScope,
    diff_index: usize,
    context: Vec<String>,
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
    diff_source: Option<git::DiffSource>,
    pub diff_loading: bool,
    pub diff_scroll: usize,
    pub diff_comment_selected: Option<usize>,
    pub diff_thread_selected: Option<usize>,
    pub range_start: Option<usize>,
    pub diff_jump_direction: Option<isize>,
    pub comments: Vec<ReviewComment>,
    pub comment_buffer: String,
    pub comment_selected: usize,
    pub file_selected: usize,
    pub agent_target: Option<AgentTarget>,
    pub dispatching: bool,
    pub delivery_message: Option<String>,
    pub search_input: String,
    pub search_query: String,
    pub sessions: Vec<AgentSession>,
    pub session_selected: usize,
    pub sessions_loading: bool,
    comment_anchor: Option<CommentAnchor>,
    comment_edit_index: Option<usize>,
    comment_followup_index: Option<usize>,
    pending_send_comment_index: Option<usize>,
    dispatched_comments: Vec<ReviewComment>,
    dispatched_agent: Option<String>,
    dispatch_rx: Option<Receiver<DispatchEvent>>,
    dispatch_cancel: Option<agent::DispatchCancellation>,
    dispatch_snapshot: Option<git::ChangeSnapshot>,
    pub last_agent_changes: Vec<String>,
    session_rx: Option<Receiver<SessionListEvent>>,
    help_return: View,
    sessions_return: View,
    history_rx: Option<Receiver<HistoryEvent>>,
    history_cancel: Option<git::Cancellation>,
    diff_rx: Option<Receiver<DiffEvent>>,
    diff_cancel: Option<git::Cancellation>,
    pending_graph: Option<String>,
    review_store: Option<ReviewStore>,
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
            diff_source: None,
            diff_loading: false,
            diff_scroll: 0,
            diff_comment_selected: None,
            diff_thread_selected: None,
            range_start: None,
            diff_jump_direction: None,
            comments: Vec::new(),
            comment_buffer: String::new(),
            comment_selected: 0,
            file_selected: 0,
            agent_target: None,
            dispatching: false,
            delivery_message: None,
            search_input: String::new(),
            search_query: String::new(),
            sessions: Vec::new(),
            session_selected: 0,
            sessions_loading: false,
            comment_anchor: None,
            comment_edit_index: None,
            comment_followup_index: None,
            pending_send_comment_index: None,
            dispatched_comments: Vec::new(),
            dispatched_agent: None,
            dispatch_rx: None,
            dispatch_cancel: None,
            dispatch_snapshot: None,
            last_agent_changes: Vec::new(),
            session_rx: None,
            help_return: View::History,
            sessions_return: View::History,
            history_rx: None,
            history_cancel: None,
            diff_rx: None,
            diff_cancel: None,
            pending_graph: None,
            review_store: None,
        }
    }

    pub fn enable_review_store(&mut self) {
        match ReviewStore::discover() {
            Ok(store) => match store.load() {
                Ok(comments) => {
                    self.comments = comments;
                    self.review_store = Some(store);
                }
                Err(error) => self.message = error,
            },
            Err(error) => self.message = error,
        }
    }

    fn persist_reviews(&mut self) {
        if let Some(store) = &self.review_store
            && let Err(error) = store.save(&self.comments)
        {
            self.message = error;
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
        let source = git::DiffSource::Commit(oid.clone());
        self.diff_cancel = Some(git::stream_diff(source.clone(), tx));
        self.view = View::Diff;
        self.diff.clear();
        self.diff_oid = Some(oid);
        self.diff_source = Some(source);
        self.diff_loading = true;
        self.diff_scroll = 0;
        self.diff_comment_selected = None;
        self.diff_thread_selected = None;
        self.diff_jump_direction = None;
        self.diff_rx = Some(rx);
        self.message = "Loading commit diff...".into();
    }

    pub fn close_diff(&mut self) {
        self.view = View::History;
        self.diff_comment_selected = None;
        self.diff_thread_selected = None;
        self.pending_g = false;
        self.diff_jump_direction = None;
        self.diff_rx = None;
        if let Some(cancellation) = self.diff_cancel.take() {
            cancellation.cancel();
        }
        self.diff_loading = false;
    }

    pub fn reload_diff(&mut self) {
        let Some(source) = self.diff_source.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        if let Some(cancellation) = self.diff_cancel.take() {
            cancellation.cancel();
        }
        self.diff_cancel = Some(git::stream_diff(source, tx));
        self.diff.clear();
        self.diff_scroll = 0;
        self.diff_comment_selected = None;
        self.diff_thread_selected = None;
        self.diff_loading = true;
        self.diff_rx = Some(rx);
        self.message = "Reloading commit diff...".into();
    }

    pub fn open_worktree_diff(&mut self) {
        self.open_diff_source(git::DiffSource::Worktree);
    }

    pub fn open_staged_diff(&mut self) {
        self.open_diff_source(git::DiffSource::Staged);
    }

    fn open_diff_source(&mut self, source: git::DiffSource) {
        if let Some(cancellation) = self.diff_cancel.take() {
            cancellation.cancel();
        }
        let id = source.id();
        let (tx, rx) = mpsc::channel();
        self.diff_cancel = Some(git::stream_diff(source.clone(), tx));
        self.diff.clear();
        self.diff_oid = Some(id.clone());
        self.diff_source = Some(source);
        self.view = View::Diff;
        self.diff_loading = true;
        self.diff_scroll = 0;
        self.diff_comment_selected = None;
        self.diff_thread_selected = None;
        self.diff_jump_direction = None;
        self.diff_rx = Some(rx);
        self.message = format!("Loading {id} diff...");
    }

    pub fn open_help(&mut self) {
        self.help_return = self.view;
        self.view = View::Help;
        self.input_mode = InputMode::Normal;
    }

    pub fn close_help(&mut self) {
        self.view = self.help_return;
    }

    pub fn open_sessions(&mut self) {
        if self.view != View::Sessions {
            self.sessions_return = self.view;
        }
        self.view = View::Sessions;
        self.sessions.clear();
        self.session_selected = 0;
        self.sessions_loading = true;
        let (tx, rx) = mpsc::channel();
        agent::discover_sessions(tx);
        self.session_rx = Some(rx);
        self.message = "Discovering agent sessions...".into();
    }

    pub fn close_sessions(&mut self) {
        self.view = self.sessions_return;
        self.session_rx = None;
        self.sessions_loading = false;
    }

    pub fn move_session_selection(&mut self, amount: isize) {
        if amount < 0 {
            self.session_selected = self.session_selected.saturating_sub(amount.unsigned_abs());
        } else {
            self.session_selected = self
                .session_selected
                .saturating_add(amount as usize)
                .min(self.sessions.len().saturating_sub(1));
        }
    }

    pub fn select_agent_session(&mut self) {
        let Some(session) = self.sessions.get(self.session_selected) else {
            self.message = "No agent sessions found for this repository".into();
            return;
        };
        self.agent_target = Some(session.target.clone());
        self.message = format!("Agent session: {}", session.target.label());
        self.close_sessions();
    }

    pub fn drain_session_events(&mut self) -> bool {
        let event = match self.session_rx.as_ref().map(Receiver::try_recv) {
            Some(Ok(event)) => event,
            Some(Err(TryRecvError::Empty)) | None => return false,
            Some(Err(TryRecvError::Disconnected)) => {
                SessionListEvent::Error("Session discovery ended unexpectedly".into())
            }
        };
        self.session_rx = None;
        self.sessions_loading = false;
        match event {
            SessionListEvent::Done(sessions) => {
                self.sessions = sessions;
                self.message = format!("{} agent sessions", self.sessions.len());
            }
            SessionListEvent::Error(error) => self.message = error,
        }
        true
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
            View::Comments | View::Files | View::Help | View::Sessions => None,
        };
        match (self.view, destination) {
            (View::History, Some(index)) => self.selected = Some(index),
            (View::Diff, Some(index)) => {
                self.diff_scroll = index;
                self.diff_comment_selected = None;
                self.diff_thread_selected = None;
            }
            (_, Some(_)) => {}
            (_, None) => self.message = format!("Pattern not found: {}", self.search_query),
        }
    }

    pub fn move_diff(&mut self, amount: isize) {
        self.pending_g = false;
        self.diff_comment_selected = None;
        self.diff_thread_selected = None;
        if amount < 0 {
            self.diff_scroll = self.diff_scroll.saturating_sub(amount.unsigned_abs());
        } else {
            self.diff_scroll = self
                .diff_scroll
                .saturating_add(amount as usize)
                .min(self.diff.len().saturating_sub(1));
        }
    }

    pub fn move_diff_cursor(&mut self, direction: isize) {
        self.pending_g = false;
        if direction > 0 {
            if let Some(selected) = self.diff_comment_selected {
                let thread_len = self.comments[selected].thread.len();
                if self
                    .diff_thread_selected
                    .is_some_and(|thread| thread + 1 < thread_len)
                {
                    self.diff_thread_selected = self.diff_thread_selected.map(|thread| thread + 1);
                } else if self.diff_thread_selected.is_none() && thread_len > 0 {
                    self.diff_thread_selected = Some(0);
                } else if let Some(next) = self
                    .comment_indices_at_diff_line(self.diff_scroll)
                    .into_iter()
                    .find(|&index| index > selected)
                {
                    self.diff_comment_selected = Some(next);
                    self.diff_thread_selected = None;
                } else {
                    self.diff_comment_selected = None;
                    self.diff_thread_selected = None;
                    self.diff_scroll = self
                        .diff_scroll
                        .saturating_add(1)
                        .min(self.diff.len().saturating_sub(1));
                }
            } else if let Some(first) = self
                .comment_indices_at_diff_line(self.diff_scroll)
                .into_iter()
                .next()
            {
                self.diff_comment_selected = Some(first);
                self.diff_thread_selected = None;
            } else {
                self.diff_scroll = self
                    .diff_scroll
                    .saturating_add(1)
                    .min(self.diff.len().saturating_sub(1));
            }
        } else if let Some(selected) = self.diff_comment_selected {
            if let Some(thread) = self.diff_thread_selected {
                self.diff_thread_selected = thread.checked_sub(1);
            } else if let Some(previous) = self
                .comment_indices_at_diff_line(self.diff_scroll)
                .into_iter()
                .rev()
                .find(|&index| index < selected)
            {
                self.select_end_of_comment(previous);
            } else {
                self.diff_comment_selected = None;
                self.diff_thread_selected = None;
            }
        } else if self.diff_scroll > 0 {
            self.diff_scroll -= 1;
            if let Some(last) = self
                .comment_indices_at_diff_line(self.diff_scroll)
                .into_iter()
                .next_back()
            {
                self.select_end_of_comment(last);
            }
        }
    }

    fn select_end_of_comment(&mut self, comment_index: usize) {
        self.diff_comment_selected = Some(comment_index);
        self.diff_thread_selected = self.comments[comment_index].thread.len().checked_sub(1);
    }

    fn comment_indices_at_diff_line(&self, diff_index: usize) -> Vec<usize> {
        self.comments
            .iter()
            .enumerate()
            .filter(|(_, comment)| {
                comment.diff_index == diff_index
                    && self.diff_oid.as_deref() == Some(comment.commit.as_str())
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn handle_diff_g(&mut self) {
        if self.pending_g {
            self.diff_scroll = 0;
            self.diff_comment_selected = None;
            self.diff_thread_selected = None;
            self.pending_g = false;
        } else {
            self.pending_g = true;
        }
    }

    pub fn select_diff_end(&mut self) {
        self.pending_g = false;
        self.diff_scroll = self.diff.len().saturating_sub(1);
        self.diff_comment_selected = None;
        self.diff_thread_selected = None;
    }

    pub fn begin_diff_jump(&mut self, direction: isize) {
        self.pending_g = false;
        self.diff_comment_selected = None;
        self.diff_thread_selected = None;
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
        self.diff_comment_selected = None;
        self.diff_thread_selected = None;
        self.message = format!("Line {}", self.diff_scroll + 1);
    }

    pub fn start_comment(&mut self) {
        if let Some(range_start) = self.range_start.take() {
            self.start_range_comment(range_start);
            return;
        }
        let Some((file, line)) = diff_location(&self.diff, self.diff_scroll) else {
            self.message = "Select an added, removed, or context line".into();
            return;
        };
        let Some(commit) = self.diff_oid.clone() else {
            return;
        };
        self.comment_anchor = Some(CommentAnchor {
            commit,
            scope: ReviewScope::Line { file, line },
            diff_index: self.diff_scroll,
            context: review_context(&self.diff, self.diff_scroll),
        });
        self.comment_buffer.clear();
        self.input_mode = InputMode::Comment;
    }

    pub fn start_file_comment(&mut self) {
        let Some((file, diff_index)) = diff_file_anchor(&self.diff, self.diff_scroll) else {
            self.message = "Select a line inside a changed file".into();
            return;
        };
        let Some(commit) = self.diff_oid.clone() else {
            return;
        };
        self.comment_anchor = Some(CommentAnchor {
            commit,
            scope: ReviewScope::File(file),
            diff_index,
            context: Vec::new(),
        });
        self.diff_scroll = diff_index;
        self.comment_buffer.clear();
        self.input_mode = InputMode::Comment;
    }

    pub fn start_commit_comment(&mut self) {
        let Some(commit) = self.diff_oid.clone() else {
            return;
        };
        let diff_index = commit_comment_anchor(&self.diff);
        self.comment_anchor = Some(CommentAnchor {
            commit,
            scope: ReviewScope::Commit,
            diff_index,
            context: Vec::new(),
        });
        self.diff_scroll = diff_index;
        self.comment_buffer.clear();
        self.input_mode = InputMode::Comment;
    }

    pub fn toggle_range_selection(&mut self) {
        if self.range_start.take().is_some() {
            self.message = "Range selection cancelled".into();
            return;
        }
        if diff_location(&self.diff, self.diff_scroll).is_none() {
            self.message = "Select a diff content line first".into();
            return;
        }
        self.range_start = Some(self.diff_scroll);
        self.diff_comment_selected = None;
        self.diff_thread_selected = None;
        self.message = "Range selection: move with j/k, then press a".into();
    }

    pub fn cancel_range_selection(&mut self) -> bool {
        if self.range_start.take().is_some() {
            self.message = "Range selection cancelled".into();
            true
        } else {
            false
        }
    }

    pub fn range_contains(&self, index: usize) -> bool {
        self.range_start.is_some_and(|start| {
            let lower = start.min(self.diff_scroll);
            let upper = start.max(self.diff_scroll);
            (lower..=upper).contains(&index)
        })
    }

    fn start_range_comment(&mut self, range_start: usize) {
        let Some((start_file, start_line)) = diff_location(&self.diff, range_start) else {
            self.message = "Invalid range start".into();
            return;
        };
        let Some((end_file, end_line)) = diff_location(&self.diff, self.diff_scroll) else {
            self.message = "Select a diff content line".into();
            return;
        };
        if start_file != end_file {
            self.message = "A review range cannot cross files".into();
            return;
        }
        let Some(commit) = self.diff_oid.clone() else {
            return;
        };
        let (start, end) = if start_line <= end_line {
            (start_line, end_line)
        } else {
            (end_line, start_line)
        };
        let diff_index = range_start.max(self.diff_scroll);
        self.diff_scroll = diff_index;
        self.comment_anchor = Some(CommentAnchor {
            commit,
            scope: ReviewScope::Range {
                file: start_file,
                start,
                end,
            },
            diff_index,
            context: review_context(&self.diff, diff_index),
        });
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
        self.comment_followup_index = None;
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
            self.persist_reviews();
            return;
        }
        if let Some(index) = self.comment_followup_index.take() {
            if let Some(comment) = self.comments.get_mut(index) {
                comment.thread.push(ReviewThreadMessage {
                    role: ReviewThreadRole::Human,
                    body,
                });
                self.diff_comment_selected = Some(index);
                self.diff_thread_selected = comment.thread.len().checked_sub(1);
            }
            self.comment_buffer.clear();
            self.input_mode = InputMode::Normal;
            self.message = "Follow-up added".into();
            self.persist_reviews();
            return;
        }
        let Some(anchor) = self.comment_anchor.take() else {
            self.cancel_comment();
            return;
        };
        let comment_index = self.comments.len();
        self.comments.push(ReviewComment {
            commit: anchor.commit,
            scope: anchor.scope,
            diff_index: anchor.diff_index,
            body,
            context: anchor.context,
            thread: Vec::new(),
        });
        self.diff_comment_selected = Some(comment_index);
        self.diff_thread_selected = None;
        self.comment_buffer.clear();
        self.input_mode = InputMode::Normal;
        self.message = format!("{} review comments", self.comments.len());
        self.persist_reviews();
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
        self.comment_followup_index = None;
    }

    pub fn diff_files(&self) -> Vec<DiffFileSummary> {
        self.diff
            .iter()
            .enumerate()
            .filter(|(_, line)| line.starts_with("diff --git "))
            .filter_map(|(diff_index, _)| {
                let (path, _) = diff_file_anchor(&self.diff, diff_index)?;
                let review_count = self
                    .comments
                    .iter()
                    .filter(|comment| {
                        comment.commit == self.diff_oid.as_deref().unwrap_or_default()
                            && comment.scope.file() == Some(path.as_str())
                    })
                    .count();
                Some(DiffFileSummary {
                    agent_changed: self
                        .last_agent_changes
                        .iter()
                        .any(|changed| changed == &path),
                    path,
                    diff_index,
                    review_count,
                })
            })
            .collect()
    }

    pub fn open_files(&mut self) {
        let files = self.diff_files();
        if files.is_empty() {
            self.message = "No changed files".into();
            return;
        }
        self.file_selected = files
            .iter()
            .position(|file| file.diff_index >= self.diff_scroll)
            .unwrap_or(files.len() - 1);
        self.view = View::Files;
    }

    pub fn close_files(&mut self) {
        self.view = View::Diff;
    }

    pub fn move_file_selection(&mut self, amount: isize) {
        let count = self.diff_files().len();
        if amount < 0 {
            self.file_selected = self.file_selected.saturating_sub(amount.unsigned_abs());
        } else {
            self.file_selected = self
                .file_selected
                .saturating_add(amount as usize)
                .min(count.saturating_sub(1));
        }
    }

    pub fn select_diff_file(&mut self) {
        if let Some(file) = self.diff_files().get(self.file_selected) {
            self.diff_scroll = file.diff_index;
            self.diff_comment_selected = None;
            self.diff_thread_selected = None;
        }
        self.view = View::Diff;
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

    pub fn edit_inline_comment(&mut self) {
        let Some(index) = self.diff_comment_selected else {
            return;
        };
        if self.diff_thread_selected.is_some() {
            self.comment_buffer.clear();
            self.comment_followup_index = Some(index);
            self.input_mode = InputMode::Comment;
            return;
        }
        self.comment_selected = index;
        self.edit_selected_comment();
    }

    pub fn editing_comment_index(&self) -> Option<usize> {
        self.comment_edit_index
    }

    pub fn followup_comment_index(&self) -> Option<usize> {
        self.comment_followup_index
    }

    pub fn comment_scope_label(&self) -> Option<String> {
        self.comment_anchor
            .as_ref()
            .map(|anchor| anchor.scope.label())
            .or_else(|| {
                self.comment_edit_index
                    .or(self.comment_followup_index)
                    .and_then(|index| self.comments.get(index))
                    .map(|comment| comment.scope.label())
            })
    }

    pub fn delete_selected_comment(&mut self) {
        if self.comment_selected >= self.comments.len() {
            self.message = "No comments".into();
            return;
        }
        self.remove_comment_at(self.comment_selected);
        self.message = format!("{} review comments", self.comments.len());
        self.persist_reviews();
    }

    fn remove_comment_at(&mut self, removed: usize) {
        self.comments.remove(removed);
        self.diff_comment_selected = match self.diff_comment_selected {
            Some(selected) if selected == removed => None,
            Some(selected) if selected > removed => Some(selected - 1),
            selected => selected,
        };
        if self.diff_comment_selected.is_none() {
            self.diff_thread_selected = None;
        }
        self.comment_selected = self
            .comment_selected
            .min(self.comments.len().saturating_sub(1));
    }

    pub fn request_send_review(&mut self) {
        if self.dispatching {
            self.message = "A review is already being delivered".into();
            self.delivery_message = Some("Send blocked: a review is already sending".into());
        } else if self.agent_target.is_none() {
            self.message = "Set a target with :session <provider> <id>".into();
            self.delivery_message = Some("Send blocked: select an agent session with s".into());
        } else if self.comments.is_empty() {
            self.message = "No review comments to send".into();
            self.delivery_message = Some("Send blocked: no review comments".into());
        } else {
            self.delivery_message = None;
            self.pending_send_comment_index = if self.view == View::Diff {
                self.diff_comment_selected
                    .filter(|&index| index < self.comments.len())
            } else {
                None
            };
            self.input_mode = InputMode::ConfirmSend;
        }
    }

    pub fn pending_send_comment_index(&self) -> Option<usize> {
        self.pending_send_comment_index
    }

    pub fn dismiss_delivery_message(&mut self) {
        self.delivery_message = None;
    }

    pub fn cancel_send_review(&mut self) {
        self.input_mode = InputMode::Normal;
        self.pending_send_comment_index = None;
        self.message = "Review delivery cancelled".into();
        self.delivery_message = Some("Send cancelled".into());
    }

    pub fn confirm_send_review(&mut self) {
        let Some(target) = self.agent_target.clone() else {
            self.cancel_send_review();
            return;
        };
        let comments = self
            .pending_send_comment_index
            .and_then(|index| self.comments.get(index).cloned())
            .map(|comment| vec![comment])
            .unwrap_or_else(|| self.comments.clone());
        let prompt = format_review_request(&comments);
        self.dispatch_snapshot = git::change_snapshot().ok();
        let (tx, rx) = mpsc::channel();
        self.dispatched_agent = Some(target.response_label());
        self.delivery_message = Some(format!("Sending review to {}...", target.short_label()));
        self.dispatch_cancel = Some(agent::dispatch(target, prompt, tx));
        self.dispatched_comments = comments;
        self.pending_send_comment_index = None;
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
        match event {
            DispatchEvent::Progress(progress) => {
                self.message = progress.clone();
                self.delivery_message = Some(format!("Agent: {progress}"));
            }
            DispatchEvent::Done(mut response) => {
                self.dispatch_rx = None;
                self.dispatch_cancel = None;
                self.dispatching = false;
                let delivered = std::mem::take(&mut self.dispatched_comments);
                if let Some(before) = self.dispatch_snapshot.take()
                    && let Ok(after) = git::change_snapshot()
                {
                    self.last_agent_changes = changed_paths(&before, &after);
                    let requested_files = delivered
                        .iter()
                        .filter_map(|comment| comment.scope.file())
                        .collect::<Vec<_>>();
                    let target_changed = requested_files.iter().any(|file| {
                        self.last_agent_changes
                            .iter()
                            .any(|changed| changed == file)
                    });
                    let verification = if self.last_agent_changes.is_empty() {
                        "Verification: no new working-tree changes detected.".to_string()
                    } else if requested_files.is_empty() {
                        format!(
                            "Verification: changed {} file(s): {}.",
                            self.last_agent_changes.len(),
                            self.last_agent_changes.join(", ")
                        )
                    } else {
                        format!(
                            "Verification: changed {} file(s); requested file changed: {}. Files: {}.",
                            self.last_agent_changes.len(),
                            if target_changed { "yes" } else { "no" },
                            self.last_agent_changes.join(", ")
                        )
                    };
                    response.push_str("\n\n");
                    response.push_str(&verification);
                }
                let agent = self
                    .dispatched_agent
                    .take()
                    .unwrap_or_else(|| "agent".into());
                let mut attached_indices = Vec::new();
                for comment in delivered {
                    if let Some((index, item)) = self
                        .comments
                        .iter_mut()
                        .enumerate()
                        .find(|(_, item)| **item == comment)
                    {
                        item.thread.push(ReviewThreadMessage {
                            role: ReviewThreadRole::Agent(agent.clone()),
                            body: response.clone(),
                        });
                        attached_indices.push(index);
                    }
                }
                let attached = attached_indices.len();
                if let [index] = attached_indices.as_slice() {
                    self.diff_scroll = self.comments[*index].diff_index;
                    self.diff_comment_selected = Some(*index);
                    self.diff_thread_selected = self.comments[*index].thread.len().checked_sub(1);
                }
                self.message = if attached == 1 {
                    "Agent response received".into()
                } else {
                    format!("Agent response attached to {attached} comments")
                };
                self.delivery_message = Some(self.message.clone());
                self.persist_reviews();
            }
            DispatchEvent::Error(error) => {
                self.dispatch_rx = None;
                self.dispatch_cancel = None;
                self.dispatching = false;
                self.dispatched_comments.clear();
                self.dispatched_agent = None;
                self.dispatch_snapshot = None;
                self.message = error.clone();
                self.delivery_message = Some(format!("Send failed: {error}"));
            }
        }
        true
    }

    pub fn cancel_agent_dispatch(&mut self) -> bool {
        let Some(cancellation) = self.dispatch_cancel.take() else {
            return false;
        };
        cancellation.cancel();
        self.message = "Cancelling Agent...".into();
        self.delivery_message = Some(self.message.clone());
        true
    }

    pub fn view_agent_changes(&mut self) {
        if self.last_agent_changes.is_empty() {
            self.message = "No Agent changes detected".into();
            return;
        }
        self.open_worktree_diff();
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
                    self.reanchor_reviews();
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

    fn reanchor_reviews(&mut self) {
        let Some(id) = self.diff_oid.as_deref() else {
            return;
        };
        for comment in self
            .comments
            .iter_mut()
            .filter(|comment| comment.commit == id)
        {
            if let Some(index) = scope_diff_index(&self.diff, &comment.scope) {
                comment.diff_index = index;
            }
        }
        self.persist_reviews();
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
            Some("sessions") => self.open_sessions(),
            Some("worktree") => self.open_worktree_diff(),
            Some("staged") => self.open_staged_diff(),
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

fn diff_file_anchor(diff: &[String], cursor: usize) -> Option<(String, usize)> {
    let anchor = (0..=cursor.min(diff.len().saturating_sub(1)))
        .rev()
        .find(|&index| diff[index].starts_with("diff --git "))?;
    let end = ((anchor + 1)..diff.len())
        .find(|&index| diff[index].starts_with("diff --git "))
        .unwrap_or(diff.len());
    let file = diff[anchor..end]
        .iter()
        .find_map(|line| {
            line.strip_prefix("+++ b/")
                .or_else(|| line.strip_prefix("--- a/"))
        })
        .or_else(|| diff[anchor].rsplit_once(" b/").map(|(_, path)| path))?;
    Some((file.to_string(), anchor))
}

fn commit_comment_anchor(diff: &[String]) -> usize {
    diff.iter()
        .position(|line| line.starts_with("diff --git "))
        .map(|index| index.saturating_sub(1))
        .unwrap_or_else(|| diff.len().saturating_sub(1))
}

fn review_context(diff: &[String], index: usize) -> Vec<String> {
    if diff.is_empty() {
        return Vec::new();
    }
    let index = index.min(diff.len() - 1);
    let hunk_start = (0..=index)
        .rev()
        .find(|&cursor| diff[cursor].starts_with("@@"))
        .unwrap_or(index.saturating_sub(8));
    let hunk_end = ((index + 1)..diff.len())
        .find(|&cursor| diff[cursor].starts_with("@@") || diff[cursor].starts_with("diff --git "))
        .unwrap_or(diff.len());
    let start = hunk_start.max(index.saturating_sub(8));
    let end = hunk_end.min(index.saturating_add(9)).max(start + 1);
    diff[start..end].to_vec()
}

fn changed_paths(before: &git::ChangeSnapshot, after: &git::ChangeSnapshot) -> Vec<String> {
    let mut paths = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter(|path| before.get(path) != after.get(path))
        .collect()
}

fn scope_diff_index(diff: &[String], scope: &ReviewScope) -> Option<usize> {
    match scope {
        ReviewScope::Commit => (!diff.is_empty()).then(|| commit_comment_anchor(diff)),
        ReviewScope::File(expected) => diff
            .iter()
            .enumerate()
            .filter(|(_, line)| line.starts_with("diff --git "))
            .find_map(|(index, _)| {
                diff_file_anchor(diff, index)
                    .filter(|(file, _)| file == expected)
                    .map(|(_, anchor)| anchor)
            }),
        ReviewScope::Line { file, line } => diff.iter().enumerate().find_map(|(index, _)| {
            (diff_location(diff, index).as_ref() == Some(&(file.clone(), *line))).then_some(index)
        }),
        ReviewScope::Range { file, end, .. } => diff.iter().enumerate().find_map(|(index, _)| {
            (diff_location(diff, index).as_ref() == Some(&(file.clone(), *end))).then_some(index)
        }),
    }
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
            "{}. Commit: {}\n   Scope: {}\n   Human review: {}\n",
            index + 1,
            comment.commit,
            comment.scope.prompt_location(),
            comment.body
        ));
        if !comment.context.is_empty() {
            prompt.push_str("   Relevant diff context:\n```diff\n");
            for line in &comment.context {
                prompt.push_str(line);
                prompt.push('\n');
            }
            prompt.push_str("```\n");
        }
        for message in &comment.thread {
            let author = match &message.role {
                ReviewThreadRole::Human => "Human follow-up".to_string(),
                ReviewThreadRole::Agent(agent) => format!("Agent response ({agent})"),
            };
            prompt.push_str(&format!("   {author}: {}\n", message.body));
        }
        prompt.push('\n');
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
    fn session_picker_sets_target_and_returns_to_previous_view() {
        let mut app = App::new();
        app.view = View::Sessions;
        app.sessions_return = View::Diff;
        app.sessions = vec![AgentSession {
            target: AgentTarget::parse("codex", "019a-session").unwrap(),
            title: "Review session".into(),
            cwd: "/work/project".into(),
            status: "idle".into(),
            updated_at: 42,
        }];

        app.select_agent_session();

        assert_eq!(app.view, View::Diff);
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
    fn diff_cursor_moves_through_comments_and_enter_edits_them() {
        let mut app = App::new();
        app.diff_oid = Some("abc123".into());
        app.diff = vec!["line one".into(), "line two".into()];
        app.comments = vec![ReviewComment {
            commit: "abc123".into(),
            scope: ReviewScope::Line {
                file: "src/main.rs".into(),
                line: 1,
            },
            diff_index: 0,
            body: "Please simplify".into(),
            context: Vec::new(),
            thread: Vec::new(),
        }];

        app.move_diff_cursor(1);
        assert_eq!(app.diff_scroll, 0);
        assert_eq!(app.diff_comment_selected, Some(0));

        app.edit_inline_comment();
        assert_eq!(app.input_mode, InputMode::Comment);
        assert_eq!(app.comment_buffer, "Please simplify");
        assert_eq!(app.editing_comment_index(), Some(0));

        app.cancel_comment();
        app.move_diff_cursor(1);
        assert_eq!(app.diff_scroll, 1);
        assert_eq!(app.diff_comment_selected, None);

        app.move_diff_cursor(-1);
        assert_eq!(app.diff_scroll, 0);
        assert_eq!(app.diff_comment_selected, Some(0));
    }

    #[test]
    fn agent_response_accepts_a_follow_up_and_sends_the_thread_context() {
        let mut app = App::new();
        app.diff_oid = Some("abc123".into());
        app.diff = vec!["+changed".into()];
        app.comments = vec![ReviewComment {
            commit: "abc123".into(),
            scope: ReviewScope::Line {
                file: "src/main.rs".into(),
                line: 1,
            },
            diff_index: 0,
            body: "Please simplify".into(),
            context: Vec::new(),
            thread: vec![ReviewThreadMessage {
                role: ReviewThreadRole::Agent("codex:session-1".into()),
                body: "Implemented the simpler version.".into(),
            }],
        }];

        app.move_diff_cursor(1);
        app.move_diff_cursor(1);
        assert_eq!(app.diff_comment_selected, Some(0));
        assert_eq!(app.diff_thread_selected, Some(0));

        app.edit_inline_comment();
        assert_eq!(app.followup_comment_index(), Some(0));
        app.comment_buffer = "Please keep the original error text.".into();
        app.submit_comment();

        assert_eq!(
            app.comments[0].thread[1],
            ReviewThreadMessage {
                role: ReviewThreadRole::Human,
                body: "Please keep the original error text.".into(),
            }
        );
        assert_eq!(app.diff_thread_selected, Some(1));

        let prompt = format_review_request(&app.comments);
        assert!(prompt.contains("Agent response (codex:session-1): Implemented"));
        assert!(prompt.contains("Human follow-up: Please keep"));
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
                scope: ReviewScope::Line {
                    file: "src/main.rs".into(),
                    line: 11,
                },
                diff_index: 5,
                body: "Please simplify".into(),
                context: review_context(&app.diff, 5),
                thread: Vec::new(),
            }]
        );
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn file_and_commit_comments_use_scope_specific_anchors() {
        let mut app = App::new();
        app.diff_oid = Some("abc123".into());
        app.diff = vec![
            "commit abc123".into(),
            "Author: A".into(),
            "".into(),
            "    subject".into(),
            "".into(),
            "diff --git a/src/main.rs b/src/main.rs".into(),
            "--- a/src/main.rs".into(),
            "+++ b/src/main.rs".into(),
            "@@ -1 +1 @@".into(),
            "+changed".into(),
        ];

        app.diff_scroll = 9;
        app.start_file_comment();
        assert_eq!(app.diff_scroll, 5);
        assert_eq!(
            app.comment_scope_label().as_deref(),
            Some("FILE · src/main.rs")
        );
        app.comment_buffer = "Review the file structure".into();
        app.submit_comment();

        app.start_commit_comment();
        assert_eq!(app.diff_scroll, 4);
        assert_eq!(app.comment_scope_label().as_deref(), Some("COMMIT"));
        app.comment_buffer = "Review the overall approach".into();
        app.submit_comment();

        assert_eq!(
            app.comments[0].scope,
            ReviewScope::File("src/main.rs".into())
        );
        assert_eq!(app.comments[0].diff_index, 5);
        assert_eq!(app.comments[1].scope, ReviewScope::Commit);
        assert_eq!(app.comments[1].diff_index, 4);

        let prompt = format_review_request(&app.comments);
        assert!(prompt.contains("Scope: whole file: src/main.rs"));
        assert!(prompt.contains("Scope: whole commit"));
    }

    #[test]
    fn range_comment_records_lines_and_bounded_diff_context() {
        let mut app = App::new();
        app.diff_oid = Some("abc123".into());
        app.diff = vec![
            "diff --git a/src/main.rs b/src/main.rs".into(),
            "--- a/src/main.rs".into(),
            "+++ b/src/main.rs".into(),
            "@@ -10,3 +10,3 @@".into(),
            " context".into(),
            "-old".into(),
            "+new".into(),
            " tail".into(),
        ];
        app.diff_scroll = 4;
        app.toggle_range_selection();
        app.move_diff(2);
        app.start_comment();
        app.comment_buffer = "Keep these lines consistent".into();
        app.submit_comment();

        assert_eq!(
            app.comments[0].scope,
            ReviewScope::Range {
                file: "src/main.rs".into(),
                start: 10,
                end: 11,
            }
        );
        assert!(app.comments[0].context[0].starts_with("@@"));
        let prompt = format_review_request(&app.comments);
        assert!(prompt.contains("Scope: line range: src/main.rs:10-11"));
        assert!(prompt.contains("Relevant diff context:"));
        assert!(prompt.contains("-old\n+new"));
    }

    #[test]
    fn comments_can_be_edited_and_deleted() {
        let mut app = App::new();
        app.comments.push(ReviewComment {
            commit: "abc123".into(),
            scope: ReviewScope::Line {
                file: "src/main.rs".into(),
                line: 11,
            },
            diff_index: 5,
            body: "Old body".into(),
            context: Vec::new(),
            thread: Vec::new(),
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
        assert_eq!(
            app.delivery_message.as_deref(),
            Some("Send blocked: select an agent session with s")
        );

        app.agent_target = Some(AgentTarget::parse("codex", "session-1").unwrap());
        app.comments.push(ReviewComment {
            commit: "abc123".into(),
            scope: ReviewScope::Line {
                file: "src/main.rs".into(),
                line: 4,
            },
            diff_index: 3,
            body: "Handle this error".into(),
            context: Vec::new(),
            thread: Vec::new(),
        });
        app.request_send_review();
        assert_eq!(app.input_mode, InputMode::ConfirmSend);

        let prompt = format_review_request(&app.comments);
        assert!(prompt.contains("src/main.rs:4"));
        assert!(prompt.contains("Handle this error"));
    }

    #[test]
    fn selected_inline_comment_is_the_single_send_scope() {
        let mut app = App::new();
        app.view = View::Diff;
        app.agent_target = Some(AgentTarget::parse("codex", "session-1").unwrap());
        app.comments = vec![
            ReviewComment {
                commit: "abc123".into(),
                scope: ReviewScope::Line {
                    file: "src/main.rs".into(),
                    line: 4,
                },
                diff_index: 3,
                body: "First comment".into(),
                context: Vec::new(),
                thread: Vec::new(),
            },
            ReviewComment {
                commit: "abc123".into(),
                scope: ReviewScope::Line {
                    file: "src/main.rs".into(),
                    line: 8,
                },
                diff_index: 7,
                body: "Second comment".into(),
                context: Vec::new(),
                thread: Vec::new(),
            },
        ];
        app.diff_comment_selected = Some(1);

        app.request_send_review();

        assert_eq!(app.input_mode, InputMode::ConfirmSend);
        assert_eq!(app.pending_send_comment_index(), Some(1));

        app.cancel_send_review();
        assert_eq!(app.pending_send_comment_index(), None);
    }

    #[test]
    fn completed_single_send_attaches_response_to_the_sent_snapshot() {
        let mut app = App::new();
        let delivered = ReviewComment {
            commit: "abc123".into(),
            scope: ReviewScope::Line {
                file: "src/main.rs".into(),
                line: 4,
            },
            diff_index: 3,
            body: "Delivered".into(),
            context: Vec::new(),
            thread: Vec::new(),
        };
        let remaining = ReviewComment {
            body: "Still pending".into(),
            ..delivered.clone()
        };
        app.comments = vec![delivered.clone(), remaining.clone()];
        app.dispatched_comments = vec![delivered];
        app.dispatching = true;
        let (tx, rx) = mpsc::channel();
        app.dispatched_agent = Some("codex:session-1".into());
        tx.send(DispatchEvent::Done("Implemented and tested.".into()))
            .unwrap();
        app.dispatch_rx = Some(rx);

        assert!(app.drain_dispatch_events());

        assert_eq!(app.comments.len(), 2);
        assert_eq!(app.comments[1], remaining);
        assert_eq!(
            app.comments[0].thread,
            [ReviewThreadMessage {
                role: ReviewThreadRole::Agent("codex:session-1".into()),
                body: "Implemented and tested.".into(),
            }]
        );
        assert_eq!(app.diff_comment_selected, Some(0));
        assert_eq!(app.diff_thread_selected, Some(0));
        assert_eq!(app.message, "Agent response received");
    }

    #[test]
    fn dispatch_progress_keeps_the_agent_run_active() {
        let mut app = App::new();
        let (tx, rx) = mpsc::channel();
        app.dispatching = true;
        app.dispatch_rx = Some(rx);
        tx.send(DispatchEvent::Progress("Running tests...".into()))
            .unwrap();

        assert!(app.drain_dispatch_events());
        assert!(app.dispatching);
        assert!(app.dispatch_rx.is_some());
        assert_eq!(
            app.delivery_message.as_deref(),
            Some("Agent: Running tests...")
        );
    }

    #[test]
    fn verification_detects_added_changed_and_removed_paths() {
        let before = git::ChangeSnapshot::from([
            ("changed.rs".into(), 1),
            ("removed.rs".into(), 2),
            ("same.rs".into(), 3),
        ]);
        let after = git::ChangeSnapshot::from([
            ("added.rs".into(), 4),
            ("changed.rs".into(), 5),
            ("same.rs".into(), 3),
        ]);

        assert_eq!(
            changed_paths(&before, &after),
            ["added.rs", "changed.rs", "removed.rs"]
        );
    }

    #[test]
    fn file_navigator_counts_reviews_and_marks_agent_changes() {
        let mut app = App::new();
        app.diff_oid = Some("WORKTREE".into());
        app.diff = vec![
            "diff --git a/src/a.rs b/src/a.rs".into(),
            "--- a/src/a.rs".into(),
            "+++ b/src/a.rs".into(),
            "@@ -1 +1 @@".into(),
            "+a".into(),
            "diff --git a/src/b.rs b/src/b.rs".into(),
            "--- a/src/b.rs".into(),
            "+++ b/src/b.rs".into(),
        ];
        app.comments.push(ReviewComment {
            commit: "WORKTREE".into(),
            scope: ReviewScope::File("src/a.rs".into()),
            diff_index: 0,
            body: "Review A".into(),
            context: Vec::new(),
            thread: Vec::new(),
        });
        app.last_agent_changes = vec!["src/b.rs".into()];

        let files = app.diff_files();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].review_count, 1);
        assert!(!files[0].agent_changed);
        assert!(files[1].agent_changed);

        app.open_files();
        app.move_file_selection(1);
        app.select_diff_file();
        assert_eq!(app.view, View::Diff);
        assert_eq!(app.diff_scroll, 5);
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
