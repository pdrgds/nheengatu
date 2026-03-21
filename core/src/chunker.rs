use crate::book::{Chapter, Chunk};

const DEFAULT_MAX_WORDS: usize = 2500;

pub struct ChunkerConfig {
    pub max_words_per_chunk: usize,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            max_words_per_chunk: DEFAULT_MAX_WORDS,
        }
    }
}

/// Split an oversized paragraph by word count (hard split, no sentence boundary).
/// Used when a single paragraph exceeds max_words_per_chunk.
fn split_oversized(text: &str, max_words: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= max_words {
        return vec![text.to_string()];
    }
    words.chunks(max_words).map(|w| w.join(" ")).collect()
}

/// Splits a chapter into chunks of at most `config.max_words_per_chunk` words.
///
/// The `chunk_index` in each returned [`Chunk`] is 0-based within this chapter.
/// If you need a global ordering across chapters, combine `chapter_index` and
/// `chunk_index` as a composite key.
pub fn chunk_chapter(chapter: &Chapter, config: &ChunkerConfig) -> Vec<Chunk> {
    assert!(
        config.max_words_per_chunk > 0,
        "max_words_per_chunk must be greater than 0"
    );
    // Split into sub-paragraphs; handle oversized single paragraphs
    let all_paragraphs: Vec<String> = chapter
        .content
        .split("\n\n")
        .flat_map(|p| split_oversized(p, config.max_words_per_chunk))
        .collect();

    if all_paragraphs.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut words = 0;
    let mut idx = 0;

    for para in &all_paragraphs {
        let para_words = para.split_whitespace().count();
        if words + para_words > config.max_words_per_chunk && !current.is_empty() {
            chunks.push(Chunk {
                chapter_index: chapter.index,
                chunk_index: idx,
                content: current.trim().to_string(),
            });
            idx += 1;
            current.clear();
            words = 0;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
        words += para_words;
    }
    if !current.trim().is_empty() {
        chunks.push(Chunk {
            chapter_index: chapter.index,
            chunk_index: idx,
            content: current.trim().to_string(),
        });
    }
    chunks
}

pub fn chunk_chapters(chapters: &[Chapter], config: &ChunkerConfig) -> Vec<Chunk> {
    chapters
        .iter()
        .flat_map(|ch| chunk_chapter(ch, config))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::Chapter;

    #[test]
    fn small_chapter_single_chunk() {
        let ch = Chapter {
            index: 0,
            title: None,
            content: "Short text.".into(),
        };
        let chunks = chunk_chapter(&ch, &ChunkerConfig::default());
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn large_chapter_splits_at_paragraphs() {
        // 20 paragraphs of 200 words each = 4000 words; should split into >=2 chunks
        let para = "word ".repeat(200);
        let content = (0..20)
            .map(|_| para.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let ch = Chapter {
            index: 0,
            title: None,
            content,
        };
        let chunks = chunk_chapter(&ch, &ChunkerConfig { max_words_per_chunk: 2500 });
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert_eq!(chunk.chapter_index, 0);
        }
    }

    #[test]
    fn oversized_single_paragraph_is_split() {
        // One paragraph of 5000 words — no \n\n — must still be chunked
        let content = "word ".repeat(5000);
        let ch = Chapter {
            index: 0,
            title: None,
            content,
        };
        let chunks = chunk_chapter(&ch, &ChunkerConfig { max_words_per_chunk: 2500 });
        assert!(
            chunks.len() >= 2,
            "oversized paragraph must be split by word count"
        );
    }

    #[test]
    fn oversized_exact_boundary_is_not_split() {
        // Exactly max_words words — should return single chunk, not two
        let content = "word ".repeat(2500).trim().to_string();
        let ch = Chapter {
            index: 0,
            title: None,
            content,
        };
        let chunks = chunk_chapter(&ch, &ChunkerConfig::default());
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn chunk_chapters_preserves_chapter_indices() {
        let config = ChunkerConfig::default();
        let chapters = vec![
            Chapter {
                index: 0,
                title: None,
                content: "Short chapter one.".into(),
            },
            Chapter {
                index: 1,
                title: None,
                content: "Short chapter two.".into(),
            },
        ];
        let chunks = chunk_chapters(&chapters, &config);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chapter_index, 0);
        assert_eq!(chunks[1].chapter_index, 1);
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[1].chunk_index, 0);
    }

    #[test]
    fn chunk_indices_are_sequential() {
        let para = "word ".repeat(300);
        let content = (0..15)
            .map(|_| para.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let ch = Chapter {
            index: 2,
            title: None,
            content,
        };
        let chunks = chunk_chapter(&ch, &ChunkerConfig { max_words_per_chunk: 2500 });
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i);
        }
    }
}
