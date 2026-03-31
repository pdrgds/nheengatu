use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub groq_api_key: Option<String>,
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

/// Resolve the Groq API key: flag > env > config file.
/// Returns None if not found anywhere.
pub fn resolve_api_key(flag_value: Option<&str>, config_path: &Path) -> Option<String> {
    // 1. CLI flag (skip empty strings from clap defaults)
    if let Some(key) = flag_value {
        if !key.is_empty() {
            return Some(key.to_string());
        }
    }

    // 2. Environment variable
    if let Ok(key) = std::env::var("GROQ_API_KEY") {
        if !key.is_empty() {
            return Some(key);
        }
    }

    // 3. Config file
    let cfg = load_config_from(config_path);
    cfg.groq_api_key
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
        assert!(cfg.groq_api_key.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nheengatu").join("config.toml");
        let cfg = Config { groq_api_key: Some("gsk_test123".to_string()) };
        save_config_to(&cfg, &path).unwrap();
        let loaded = load_config_from(&path);
        assert_eq!(loaded.groq_api_key.as_deref(), Some("gsk_test123"));
    }

    #[test]
    fn resolve_key_flag_wins() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nheengatu").join("config.toml");
        let cfg = Config { groq_api_key: Some("from_config".to_string()) };
        save_config_to(&cfg, &path).unwrap();

        let key = resolve_api_key(Some("from_flag"), &path);
        assert_eq!(key.as_deref(), Some("from_flag"));
    }

    #[test]
    fn resolve_key_falls_back_to_config() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nheengatu").join("config.toml");
        let cfg = Config { groq_api_key: Some("from_config".to_string()) };
        save_config_to(&cfg, &path).unwrap();

        let key = resolve_api_key(None, &path);
        assert_eq!(key.as_deref(), Some("from_config"));
    }
}
