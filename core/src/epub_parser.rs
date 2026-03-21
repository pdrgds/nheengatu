use crate::book::{Book, Chapter};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EpubParseError {
    #[error("failed to open epub: {0}")]
    OpenError(String),
    #[error("no chapters found")]
    NoChapters,
}

/// Replace block-level closing tags with newlines BEFORE stripping,
/// so paragraph structure is preserved in the output text.
fn strip_html(html: &str) -> String {
    let html = html
        .replace("</p>", "\n\n")
        .replace("</div>", "\n\n")
        .replace("</h1>", "\n\n")
        .replace("</h2>", "\n\n")
        .replace("</h3>", "\n\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");

    // Strip all remaining tags
    let mut result = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                result.push(' ');
            }
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    // Normalize: trim each paragraph, remove empty ones, rejoin with \n\n
    result
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Strip HTML tags without any punctuation normalization — used for title extraction.
fn strip_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                result.push(' ');
            }
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract text content from the first <h1> or <h2> tag.
fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    for tag in ["h1", "h2"] {
        let open_pattern = format!("<{}", tag);
        let close_tag_lower = format!("</{}>", tag);
        let close_tag_upper = format!("</{}>", tag.to_uppercase());
        // open_pattern and '>' are ASCII — byte positions found in `lower` are
        // identical to byte positions in `html` up to content_start.
        if let Some(open_pos) = lower.find(&open_pattern) {
            if let Some(gt_offset) = lower[open_pos..].find('>') {
                let content_start = open_pos + gt_offset + 1;
                // Search `html` directly for the close tag so that byte offsets
                // remain valid even when heading content contains non-ASCII chars
                // whose byte length changes under to_lowercase() (e.g. 'İ' → "i\u{307}").
                let after_open = &html[content_start..];
                let close_offset = after_open
                    .find(&close_tag_lower)
                    .or_else(|| after_open.find(&close_tag_upper));
                if let Some(close_offset) = close_offset {
                    let raw = &html[content_start..content_start + close_offset];
                    let title = strip_tags(raw).trim().to_string();
                    if !title.is_empty() {
                        return Some(title);
                    }
                }
            }
        }
    }
    None
}

pub fn parse_epub(path: &Path) -> Result<Book, EpubParseError> {
    let mut doc = epub::doc::EpubDoc::new(path)
        .map_err(|e| EpubParseError::OpenError(e.to_string()))?;

    let title = doc.get_title().unwrap_or_else(|| "Untitled".to_string());
    let language = doc.mdata("language").map(|m| m.value.clone());

    // Collect spine item idrefs first to avoid borrow conflicts
    let spine_ids: Vec<String> = doc.spine.iter().map(|s| s.idref.clone()).collect();

    let mut chapters = Vec::new();
    for (index, idref) in spine_ids.iter().enumerate() {
        if let Some((html, _mime)) = doc.get_resource_str(idref) {
            let chapter_title = extract_title(&html);
            let text = strip_html(&html);
            if !text.trim().is_empty() {
                chapters.push(Chapter {
                    index,
                    title: chapter_title,
                    content: text,
                });
            }
        }
    }

    if chapters.is_empty() {
        return Err(EpubParseError::NoChapters);
    }

    Ok(Book::new(title, chapters, language))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/test.epub")
    }

    #[test]
    fn parse_test_epub() {
        let book = parse_epub(&fixture()).unwrap();
        assert_eq!(book.metadata.title, "Test Book");
        assert_eq!(book.metadata.chapter_count, 2);
        assert!(book.metadata.word_count > 0);
        assert_eq!(book.metadata.source_language, Some("en".to_string()));
    }

    #[test]
    fn chapters_have_content() {
        let book = parse_epub(&fixture()).unwrap();
        assert!(book.chapters[0].content.contains("first paragraph"));
        assert!(book.chapters[1].content.contains("Second chapter"));
    }

    #[test]
    fn chapters_have_titles() {
        let book = parse_epub(&fixture()).unwrap();
        assert_eq!(book.chapters[0].title.as_deref(), Some("Chapter One"));
        assert_eq!(book.chapters[1].title.as_deref(), Some("Chapter Two"));
    }

    #[test]
    fn strip_html_preserves_paragraph_breaks() {
        let html = "<p>First para.</p><p>Second para.</p>";
        let text = strip_html(html);
        assert!(text.contains("\n\n"), "paragraph breaks must be preserved");
    }

    #[test]
    fn strip_html_basic() {
        assert_eq!(strip_html("<p>Hello <b>world</b></p>").trim(), "Hello world");
    }

    #[test]
    fn parse_nonexistent_file_returns_error() {
        use std::path::Path;
        let result = parse_epub(Path::new("/nonexistent/path/book.epub"));
        assert!(matches!(result, Err(EpubParseError::OpenError(_))));
    }

    #[test]
    fn strip_html_only_whitespace_returns_empty() {
        let result = strip_html("<p>   </p><div></div>");
        assert!(result.trim().is_empty());
    }

    #[test]
    fn extract_title_no_heading_returns_none() {
        let html = "<html><body><p>Some content without headings.</p></body></html>";
        assert!(extract_title(html).is_none());
    }
}
