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

const CHAPTER_PREFIXES: [&str; 3] = ["chapter", "kapitel", "part"];

/// Whether `word` is a well-formed Roman numeral.
///
/// Membership in the letter set I V X L C D M is not enough: ordinary words
/// such as "Ill", "Civil" and "Vivid" are spelled entirely from it. A numeral
/// must also be in canonical form, so the word is parsed to a value and
/// re-encoded — only an exact round trip counts.
fn is_roman_numeral(word: &str) -> bool {
    const VALUES: [(u32, &str); 13] = [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"),
        (100, "C"), (90, "XC"), (50, "L"), (40, "XL"),
        (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];

    let upper = word.to_ascii_uppercase();
    let digit = |c: char| match c {
        'I' => Some(1),
        'V' => Some(5),
        'X' => Some(10),
        'L' => Some(50),
        'C' => Some(100),
        'D' => Some(500),
        'M' => Some(1000),
        _ => None,
    };

    let digits: Option<Vec<u32>> = upper.chars().map(digit).collect();
    let digits = match digits {
        Some(d) if !d.is_empty() => d,
        _ => return false,
    };

    let mut total = 0i64;
    for (i, &value) in digits.iter().enumerate() {
        match digits.get(i + 1) {
            Some(&next) if value < next => total -= i64::from(value),
            _ => total += i64::from(value),
        }
    }
    if total <= 0 {
        return false;
    }

    let mut canonical = String::new();
    let mut remaining = total as u32;
    for (value, symbol) in VALUES {
        while remaining >= value {
            canonical.push_str(symbol);
            remaining -= value;
        }
    }

    canonical == upper
}

/// Whether a chapter title carries any translatable prose. A title made up
/// only of numerals, Roman numerals, and/or a generic "chapter" word (e.g.
/// "I", "3.", "Chapter I", "Kapitel II", "Part 1") has nothing to translate —
/// sending it to the translator on its own, with no surrounding context,
/// produces nonsense such as the Roman numeral "I" coming back as the
/// Portuguese pronoun "Eu". Titles that merely contain a numeral alongside
/// real words, e.g. "I Remember Mama" or "3 Little Pigs", still return true.
fn should_translate_title(title: &str) -> bool {
    title.split_whitespace().any(|word| {
        let core = word.trim_matches(|c: char| !c.is_alphanumeric());
        if core.is_empty() {
            return false;
        }
        let is_arabic_numeral = core.chars().all(|c| c.is_ascii_digit());
        let is_chapter_prefix = CHAPTER_PREFIXES.iter().any(|p| p.eq_ignore_ascii_case(core));
        !(is_arabic_numeral || is_roman_numeral(core) || is_chapter_prefix)
    })
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
    // Called as each chunk finishes: (chunks_done, total_chunks). Chunks, not
    // chapters — a single chapter can be dozens of chunks, so chapter counts
    // barely move. Previously this fired in a loop after all work was done.
    on_chapter_done: &(dyn Fn(usize, usize) + Sync),
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
        on_chapter_done,
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
        } else if !should_translate_title(&raw_title) {
            raw_title.clone()
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

#[cfg(test)]
mod title_translation_tests {
    use super::*;

    #[test]
    fn skips_roman_numerals() {
        for title in ["I", "II", "III", "IV", "V", "IX", "X", "XIV", "xi"] {
            assert!(!should_translate_title(title), "expected {title:?} to be skipped");
        }
    }

    #[test]
    fn skips_arabic_numerals() {
        for title in ["1", "2", "42"] {
            assert!(!should_translate_title(title), "expected {title:?} to be skipped");
        }
    }

    #[test]
    fn skips_chapter_prefix_plus_numeral() {
        for title in ["Chapter I", "Chapter 3", "Kapitel II", "I.", "3.", "IV -", "Part 1"] {
            assert!(!should_translate_title(title), "expected {title:?} to be skipped");
        }
    }

    #[test]
    fn skips_blank_titles() {
        for title in ["", "   ", "\t\n"] {
            assert!(!should_translate_title(title), "expected {title:?} to be skipped");
        }
    }

    #[test]
    fn translates_prose_titles() {
        for title in [
            "Die Verwandlung",
            "The Trial",
            "I Remember Mama",
            "Chapter One: The Beginning",
            "3 Little Pigs",
        ] {
            assert!(should_translate_title(title), "expected {title:?} to be translated");
        }
    }

    #[test]
    fn translates_real_words_spelled_with_roman_numeral_letters() {
        for title in ["Ill", "Civil", "Mild", "Vivid", "Dim", "Mill"] {
            assert!(
                should_translate_title(title),
                "{} is a real word, not a Roman numeral",
                title
            );
        }
    }

    #[test]
    fn bare_roman_numeral_i_is_not_mistaken_for_the_pronoun() {
        // The exact bug: a standalone "I" chapter heading (Roman numeral "one")
        // was translated as if it were the English pronoun "I", coming back as
        // the Portuguese "Eu". It must be passed through untranslated instead.
        assert!(!should_translate_title("I"));
    }
}

#[cfg(test)]
mod progress_tests {
    #[test]
    fn callback_signature_compiles() {
        let mut calls: Vec<(usize, usize)> = vec![];
        let mut cb = |current: usize, total: usize| {
            calls.push((current, total));
        };
        cb(1, 5);
        cb(2, 5);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], (1, 5));
    }
}
