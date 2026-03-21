use clap::Parser;
use dotenvy::dotenv;
use gunnlod_core::{
    book::Book,
    chunker::{chunk_chapters, ChunkerConfig},
    epub_parser,
    epub_writer::{self, OutputChapter},
    translator::{requires_two_pass, translate_chunks, GroqTranslator, OllamaTranslator, Translator},
};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gunnlod", about = "Translate and simplify books to your language level")]
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

    println!("Parsing {}...", cli.input.display());
    let book: Book = epub_parser::parse_epub(&cli.input)?;
    let source_lang = cli
        .source_lang
        .or(book.metadata.source_language.clone())
        .unwrap_or_else(|| "auto".into());

    println!(
        "\"{}\" — {} words, {} chapters, lang: {}",
        book.metadata.title, book.metadata.word_count, book.metadata.chapter_count, source_lang
    );

    let selected: Vec<_> = if cli.chapters.is_empty() {
        book.chapters.iter().collect()
    } else {
        cli.chapters.iter().filter_map(|&n| book.chapters.get(n - 1)).collect()
    };
    if selected.is_empty() {
        anyhow::bail!("No chapters matched --chapters selection (book has {} chapters)", book.metadata.chapter_count);
    }
    println!("Translating {}/{} chapters", selected.len(), book.metadata.chapter_count);

    let config = ChunkerConfig { max_words_per_chunk: cli.max_chunk_words };
    let selected_owned: Vec<_> = selected.iter().map(|c| (*c).clone()).collect();
    let chunks = chunk_chapters(&selected_owned, &config);
    println!("{} chunks to translate", chunks.len());

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

    let two_pass = cli.two_pass || requires_two_pass(&cli.level);
    if two_pass {
        println!("Pipeline: two-pass (simplify → translate) for level {}", cli.level);
    }
    let results =
        translate_chunks(simplifier.as_ref(), translator.as_ref(), &chunks, &source_lang, &cli.target_lang, &cli.level, two_pass)
            .await?;

    // Group translated chunks back by original chapter, preserving chapter titles
    let mut chapter_map: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (chunk, text) in chunks.iter().zip(results.iter()) {
        chapter_map
            .entry(chunk.chapter_index)
            .or_default()
            .push(text.clone());
    }

    // Translate chapter titles to the target language
    let mut translated_titles: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    for &chapter_idx in chapter_map.keys() {
        let raw_title = book
            .chapters
            .get(chapter_idx)
            .and_then(|c| c.title.clone())
            .unwrap_or_else(|| format!("Chapter {}", chapter_idx + 1));
        let translated = translator
            .translate_chunk(&raw_title, &source_lang, &cli.target_lang, "")
            .await?;
        translated_titles.insert(chapter_idx, translated.trim().to_string());
    }

    let output_chapters: Vec<OutputChapter> = chapter_map
        .into_iter()
        .map(|(chapter_idx, texts)| {
            let title = translated_titles
                .remove(&chapter_idx)
                .unwrap_or_else(|| format!("Chapter {}", chapter_idx + 1));
            OutputChapter {
                title,
                content: texts.join("\n\n"),
            }
        })
        .collect();

    let title = format!(
        "{} ({} {})",
        book.metadata.title, cli.target_lang, cli.level
    );
    epub_writer::write_epub(&title, &cli.target_lang, &output_chapters, &cli.output)?;

    println!("Done: {}", cli.output.display());
    Ok(())
}
