use std::collections::BTreeMap;
use std::path::Path;

use thiserror::Error;

use crate::chunker::{chunk_chapters, ChunkerConfig};
use crate::epub_parser;
use crate::epub_writer::{self, OutputChapter};
use crate::translator::{requires_two_pass, translate_chunks, TranslateError, Translator};

#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("epub parse error: {0}")]
    Parse(#[from] crate::epub_parser::EpubParseError),
    #[error("translation error: {0}")]
    Translate(#[from] TranslateError),
    #[error("epub write error: {0}")]
    Write(#[from] crate::epub_writer::EpubWriteError),
    #[error("{0}")]
    Other(String),
}

pub struct PipelineConfig {
    /// Source language code (e.g. "de"). Falls back to EPUB metadata, then "auto".
    pub source_lang: Option<String>,
    /// Target language code (e.g. "pt").
    pub target_lang: String,
    /// CEFR level (e.g. "A2", "B1").
    pub level: String,
    /// Chapters to translate (1-based). Empty = all chapters.
    pub chapters: Vec<usize>,
    /// Max words per translation chunk.
    pub max_chunk_words: usize,
    /// Force two-pass pipeline regardless of level. If false, uses level-based detection.
    pub force_two_pass: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            source_lang: None,
            target_lang: "en".into(),
            level: "B1".into(),
            chapters: vec![],
            max_chunk_words: 2500,
            force_two_pass: false,
        }
    }
}

/// Run the full translation pipeline: parse → chunk → translate → assemble → write EPUB.
///
/// `simplifier` is used for Pass 1 (simplify in source language).
/// `translator` is used for Pass 2 (translate to target language), and for single-pass.
/// When source == target, translation pass is skipped automatically.
pub async fn run_pipeline(
    input: &Path,
    output: &Path,
    config: &PipelineConfig,
    simplifier: &(dyn Translator + Send + Sync),
    translator: &(dyn Translator + Send + Sync),
) -> Result<(), PipelineError> {
    let book = epub_parser::parse_epub(input)?;

    let source_lang = config
        .source_lang
        .clone()
        .or(book.metadata.source_language.clone())
        .unwrap_or_else(|| "auto".into());

    println!(
        "\"{}\" — {} words, {} chapters, lang: {}",
        book.metadata.title, book.metadata.word_count, book.metadata.chapter_count, source_lang
    );

    let selected: Vec<_> = if config.chapters.is_empty() {
        book.chapters.iter().collect()
    } else {
        config
            .chapters
            .iter()
            .filter_map(|&n| book.chapters.get(n - 1))
            .collect()
    };

    if selected.is_empty() {
        return Err(PipelineError::Other(format!(
            "no chapters matched selection (book has {} chapters)",
            book.metadata.chapter_count
        )));
    }

    println!("Translating {}/{} chapters", selected.len(), book.metadata.chapter_count);

    let chunker_config = ChunkerConfig { max_words_per_chunk: config.max_chunk_words };
    let selected_owned: Vec<_> = selected.iter().map(|c| (*c).clone()).collect();
    let chunks = chunk_chapters(&selected_owned, &chunker_config);
    println!("{} chunks to translate", chunks.len());

    let two_pass = config.force_two_pass || requires_two_pass(&config.level);
    if two_pass {
        println!("Pipeline: two-pass (simplify → translate) for level {}", config.level);
    }

    let results = translate_chunks(
        simplifier,
        translator,
        &chunks,
        &source_lang,
        &config.target_lang,
        &config.level,
        two_pass,
    )
    .await?;

    // Group translated chunks by chapter
    let mut chapter_map: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (chunk, text) in chunks.iter().zip(results.iter()) {
        chapter_map
            .entry(chunk.chapter_index)
            .or_default()
            .push(text.clone());
    }

    // Translate chapter titles to target language (skip if title is empty)
    let mut translated_titles: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    for &chapter_idx in chapter_map.keys() {
        let raw_title = book
            .chapters
            .get(chapter_idx)
            .and_then(|c| c.title.clone())
            .unwrap_or_default();
        let translated = if raw_title.is_empty() {
            String::new()
        } else {
            translator
                .translate_chunk(&raw_title, &source_lang, &config.target_lang, "")
                .await?
                .trim()
                .to_string()
        };
        translated_titles.insert(chapter_idx, translated);
    }

    let output_chapters: Vec<OutputChapter> = chapter_map
        .into_iter()
        .map(|(chapter_idx, texts)| {
            let title = translated_titles.remove(&chapter_idx).unwrap_or_default();
            OutputChapter {
                title,
                content: texts.join("\n\n"),
            }
        })
        .collect();

    let epub_title = format!(
        "{} ({} {})",
        book.metadata.title, config.target_lang, config.level
    );
    epub_writer::write_epub(&epub_title, &config.target_lang, &output_chapters, output)?;

    Ok(())
}
