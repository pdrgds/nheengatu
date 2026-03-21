use clap::Parser;
use gunnlod_core::{
    book::Book,
    chunker::{chunk_chapters, ChunkerConfig},
    epub_parser,
    epub_writer::{self, OutputChapter},
    translator::{translate_chunks, GroqTranslator, OllamaTranslator, Translator},
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
    #[arg(short, long, default_value = "groq")]
    backend: String,
    #[arg(long, env = "GROQ_API_KEY", default_value = "")]
    groq_api_key: String,
    #[arg(long, default_value = "http://localhost:11434")]
    ollama_url: String,
    #[arg(short, long)]
    model: Option<String>,
    #[arg(long, default_value = "2500")]
    max_chunk_words: usize,
    /// Only translate these chapters (1-based, comma-separated). E.g. --chapters 1,2,5
    #[arg(long, value_delimiter = ',')]
    chapters: Vec<usize>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    let translator: Box<dyn Translator> = match cli.backend.as_str() {
        "groq" => {
            let mut t = GroqTranslator::new(cli.groq_api_key)?;
            if let Some(m) = cli.model {
                t = t.with_model(m);
            }
            println!("Translating via Groq ({})...", t.model());
            Box::new(t)
        }
        "ollama" => {
            let t = OllamaTranslator::new(Some(cli.ollama_url), cli.model);
            println!("Translating via Ollama ({})...", t.model());
            Box::new(t)
        }
        other => anyhow::bail!("Unknown backend: {}. Use 'groq' or 'ollama'.", other),
    };

    let results =
        translate_chunks(translator.as_ref(), &chunks, &source_lang, &cli.target_lang, &cli.level)
            .await?;

    // Group translated chunks back by original chapter, preserving chapter titles
    let mut chapter_map: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (chunk, text) in chunks.iter().zip(results.iter()) {
        chapter_map
            .entry(chunk.chapter_index)
            .or_default()
            .push(text.clone());
    }

    let output_chapters: Vec<OutputChapter> = chapter_map
        .into_iter()
        .map(|(chapter_idx, texts)| {
            let title = book
                .chapters
                .get(chapter_idx)
                .and_then(|c| c.title.clone())
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
