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
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short = 't', long)]
    target_lang: String,
    #[arg(short, long)]
    level: String,
    #[arg(long)]
    source_lang: Option<String>,
    /// Backend for pass 2 (translate). Also used for pass 1 if --simplify-backend is not set.
    #[arg(short, long, default_value = "groq")]
    backend: String,
    /// Backend for pass 1 (simplify). Defaults to --backend if not set.
    #[arg(long)]
    simplify_backend: Option<String>,
    #[arg(long, env = "GROQ_API_KEY", default_value = "")]
    groq_api_key: String,
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
    chapters: Vec<usize>,
    /// Prompt style: simple (level name only) or detailed (level rules + examples) [default: detailed]
    #[arg(long, default_value = "detailed")]
    prompt: String,
    /// Force two-pass pipeline (simplify then translate) regardless of level
    #[arg(long)]
    two_pass: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let cli = Cli::parse();

    let simple_prompt = cli.prompt == "simple";
    let simplify_backend = cli.simplify_backend.as_deref().unwrap_or(&cli.backend).to_string();

    let simplifier: Box<dyn Translator> = match simplify_backend.as_str() {
        "groq" => {
            let mut s = GroqTranslator::new(cli.groq_api_key.clone())?;
            s.simple_prompt = simple_prompt;
            if let Some(m) = cli.model.clone() { s = s.with_model(m); }
            println!("Pass 1 (simplify) via Groq ({})...", s.model());
            Box::new(s)
        }
        "ollama" => {
            let mut s = OllamaTranslator::new(Some(cli.ollama_url.clone()), cli.model.clone());
            s.simple_prompt = simple_prompt;
            println!("Pass 1 (simplify) via Ollama ({})...", s.model());
            Box::new(s)
        }
        other => anyhow::bail!("Unknown simplify backend: {}. Use 'groq' or 'ollama'.", other),
    };

    let translator: Box<dyn Translator> = match cli.backend.as_str() {
        "groq" => {
            let mut t = GroqTranslator::new(cli.groq_api_key)?;
            let m = cli.translate_model.or(cli.model);
            if let Some(m) = m { t = t.with_model(m); }
            println!("Pass 2 (translate) via Groq ({})...", t.model());
            Box::new(t)
        }
        "ollama" => {
            let m = cli.translate_model.or(cli.model);
            let t = OllamaTranslator::new(Some(cli.ollama_url), m);
            println!("Pass 2 (translate) via Ollama ({})...", t.model());
            Box::new(t)
        }
        other => anyhow::bail!("Unknown backend: {}. Use 'groq' or 'ollama'.", other),
    };

    let config = PipelineConfig {
        source_lang: cli.source_lang,
        target_lang: cli.target_lang,
        level: cli.level,
        chapters: cli.chapters,
        max_chunk_words: cli.max_chunk_words,
        force_two_pass: cli.two_pass,
    };

    println!("Parsing {}...", cli.input.display());
    run_pipeline(&cli.input, &cli.output, &config, simplifier.as_ref(), translator.as_ref(), &|_, _| {}).await?;
    println!("Done: {}", cli.output.display());

    Ok(())
}
