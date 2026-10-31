use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::git::{self, HistoryEvent, HistoryLine};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
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
    history_rx: Option<Receiver<HistoryEvent>>,
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
            history_rx: None,
            pending_graph: None,
        }
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
}
