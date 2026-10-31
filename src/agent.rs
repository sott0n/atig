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
