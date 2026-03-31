use dialoguer::{Confirm, Input, Select};
use std::path::PathBuf;

use crate::config;
use crate::languages::{self, Language};

/// Prompt for an EPUB file path. Returns the path.
pub fn ask_input_file() -> anyhow::Result<PathBuf> {
    let input: String = Input::new()
        .with_prompt("What book do you want to translate?")
        .interact_text()?;
    let path = PathBuf::from(input.trim());
    anyhow::ensure!(path.exists(), "File not found: {}", path.display());
    anyhow::ensure!(
        path.extension().is_some_and(|e| e.eq_ignore_ascii_case("epub")),
        "File must be an .epub: {}",
        path.display()
    );
    Ok(path)
}

/// Prompt for source language from the fixed list.
pub fn ask_source_language() -> anyhow::Result<Language> {
    let langs = languages::all_languages();
    let items: Vec<&str> = langs.iter().map(|l| l.name).collect();
    let selection = Select::new()
        .with_prompt("What language is this book in?")
        .items(&items)
        .interact()?;
    Ok(langs[selection].clone())
}

/// Prompt for target language from the fixed list, excluding the source.
pub fn ask_target_language(source_code: &str) -> anyhow::Result<Language> {
    let langs = languages::all_languages_except(source_code);
    let items: Vec<&str> = langs.iter().map(|l| l.name).collect();
    let selection = Select::new()
        .with_prompt("What language should it be in?")
        .items(&items)
        .interact()?;
    Ok(langs[selection].clone())
}

/// CEFR level with display description.
struct Level {
    code: &'static str,
    label: &'static str,
}

const LEVELS: &[Level] = &[
    Level { code: "A1", label: "Beginner (A1)" },
    Level { code: "A2", label: "Elementary (A2)" },
    Level { code: "B1", label: "Intermediate (B1)" },
    Level { code: "B2", label: "Upper-intermediate (B2)" },
    Level { code: "C1", label: "Advanced (C1)" },
    Level { code: "C2", label: "Near-native (C2)" },
];

/// Prompt for CEFR level.
pub fn ask_level() -> anyhow::Result<String> {
    let items: Vec<&str> = LEVELS.iter().map(|l| l.label).collect();
    let selection = Select::new()
        .with_prompt("What's your level?")
        .items(&items)
        .interact()?;
    Ok(LEVELS[selection].code.to_string())
}

/// Prompt for chapter selection. Returns empty vec for "all".
pub fn ask_chapters() -> anyhow::Result<Vec<usize>> {
    let input: String = Input::new()
        .with_prompt("Which chapters? (enter for all, or e.g. 1,3,5)")
        .allow_empty(true)
        .interact_text()?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let chapters: Vec<usize> = trimmed
        .split(',')
        .map(|s| s.trim().parse::<usize>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| anyhow::anyhow!("Invalid chapter numbers: {}", trimmed))?;
    Ok(chapters)
}

/// Ask standard vs advanced, returns true if advanced.
pub fn ask_advanced() -> anyhow::Result<bool> {
    let items = &["Standard (recommended)", "Advanced (choose models, backends, chunk size)"];
    let selection = Select::new()
        .with_prompt("Standard or Advanced config?")
        .items(items)
        .default(0)
        .interact()?;
    Ok(selection == 1)
}

/// Advanced: ask for backend.
pub fn ask_backend() -> anyhow::Result<String> {
    let items = &["Groq (cloud)", "Ollama (local)"];
    let selection = Select::new()
        .with_prompt("Backend")
        .items(items)
        .default(0)
        .interact()?;
    Ok(if selection == 0 { "groq" } else { "ollama" }.to_string())
}

/// Advanced: ask for model name with a default.
pub fn ask_model(prompt: &str, default: &str) -> anyhow::Result<Option<String>> {
    let input: String = Input::new()
        .with_prompt(format!("{} (enter for default: {})", prompt, default))
        .allow_empty(true)
        .interact_text()?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

/// Advanced: ask for force two-pass.
pub fn ask_two_pass() -> anyhow::Result<bool> {
    Ok(Confirm::new()
        .with_prompt("Force two-pass pipeline?")
        .default(false)
        .interact()?)
}

/// Advanced: ask for max chunk words.
pub fn ask_max_chunk_words() -> anyhow::Result<usize> {
    let input: String = Input::new()
        .with_prompt("Max words per chunk? (enter for default: 2500)")
        .default("2500".to_string())
        .interact_text()?;
    Ok(input.trim().parse()?)
}

/// Prompt for Groq API key and optionally save it.
pub fn ask_api_key() -> anyhow::Result<String> {
    println!("No Groq API key found. Get a free one at https://console.groq.com");
    let key: String = Input::new()
        .with_prompt("Paste your key")
        .interact_text()?;
    let key = key.trim().to_string();
    anyhow::ensure!(!key.is_empty(), "API key cannot be empty");

    let save = Confirm::new()
        .with_prompt("Save to config so you don't need to enter it again?")
        .default(true)
        .interact()?;

    if save {
        let path = config::config_path();
        let cfg = config::Config {
            groq_api_key: Some(key.clone()),
        };
        config::save_config_to(&cfg, &path)?;
        println!("Saved to {}", path.display());
    }

    Ok(key)
}
