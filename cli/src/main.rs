mod config;
mod interactive;
mod languages;
mod progress;

use clap::Parser;
use dotenvy::dotenv;
use nheengatu_core::{
    pipeline::{run_pipeline, PipelineConfig},
    translator::{GroqTranslator, OllamaTranslator, Translator},
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "nheengatu", about = "Translate and simplify books to your language level")]
struct Cli {
    #[arg(short, long)]
    input: Option<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short = 't', long)]
    target_lang: Option<String>,
    #[arg(short, long)]
    level: Option<String>,
    #[arg(long)]
    source_lang: Option<String>,
    /// Backend for pass 2 (translate). Also used for pass 1 if --simplify-backend is not set.
    #[arg(short, long)]
    backend: Option<String>,
    /// Backend for pass 1 (simplify). Defaults to --backend if not set.
    #[arg(long)]
    simplify_backend: Option<String>,
    #[arg(long, env = "GROQ_API_KEY")]
    groq_api_key: Option<String>,
    #[arg(long, default_value = "http://localhost:11434")]
    ollama_url: String,
    #[arg(short, long)]
    model: Option<String>,
    /// Model for pass 2 (translate). Defaults to same as --model.
    #[arg(long)]
    translate_model: Option<String>,
    #[arg(long, default_value = "2500")]
    max_chunk_words: usize,
    /// Only translate these chapters (1-based, comma-separated). E.g. --chapters 1,2,5
    #[arg(long, value_delimiter = ',')]
    chapters: Option<Vec<usize>>,
    /// Prompt style: simple (level name only) or detailed (level rules + examples) [default: detailed]
    #[arg(long, default_value = "detailed")]
    prompt: String,
    /// Force two-pass pipeline (simplify then translate) regardless of level
    #[arg(long)]
    two_pass: bool,
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .to_lowercase()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let cli = Cli::parse();

    // --- Resolve input file ---
    let input = match cli.input {
        Some(p) => p,
        None => interactive::ask_input_file()?,
    };

    // --- Resolve source language ---
    let source_lang = match cli.source_lang {
        Some(ref s) => {
            let lang = languages::resolve_language(s)
                .ok_or_else(|| anyhow::anyhow!("Unknown source language: '{}'. Use a supported language name or code.", s))?;
            lang.code.to_string()
        }
        None => {
            let lang = interactive::ask_source_language()?;
            lang.code.to_string()
        }
    };

    // --- Resolve target language ---
    let target_lang = match cli.target_lang {
        Some(ref s) => {
            let lang = languages::resolve_language(s)
                .ok_or_else(|| anyhow::anyhow!("Unknown target language: '{}'. Use a supported language name or code.", s))?;
            lang.code.to_string()
        }
        None => {
            let lang = interactive::ask_target_language(&source_lang)?;
            lang.code.to_string()
        }
    };

    // --- Resolve level ---
    let level = match cli.level {
        Some(l) => l.to_uppercase(),
        None => interactive::ask_level()?,
    };

    // --- Resolve chapters ---
    let chapters = match cli.chapters {
        Some(c) => c,
        None => interactive::ask_chapters()?,
    };

    // --- Standard vs Advanced ---
    let mut backend = cli.backend.clone().unwrap_or_else(|| "groq".to_string());
    let simplify_backend = cli.simplify_backend.clone();
    let mut model = cli.model.clone();
    let mut translate_model = cli.translate_model.clone();
    let mut force_two_pass = cli.two_pass;
    let mut max_chunk_words = cli.max_chunk_words;

    // Only show standard/advanced prompt if no advanced flags were explicitly set
    let has_advanced_flags = cli.backend.is_some()
        || cli.simplify_backend.is_some()
        || cli.model.is_some()
        || cli.translate_model.is_some()
        || cli.two_pass;

    if !has_advanced_flags {
        if interactive::ask_advanced()? {
            backend = interactive::ask_backend()?;
            model = interactive::ask_model("Model for simplify pass?", "llama-3.3-70b-versatile")?;
            translate_model = interactive::ask_model("Model for translate pass?", "same as above")?;
            force_two_pass = interactive::ask_two_pass()?;
            max_chunk_words = interactive::ask_max_chunk_words()?;
        }
    }

    let simplify_backend_str = simplify_backend.unwrap_or_else(|| backend.clone());

    // --- Resolve API key ---
    let config_path = config::config_path();
    let needs_groq = backend == "groq" || simplify_backend_str == "groq";
    let groq_api_key = if needs_groq {
        match config::resolve_api_key(cli.groq_api_key.as_deref(), &config_path) {
            Some(key) => key,
            None => interactive::ask_api_key()?,
        }
    } else {
        String::new()
    };

    // --- Build output path ---
    let simple_prompt = cli.prompt == "simple";

    let output = match cli.output {
        Some(p) => p,
        None => {
            // We'll generate the filename after parsing the EPUB to get the title.
            // Use a placeholder that we replace below.
            PathBuf::new()
        }
    };

    // --- Build translators ---
    let simplifier: Box<dyn Translator> = match simplify_backend_str.as_str() {
        "groq" => {
            let mut s = GroqTranslator::new(groq_api_key.clone())?;
            s.simple_prompt = simple_prompt;
            if let Some(m) = model.clone() {
                s = s.with_model(m);
            }
            Box::new(s)
        }
        "ollama" => {
            let mut s = OllamaTranslator::new(Some(cli.ollama_url.clone()), model.clone());
            s.simple_prompt = simple_prompt;
            Box::new(s)
        }
        other => anyhow::bail!("Unknown simplify backend: {}. Use 'groq' or 'ollama'.", other),
    };

    let translator: Box<dyn Translator> = match backend.as_str() {
        "groq" => {
            let mut t = GroqTranslator::new(groq_api_key)?;
            let m = translate_model.or(model);
            if let Some(m) = m {
                t = t.with_model(m);
            }
            Box::new(t)
        }
        "ollama" => {
            let m = translate_model.or(model);
            let t = OllamaTranslator::new(Some(cli.ollama_url), m);
            Box::new(t)
        }
        other => anyhow::bail!("Unknown backend: {}. Use 'groq' or 'ollama'.", other),
    };

    // --- Auto-generate output path if needed ---
    // We need the book title, so we parse the EPUB to generate the filename.
    // But run_pipeline also parses internally — that's fine, it's a fast local operation.
    let output = if output.as_os_str().is_empty() {
        let book = nheengatu_core::epub_parser::parse_epub(&input)?;
        let name = sanitize_filename(&book.metadata.title);
        PathBuf::from(format!("{}-{}-{}.epub", name, target_lang, level.to_lowercase()))
    } else {
        output
    };

    // --- Pipeline config ---
    let config = PipelineConfig {
        source_lang: Some(source_lang),
        target_lang,
        level,
        chapters,
        max_chunk_words,
        force_two_pass,
    };

    // --- Run with progress bar ---
    println!("Translating \"{}\"...", input.display());

    // We don't know total chapters yet when creating the bar, so we create it
    // lazily on first callback. Use a Mutex (Sync) to allow mutation from the closure.
    let pb: std::sync::Mutex<Option<indicatif::ProgressBar>> = std::sync::Mutex::new(None);

    run_pipeline(
        &input,
        &output,
        &config,
        simplifier.as_ref(),
        translator.as_ref(),
        &|current, total| {
            let mut pb_ref = pb.lock().unwrap();
            if pb_ref.is_none() {
                *pb_ref = Some(progress::create_progress_bar(total as u64));
            }
            if let Some(bar) = pb_ref.as_ref() {
                bar.set_position(current as u64);
                if current == total {
                    bar.finish_and_clear();
                }
            }
        },
    )
    .await?;

    println!("Done: {}", output.display());

    Ok(())
}
