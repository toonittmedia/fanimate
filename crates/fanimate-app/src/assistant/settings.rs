//! What the user chose about the Assistant: which provider and model, which
//! destinations they have allowed Fanimate to send messages to, and the two
//! switches (all AI off; let other programs control the project).
//!
//! Keys are not here: they are in `keys.rs`, never in the settings file that
//! is saved with the window layout, and never in a project.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderKind {
    #[default]
    Anthropic,
    OpenRouter,
    /// Any service or local server that speaks the OpenAI chat API: OpenAI
    /// itself, Ollama, LM Studio, a studio's own gateway.
    Custom,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 3] = [Self::Anthropic, Self::OpenRouter, Self::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic (Claude)",
            Self::OpenRouter => "OpenRouter",
            Self::Custom => "Other (OpenAI-compatible)",
        }
    }

    /// The environment variable that supplies a key without saving one.
    pub fn key_variable(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::Custom => "OPENAI_API_KEY",
        }
    }

    /// Where a person gets a key.
    pub fn key_page(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("https://console.anthropic.com/settings/keys"),
            Self::OpenRouter => Some("https://openrouter.ai/keys"),
            Self::Custom => None,
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => "claude-sonnet-5-5",
            Self::OpenRouter => "anthropic/claude-sonnet-5-5",
            Self::Custom => "",
        }
    }

    /// A few models worth offering in the drop-down; any name can be typed.
    pub fn suggestions(self) -> &'static [&'static str] {
        match self {
            Self::Anthropic => &[
                "claude-sonnet-5-5",
                "claude-opus-5-5",
                "claude-haiku-5-5",
                "claude-fable-5-1",
            ],
            Self::OpenRouter => &[
                "anthropic/claude-sonnet-5-5",
                "anthropic/claude-opus-5-5",
                "anthropic/claude-haiku-5-5",
            ],
            Self::Custom => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub provider: ProviderKind,
    pub anthropic_model: String,
    pub openrouter_model: String,
    pub custom_model: String,
    pub custom_name: String,
    /// Everything before `/chat/completions`, for the other provider.
    pub custom_url: String,
    /// The destinations the user has allowed Fanimate to send to
    /// (`target_id`), asked once each.
    pub allowed: Vec<String>,
    /// Turn off every AI feature: the panel, Ctrl+K and the link for other
    /// programs.
    pub off: bool,
    /// Let other programs (Claude Code, Codex, Antigravity) work on the open
    /// project through `fanimate-cli mcp --app`.
    pub programs: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Anthropic,
            anthropic_model: ProviderKind::Anthropic.default_model().to_owned(),
            openrouter_model: ProviderKind::OpenRouter.default_model().to_owned(),
            custom_model: String::new(),
            custom_name: "Local model".to_owned(),
            custom_url: super::OLLAMA_URL.to_owned(),
            allowed: Vec::new(),
            off: false,
            programs: false,
        }
    }
}

impl AiSettings {
    pub fn model(&self) -> &str {
        match self.provider {
            ProviderKind::Anthropic => &self.anthropic_model,
            ProviderKind::OpenRouter => &self.openrouter_model,
            ProviderKind::Custom => &self.custom_model,
        }
    }

    pub fn model_mut(&mut self) -> &mut String {
        match self.provider {
            ProviderKind::Anthropic => &mut self.anthropic_model,
            ProviderKind::OpenRouter => &mut self.openrouter_model,
            ProviderKind::Custom => &mut self.custom_model,
        }
    }

    /// What the chosen destination is called in the panel.
    pub fn name(&self) -> String {
        match self.provider {
            ProviderKind::Custom => self.custom_name.trim().to_owned(),
            other => other.label().to_owned(),
        }
    }

    /// Who receives the messages: the host name, for the permission prompt.
    pub fn destination(&self) -> String {
        match self.provider {
            ProviderKind::Anthropic => "api.anthropic.com".to_owned(),
            ProviderKind::OpenRouter => "openrouter.ai".to_owned(),
            ProviderKind::Custom => host_of(&self.custom_url),
        }
    }

    /// The identity a permission is remembered under.
    pub fn target_id(&self) -> String {
        match self.provider {
            ProviderKind::Anthropic => "anthropic".to_owned(),
            ProviderKind::OpenRouter => "openrouter".to_owned(),
            ProviderKind::Custom => format!("custom:{}", self.custom_url.trim().trim_end_matches('/')),
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.allowed.contains(&self.target_id())
    }

    pub fn allow(&mut self) {
        let id = self.target_id();
        if !self.allowed.contains(&id) {
            self.allowed.push(id);
        }
    }

    /// Whether the messages stay on this computer (a loopback address). Local
    /// models need neither a key nor the network.
    pub fn is_local(&self) -> bool {
        self.provider == ProviderKind::Custom && is_loopback(&self.custom_url)
    }
}

/// The host part of an address (`example.com` from `https://example.com:8080/v1`).
pub fn host_of(url: &str) -> String {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if let Some(inner) = authority.strip_prefix('[') {
        return inner.split(']').next().unwrap_or("").to_owned();
    }
    authority.split(':').next().unwrap_or("").to_owned()
}

/// Whether an address is on this computer.
pub fn is_loopback(url: &str) -> bool {
    let host = host_of(url).to_ascii_lowercase();
    let numeric_loopback = host
        .strip_prefix("127.")
        .is_some_and(|rest| rest.split('.').count() == 3 && rest.split('.').all(|p| p.parse::<u8>().is_ok()));
    host == "localhost" || host == "::1" || host == "0.0.0.0" || numeric_loopback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_come_out_of_addresses() {
        assert_eq!(host_of("https://openrouter.ai/api/v1"), "openrouter.ai");
        assert_eq!(host_of("http://localhost:11434/v1"), "localhost");
        assert_eq!(host_of("https://user:pw@example.com:8443/x"), "example.com");
        assert_eq!(host_of("http://[::1]:8080/v1"), "::1");
        assert_eq!(host_of("example.com/v1"), "example.com");
        assert_eq!(host_of(""), "");
    }

    #[test]
    fn only_this_computer_is_local() {
        assert!(is_loopback("http://localhost:11434/v1"));
        assert!(is_loopback("http://127.0.0.1:1234/v1"));
        assert!(is_loopback("http://[::1]/v1"));
        assert!(!is_loopback("https://api.openai.com/v1"));
        // a name that merely starts like localhost is somewhere else
        assert!(!is_loopback("http://localhost.evil.example/v1"));
        assert!(!is_loopback("http://127.example.com/v1"));
    }

    #[test]
    fn permission_is_remembered_per_destination() {
        let mut ai = AiSettings::default();
        assert!(!ai.is_allowed());
        ai.allow();
        ai.allow();
        assert!(ai.is_allowed());
        assert_eq!(ai.allowed.len(), 1);
        ai.provider = ProviderKind::OpenRouter;
        assert!(!ai.is_allowed(), "another destination asks again");
        ai.provider = ProviderKind::Custom;
        ai.custom_url = "https://gateway.studio.example/v1/".into();
        ai.allow();
        ai.custom_url = "https://gateway.studio.example/v1".into();
        assert!(ai.is_allowed(), "a trailing slash is the same place");
        ai.custom_url = "https://other.example/v1".into();
        assert!(!ai.is_allowed());
    }

    #[test]
    fn each_provider_has_its_own_model() {
        let mut ai = AiSettings::default();
        assert_eq!(ai.model(), "claude-sonnet-5-5");
        ai.provider = ProviderKind::OpenRouter;
        assert_eq!(ai.model(), "anthropic/claude-sonnet-5-5");
        *ai.model_mut() = "x/y".into();
        ai.provider = ProviderKind::Anthropic;
        assert_eq!(ai.model(), "claude-sonnet-5-5");
    }

    #[test]
    fn old_settings_without_ai_read_as_defaults() {
        let ai: AiSettings = serde_json::from_str(r#"{"provider": "OpenRouter"}"#).unwrap();
        assert_eq!(ai.provider, ProviderKind::OpenRouter);
        assert!(!ai.off && !ai.programs && ai.allowed.is_empty());
        assert_eq!(ai.anthropic_model, "claude-sonnet-5-5");
    }
}
