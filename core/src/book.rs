use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookMetadata {
    pub title: String,
    pub word_count: usize,
    pub chapter_count: usize,
    pub source_language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Chapter {
    pub index: usize,
    pub title: Option<String>,
    pub content: String,
}

impl Chapter {
    pub fn word_count(&self) -> usize {
        self.content.split_whitespace().count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub chapter_index: usize,
    pub chunk_index: usize,
    pub content: String,
}

#[derive(Debug)]
pub struct Book {
    pub metadata: BookMetadata,
    pub chapters: Vec<Chapter>,
}

impl Book {
    pub fn new(title: String, chapters: Vec<Chapter>, source_language: Option<String>) -> Self {
        let word_count: usize = chapters.iter().map(|c| c.word_count()).sum();
        let chapter_count = chapters.len();
        Self {
            metadata: BookMetadata { title, word_count, chapter_count, source_language },
            chapters,
        }
    }

    /// Preview: first chapter truncated to min(10% of total words, 5000 words).
    /// Returns owned chapters so content can be truncated within a chapter.
    pub fn preview_chapters(&self) -> Vec<Chapter> {
        if self.chapters.is_empty() {
            return vec![];
        }
        let budget = (self.metadata.word_count / 10).min(5000).max(500);
        let first = &self.chapters[0];

        if first.word_count() <= budget {
            return vec![first.clone()];
        }

        // Truncate at last sentence boundary within budget
        let words: Vec<&str> = first.content.split_whitespace().collect();
        let truncated_raw = words[..budget.min(words.len())].join(" ");
        let truncated = truncated_raw
            .rfind(|c: char| c == '.' || c == '!' || c == '?')
            .map(|pos| truncated_raw[..=pos].to_string())
            .unwrap_or(truncated_raw);

        vec![Chapter { index: first.index, title: first.title.clone(), content: truncated }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_word_count() {
        let ch = Chapter { index: 0, title: None, content: "one two three".into() };
        assert_eq!(ch.word_count(), 3);
    }

    #[test]
    fn book_metadata_computed() {
        let chapters = vec![
            Chapter { index: 0, title: None, content: "hello world".into() },
            Chapter { index: 1, title: None, content: "foo bar baz".into() },
        ];
        let book = Book::new("Test".into(), chapters, Some("en".into()));
        assert_eq!(book.metadata.word_count, 5);
        assert_eq!(book.metadata.chapter_count, 2);
    }

    #[test]
    fn preview_short_first_chapter() {
        // Short first chapter: returned in full
        let chapters = vec![
            Chapter { index: 0, title: None, content: "word ".repeat(100) },
            Chapter { index: 1, title: None, content: "word ".repeat(900) },
        ];
        let book = Book::new("Test".into(), chapters, None);
        // budget = min(1000/10, 5000) = 100 words; first chapter is exactly 100 words
        assert_eq!(book.preview_chapters().len(), 1);
    }

    #[test]
    fn preview_truncates_long_first_chapter() {
        // 20,000-word first chapter should be truncated, not given in full
        let long_chapter = "word ".repeat(20_000);
        let chapters = vec![
            Chapter { index: 0, title: None, content: long_chapter },
        ];
        let book = Book::new("Test".into(), chapters, None);
        let preview = book.preview_chapters();
        assert_eq!(preview.len(), 1);
        assert!(preview[0].word_count() <= 5000);
    }
}
