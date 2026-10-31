use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::mpsc::Sender,
    thread,
};

const RECORD_SEPARATOR: char = '\u{1e}';
const FIELD_SEPARATOR: char = '\u{1f}';
const LOG_FORMAT: &str = "%x1e%H%x1f%h%x1f%ad%x1f%D%x1f%s";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitLine {
    pub graph: String,
    pub oid: String,
    pub short_oid: String,
    pub date: String,
    pub refs: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryLine {
    Commit(CommitLine),
    Graph(String),
}

impl HistoryLine {
    pub fn commit(&self) -> Option<&CommitLine> {
        match self {
            Self::Commit(commit) => Some(commit),
            Self::Graph(_) => None,
        }
    }

    pub fn display_text(&self) -> String {
        match self {
            Self::Graph(graph) => graph.clone(),
            Self::Commit(commit) => {
                let refs = if commit.refs.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", commit.refs)
                };
                format!(
                    "{}{} {}{} {}",
                    commit.graph, commit.short_oid, commit.date, refs, commit.subject
                )
            }
        }
    }
}

pub enum HistoryEvent {
    Line(HistoryLine),
    Done,
    Error(String),
}

pub fn stream_history(revisions: Vec<String>, sender: Sender<HistoryEvent>) {
    thread::spawn(move || {
        if let Err(error) = run_git_log(&revisions, &sender) {
            let _ = sender.send(HistoryEvent::Error(error));
        }
    });
}

fn run_git_log(revisions: &[String], sender: &Sender<HistoryEvent>) -> Result<(), String> {
    let mut command = Command::new("git");
    command
        .args([
            "--no-pager",
            "log",
            "--graph",
            "--color=never",
            "--date=short",
            "--decorate=short",
            &format!("--format={LOG_FORMAT}"),
        ])
        .args(revisions)
        .env("GIT_PAGER", "cat")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to start git: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture git output".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture git error output".to_string())?;

    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("Failed to read git history: {error}"))?;
        if sender
            .send(HistoryEvent::Line(parse_history_line(&line)))
            .is_err()
        {
            let _ = child.kill();
            return Ok(());
        }
    }

    let status = child
        .wait()
        .map_err(|error| format!("Failed to wait for git: {error}"))?;
    if status.success() {
        let _ = sender.send(HistoryEvent::Done);
        return Ok(());
    }

    let mut error = String::new();
    stderr
        .read_to_string(&mut error)
        .map_err(|read_error| format!("git failed and stderr was unreadable: {read_error}"))?;
    let error = error.trim();
    Err(if error.is_empty() {
        format!("git log exited with {status}")
    } else {
        error.to_string()
    })
}

pub fn parse_history_line(line: &str) -> HistoryLine {
    let Some(marker) = line.find(RECORD_SEPARATOR) else {
        return HistoryLine::Graph(line.to_string());
    };

    let graph = line[..marker].to_string();
    let fields: Vec<_> = line[marker + RECORD_SEPARATOR.len_utf8()..]
        .splitn(5, FIELD_SEPARATOR)
        .collect();
    if fields.len() != 5 {
        return HistoryLine::Graph(line.to_string());
    }

    HistoryLine::Commit(CommitLine {
        graph,
        oid: fields[0].to_string(),
        short_oid: fields[1].to_string(),
        date: fields[2].to_string(),
        refs: fields[3].to_string(),
        subject: fields[4].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commit_record_with_graph_prefix() {
        let line = "| * \u{1e}012345\u{1f}012345\u{1f}2026-11-01\u{1f}HEAD -> main, tag: v1\u{1f}Initial commit";

        let parsed = parse_history_line(line);

        assert_eq!(
            parsed,
            HistoryLine::Commit(CommitLine {
                graph: "| * ".into(),
                oid: "012345".into(),
                short_oid: "012345".into(),
                date: "2026-11-01".into(),
                refs: "HEAD -> main, tag: v1".into(),
                subject: "Initial commit".into(),
            })
        );
    }

    #[test]
    fn preserves_graph_only_lines() {
        assert_eq!(parse_history_line("|\\"), HistoryLine::Graph("|\\".into()));
    }

    #[test]
    fn malformed_record_is_kept_as_graph_text() {
        let line = "* \u{1e}missing-fields";
        assert_eq!(parse_history_line(line), HistoryLine::Graph(line.into()));
    }
}
