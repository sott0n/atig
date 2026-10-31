use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::mpsc::Sender,
    thread,
};

const RECORD_SEPARATOR: char = '\u{1e}';
const FIELD_SEPARATOR: char = '\u{1f}';
const LOG_FORMAT: &str = "%x1e%H%x1f%h%x1f%P%x1f%ad%x1f%aN%x1f%D%x1f%s";
const DATE_WIDTH: usize = 22;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitLine {
    pub graph: String,
    pub oid: String,
    pub short_oid: String,
    pub parents: Vec<String>,
    pub date: String,
    pub author: String,
    pub refs: String,
    pub subject: String,
    pub preceding_graph: Option<String>,
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

    pub fn display_text(&self, author_width: usize) -> String {
        match self {
            Self::Graph(graph) => {
                format!("{}{}", " ".repeat(DATE_WIDTH + author_width + 2), graph)
            }
            Self::Commit(commit) => {
                let author = fit_column(&commit.author, author_width);
                let refs = format_refs(&commit.refs);
                let refs = if refs.is_empty() {
                    String::new()
                } else {
                    format!("{refs} ")
                };
                format!(
                    "{} {} {}{}{}",
                    commit.date,
                    author,
                    commit.tig_graph(),
                    refs,
                    commit.subject
                )
            }
        }
    }
}

impl CommitLine {
    pub(crate) fn tig_graph(&self) -> String {
        let marker = match self.parents.len() {
            0 => 'I',
            1 => 'o',
            _ => 'M',
        };
        let mut replaced_marker = false;
        let mut graph = self
            .graph
            .chars()
            .filter_map(|character| match character {
                '*' if !replaced_marker => {
                    replaced_marker = true;
                    Some(marker)
                }
                '|' => Some('│'),
                '\\' => Some('┐'),
                '/' => Some('┘'),
                ' ' => Some(' '),
                _ => None,
            })
            .collect::<String>();
        while graph.ends_with(' ') {
            graph.pop();
        }

        if self.parents.len() > 1 {
            graph.push_str("─┐");
        } else if self.parents.is_empty()
            && self
                .preceding_graph
                .as_deref()
                .is_some_and(|line| line.contains('/'))
        {
            graph.push_str("─┘");
        }
        graph.push(' ');
        graph
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
            "--date=format-local:%Y-%m-%d %H:%M %z",
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
        .splitn(7, FIELD_SEPARATOR)
        .collect();
    if fields.len() != 7 {
        return HistoryLine::Graph(line.to_string());
    }

    HistoryLine::Commit(CommitLine {
        graph,
        oid: fields[0].to_string(),
        short_oid: fields[1].to_string(),
        parents: fields[2].split_whitespace().map(str::to_owned).collect(),
        date: fields[3].to_string(),
        author: fields[4].to_string(),
        refs: fields[5].to_string(),
        subject: fields[6].to_string(),
        preceding_graph: None,
    })
}

pub(crate) fn fit_column(value: &str, width: usize) -> String {
    let length = value.chars().count();
    if length <= width {
        return format!("{value:<width$}");
    }
    if width == 0 {
        return String::new();
    }

    let mut truncated = value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    truncated.push('~');
    truncated
}

pub(crate) fn format_refs(refs: &str) -> String {
    refs.split(", ")
        .filter(|name| !name.is_empty())
        .map(|name| {
            if let Some(branch) = name.strip_prefix("HEAD -> ") {
                format!("[{branch}]")
            } else if let Some(tag) = name.strip_prefix("tag: ") {
                format!("<{tag}>")
            } else if name.contains('/') {
                format!("{{{name}}}")
            } else {
                format!("[{name}]")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commit_record_with_graph_prefix() {
        let line = "| * \u{1e}012345\u{1f}012345\u{1f}parent\u{1f}2026-11-01 10:30 +0900\u{1f}Alice\u{1f}HEAD -> main, tag: v1\u{1f}Initial commit";

        let parsed = parse_history_line(line);

        assert_eq!(
            parsed,
            HistoryLine::Commit(CommitLine {
                graph: "| * ".into(),
                oid: "012345".into(),
                short_oid: "012345".into(),
                parents: vec!["parent".into()],
                date: "2026-11-01 10:30 +0900".into(),
                author: "Alice".into(),
                refs: "HEAD -> main, tag: v1".into(),
                subject: "Initial commit".into(),
                preceding_graph: None,
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

    #[test]
    fn renders_tig_style_columns_and_refs() {
        let line = parse_history_line(
            "* \u{1e}012345\u{1f}012345\u{1f}\u{1f}2026-11-01 10:30 +0900\u{1f}Alice\u{1f}HEAD -> main, tag: v1, origin/main\u{1f}Initial commit",
        );

        assert_eq!(
            line.display_text(8),
            "2026-11-01 10:30 +0900 Alice    I [main] <v1> {origin/main} Initial commit"
        );
    }
}
