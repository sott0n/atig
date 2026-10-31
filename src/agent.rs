use std::{
    io::Write,
    process::{Command, Stdio},
    sync::mpsc::Sender,
    thread,
};

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

pub enum DispatchEvent {
    Done,
    Error(String),
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
        let provider = match &self.provider {
            AgentProvider::Codex => "codex",
            AgentProvider::Claude => "claude",
            AgentProvider::Other(name) => name,
        };
        format!("{provider}:{}", self.session_id)
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
    }

    #[test]
    fn rejects_session_id_with_shell_characters() {
        assert!(AgentTarget::parse("claude", "id;rm").is_err());
        assert!(AgentTarget::parse("claude", "id with spaces").is_err());
    }
}
