use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub anthropic_api_key: Option<String>,
    pub openrouter_api_key: Option<String>,
}

/// A cloud provider whose API key the CLI resolves and can save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenRouter,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::OpenRouter => "OpenRouter",
        }
    }

    pub fn env_var(self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
        }
    }

    pub fn keys_url(self) -> &'static str {
        match self {
            Provider::Anthropic => "https://platform.claude.com/settings/keys",
            Provider::OpenRouter => "https://openrouter.ai/keys",
        }
    }
}

impl Config {
    pub fn api_key(&self, provider: Provider) -> Option<&String> {
        match provider {
            Provider::Anthropic => self.anthropic_api_key.as_ref(),
            Provider::OpenRouter => self.openrouter_api_key.as_ref(),
        }
    }

    pub fn set_api_key(&mut self, provider: Provider, key: String) {
        match provider {
            Provider::Anthropic => self.anthropic_api_key = Some(key),
            Provider::OpenRouter => self.openrouter_api_key = Some(key),
        }
    }
}

/// Returns the default config file path for this platform.
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nheengatu")
        .join("config.toml")
}

pub fn load_config_from(path: &Path) -> Config {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config_to(config: &Config, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Resolve a provider's API key: flag > env > config file.
/// Returns None if not found anywhere.
pub fn resolve_api_key(flag_value: Option<&str>, provider: Provider, config_path: &Path) -> Option<String> {
    // 1. CLI flag (skip empty strings from clap defaults)
    if let Some(key) = flag_value {
        if !key.is_empty() {
            return Some(key.to_string());
        }
    }

    // 2. Environment variable
    if let Ok(key) = std::env::var(provider.env_var()) {
        if !key.is_empty() {
            return Some(key);
        }
    }

    // 3. Config file
    let cfg = load_config_from(config_path);
    cfg.api_key(provider).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_nonexistent_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nheengatu").join("config.toml");
        let cfg = load_config_from(&path);
        assert!(cfg.openrouter_api_key.is_none());
        assert!(cfg.anthropic_api_key.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nheengatu").join("config.toml");
        let cfg = Config { openrouter_api_key: Some("sk-or-test123".to_string()), ..Default::default() };
        save_config_to(&cfg, &path).unwrap();
        let loaded = load_config_from(&path);
        assert_eq!(loaded.openrouter_api_key.as_deref(), Some("sk-or-test123"));
    }

    #[test]
    fn resolve_key_flag_wins() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nheengatu").join("config.toml");
        let cfg = Config { openrouter_api_key: Some("from_config".to_string()), ..Default::default() };
        save_config_to(&cfg, &path).unwrap();

        let key = resolve_api_key(Some("from_flag"), Provider::OpenRouter, &path);
        assert_eq!(key.as_deref(), Some("from_flag"));
    }

    #[test]
    fn resolve_key_falls_back_to_config() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nheengatu").join("config.toml");
        let cfg = Config { openrouter_api_key: Some("from_config".to_string()), ..Default::default() };
        save_config_to(&cfg, &path).unwrap();

        let key = resolve_api_key(None, Provider::OpenRouter, &path);
        assert_eq!(key.as_deref(), Some("from_config"));
    }

    #[test]
    fn each_provider_reads_its_own_saved_key() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nheengatu").join("config.toml");
        let mut cfg = Config::default();
        cfg.set_api_key(Provider::Anthropic, "sk-ant-test".to_string());
        cfg.set_api_key(Provider::OpenRouter, "sk-or-test".to_string());
        save_config_to(&cfg, &path).unwrap();

        let loaded = load_config_from(&path);
        assert_eq!(loaded.api_key(Provider::Anthropic).map(String::as_str), Some("sk-ant-test"));
        assert_eq!(loaded.api_key(Provider::OpenRouter).map(String::as_str), Some("sk-or-test"));
    }

    #[test]
    fn config_saved_before_anthropic_still_loads() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "openrouter_api_key = \"sk-or-old\"\n").unwrap();
        let loaded = load_config_from(&path);
        assert_eq!(loaded.openrouter_api_key.as_deref(), Some("sk-or-old"));
        assert!(loaded.anthropic_api_key.is_none());
    }
}
