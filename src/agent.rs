use std::{
    cmp::Reverse,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::Sender,
    thread,
    time::UNIX_EPOCH,
};

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentProvider {
    Codex,
    Claude,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTarget {
    pub provider: AgentProvider,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSession {
    pub target: AgentTarget,
    pub title: String,
    pub cwd: String,
    pub status: String,
    pub updated_at: u64,
}

pub enum SessionListEvent {
    Done(Vec<AgentSession>),
    Error(String),
}

pub enum DispatchEvent {
    Done,
    Error(String),
}

pub fn discover_sessions(sender: Sender<SessionListEvent>) {
    thread::spawn(move || {
        let event = match discover_sessions_inner() {
            Ok(sessions) => SessionListEvent::Done(sessions),
            Err(error) => SessionListEvent::Error(error),
        };
        let _ = sender.send(event);
    });
}

fn discover_sessions_inner() -> Result<Vec<AgentSession>, String> {
    let repository = repository_info()?;
    let mut sessions = Vec::new();
    let mut errors = Vec::new();

    match discover_codex_sessions(&repository) {
        Ok(found) => sessions.extend(found),
        Err(error) => errors.push(format!("Codex: {error}")),
    }
    match discover_claude_sessions(&repository) {
        Ok(found) => sessions.extend(found),
        Err(error) => errors.push(format!("Claude: {error}")),
    }
    sessions.sort_by_key(|session| Reverse(session.updated_at));

    if sessions.is_empty() && !errors.is_empty() {
        Err(errors.join("; "))
    } else {
        Ok(sessions)
    }
}

struct RepositoryInfo {
    root: PathBuf,
    origin: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeSessionMetadata {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
}

fn repository_info() -> Result<RepositoryInfo, String> {
    let root = command_output(Command::new("git").args(["rev-parse", "--show-toplevel"]))?;
    let root =
        fs::canonicalize(root.trim()).map_err(|error| format!("Invalid Git root: {error}"))?;
    let origin = command_output(Command::new("git").args(["config", "--get", "remote.origin.url"]))
        .ok()
        .map(|origin| origin.trim().to_string())
        .filter(|origin| !origin.is_empty());
    Ok(RepositoryInfo { root, origin })
}

fn command_output(command: &mut Command) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("Failed to start command: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn discover_codex_sessions(repository: &RepositoryInfo) -> Result<Vec<AgentSession>, String> {
    let mut child = Command::new("codex")
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Failed to start app-server: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Failed to open app-server input".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to open app-server output".to_string())?;

    for request in [
        json!({
            "id": 1,
            "method": "initialize",
            "params": {"clientInfo": {"name": "atig", "title": "atig", "version": env!("CARGO_PKG_VERSION")}}
        }),
        json!({"method": "initialized", "params": {}}),
        json!({
            "id": 2,
            "method": "thread/list",
            "params": {"limit": 200, "sortKey": "updated_at", "sortDirection": "desc"}
        }),
    ] {
        writeln!(stdin, "{request}")
            .map_err(|error| format!("app-server write failed: {error}"))?;
    }
    stdin
        .flush()
        .map_err(|error| format!("app-server flush failed: {error}"))?;

    let mut response = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| format!("app-server read failed: {error}"))?;
        let value: Value = serde_json::from_str(&line)
            .map_err(|error| format!("Invalid app-server response: {error}"))?;
        if value.get("id").and_then(Value::as_i64) == Some(2) {
            response = Some(value);
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let response = response.ok_or_else(|| "app-server returned no thread list".to_string())?;
    if let Some(error) = response.get("error") {
        return Err(format!("thread/list failed: {error}"));
    }

    let threads = response
        .pointer("/result/data")
        .and_then(Value::as_array)
        .ok_or_else(|| "thread/list response had no data".to_string())?;
    Ok(threads
        .iter()
        .filter(|thread| thread.get("parentThreadId").is_none_or(Value::is_null))
        .filter(|thread| codex_thread_matches_repository(thread, repository))
        .filter_map(codex_session_from_value)
        .collect())
}

fn codex_thread_matches_repository(thread: &Value, repository: &RepositoryInfo) -> bool {
    let origin_matches = repository.origin.as_ref().is_some_and(|origin| {
        thread
            .pointer("/gitInfo/originUrl")
            .and_then(Value::as_str)
            .is_some_and(|candidate| candidate == origin)
    });
    origin_matches
        || thread
            .get("cwd")
            .and_then(Value::as_str)
            .is_some_and(|cwd| path_is_in_repository(Path::new(cwd), &repository.root))
}

fn codex_session_from_value(thread: &Value) -> Option<AgentSession> {
    let session_id = thread.get("id")?.as_str()?;
    let title = thread
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .or_else(|| thread.get("preview").and_then(Value::as_str))
        .unwrap_or("Codex session");
    Some(AgentSession {
        target: AgentTarget::parse("codex", session_id).ok()?,
        title: truncate(title, 80),
        cwd: thread.get("cwd")?.as_str()?.to_string(),
        status: thread
            .pointer("/status/type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        updated_at: thread.get("updatedAt")?.as_u64()?,
    })
}

fn discover_claude_sessions(repository: &RepositoryInfo) -> Result<Vec<AgentSession>, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    let projects = PathBuf::from(home).join(".claude/projects");
    if !projects.is_dir() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for project in
        fs::read_dir(projects).map_err(|error| format!("Cannot read projects: {error}"))?
    {
        let project = project.map_err(|error| format!("Cannot read project entry: {error}"))?;
        if !project.path().is_dir() {
            continue;
        }
        for entry in fs::read_dir(project.path())
            .map_err(|error| format!("Cannot read Claude project: {error}"))?
        {
            let entry = entry.map_err(|error| format!("Cannot read session entry: {error}"))?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(session) = claude_session_from_file(&path, repository) {
                sessions.push(session);
            }
        }
    }
    Ok(sessions)
}

fn claude_session_from_file(path: &Path, repository: &RepositoryInfo) -> Option<AgentSession> {
    let first_line = BufReader::new(fs::File::open(path).ok()?)
        .lines()
        .next()?
        .ok()?;
    let metadata: ClaudeSessionMetadata = serde_json::from_str(&first_line).ok()?;
    if !path_is_in_repository(Path::new(&metadata.cwd), &repository.root) {
        return None;
    }
    let updated_at = path
        .metadata()
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(AgentSession {
        target: AgentTarget::parse("claude", &metadata.session_id).ok()?,
        title: format!(
            "Claude {}",
            metadata.session_id.chars().take(8).collect::<String>()
        ),
        cwd: metadata.cwd,
        status: "saved".into(),
        updated_at,
    })
}

fn path_is_in_repository(candidate: &Path, root: &Path) -> bool {
    fs::canonicalize(candidate)
        .ok()
        .is_some_and(|candidate| candidate.starts_with(root))
}

fn truncate(value: &str, width: usize) -> String {
    let mut characters = value.chars();
    let mut result = characters.by_ref().take(width).collect::<String>();
    if characters.next().is_some() {
        result.pop();
        result.push('…');
    }
    result
}

pub fn dispatch(target: AgentTarget, prompt: String, sender: Sender<DispatchEvent>) {
    thread::spawn(move || {
        let result = match target.provider {
            AgentProvider::Codex => dispatch_codex(&target.session_id, &prompt),
            AgentProvider::Claude => dispatch_claude(&target.session_id, &prompt),
            AgentProvider::Other(_) => {
                Err("The other provider requires a custom adapter; review was not sent".into())
            }
        };
        let event = match result {
            Ok(()) => DispatchEvent::Done,
            Err(error) => DispatchEvent::Error(error),
        };
        let _ = sender.send(event);
    });
}

fn dispatch_codex(session_id: &str, prompt: &str) -> Result<(), String> {
    let mut child = Command::new("codex")
        .args(["exec", "resume", session_id, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start Codex: {error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "Failed to open Codex input".to_string())?
        .write_all(prompt.as_bytes())
        .map_err(|error| format!("Failed to send review to Codex: {error}"))?;
    finish_dispatch(child, "Codex")
}

fn dispatch_claude(session_id: &str, prompt: &str) -> Result<(), String> {
    let child = Command::new("claude")
        .args([
            "-p",
            "--resume",
            session_id,
            "--permission-prompts",
            "none",
            prompt,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start Claude: {error}"))?;
    finish_dispatch(child, "Claude")
}

fn finish_dispatch(child: std::process::Child, name: &str) -> Result<(), String> {
    let output = child
        .wait_with_output()
        .map_err(|error| format!("Failed to wait for {name}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        Err(format!("{name} exited with {}", output.status))
    } else {
        Err(format!("{name}: {stderr}"))
    }
}

impl AgentTarget {
    pub fn parse(provider: &str, session_id: &str) -> Result<Self, String> {
        if session_id.is_empty()
            || session_id.len() > 200
            || !session_id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
        {
            return Err("Session ID may only contain letters, numbers, -, _, ., and :".into());
        }

        let provider = match provider.to_ascii_lowercase().as_str() {
            "codex" => AgentProvider::Codex,
            "claude" => AgentProvider::Claude,
            "other" => AgentProvider::Other("other".into()),
            _ => return Err("Provider must be codex, claude, or other".into()),
        };
        Ok(Self {
            provider,
            session_id: session_id.to_string(),
        })
    }

    pub fn label(&self) -> String {
        format!("{}:{}", self.provider_name(), self.session_id)
    }

    pub fn short_label(&self) -> String {
        const VISIBLE_ID_CHARS: usize = 12;
        let mut id = self.session_id.chars();
        let visible = id.by_ref().take(VISIBLE_ID_CHARS).collect::<String>();
        let suffix = if id.next().is_some() { "…" } else { "" };
        format!("{}:{visible}{suffix}", self.provider_name())
    }

    fn provider_name(&self) -> &str {
        match &self.provider {
            AgentProvider::Codex => "codex",
            AgentProvider::Claude => "claude",
            AgentProvider::Other(name) => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_provider_and_safe_session_id() {
        let target = AgentTarget::parse("codex", "019a-test.session").unwrap();

        assert_eq!(target.provider, AgentProvider::Codex);
        assert_eq!(target.label(), "codex:019a-test.session");
        assert_eq!(target.short_label(), "codex:019a-test.se…");
    }

    #[test]
    fn rejects_session_id_with_shell_characters() {
        assert!(AgentTarget::parse("claude", "id;rm").is_err());
        assert!(AgentTarget::parse("claude", "id with spaces").is_err());
    }

    #[test]
    fn parses_codex_thread_metadata_without_turn_contents() {
        let value = json!({
            "id": "019a-session",
            "name": null,
            "preview": "Review the parser",
            "cwd": "/work/project",
            "status": {"type": "idle"},
            "updatedAt": 42
        });

        let session = codex_session_from_value(&value).unwrap();

        assert_eq!(session.target.label(), "codex:019a-session");
        assert_eq!(session.title, "Review the parser");
        assert_eq!(session.status, "idle");
        assert_eq!(session.updated_at, 42);
    }

    #[test]
    fn truncates_titles_on_character_boundaries() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("日本語", 4), "日本語");
    }
}
