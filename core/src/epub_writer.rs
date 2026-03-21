use std::path::Path;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum EpubWriteError {
    #[error("build error: {0}")]
    BuildError(String),
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
}

pub struct OutputChapter {
    pub title: String,
    pub content: String,
}

pub fn write_epub(
    title: &str,
    language: &str,
    chapters: &[OutputChapter],
    output_path: &Path,
) -> Result<(), EpubWriteError> {
    use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

    let mut builder = EpubBuilder::new(
        ZipLibrary::new().map_err(|e| EpubWriteError::BuildError(e.to_string()))?,
    )
    .map_err(|e| EpubWriteError::BuildError(e.to_string()))?;

    builder
        .metadata("title", title)
        .map_err(|e| EpubWriteError::BuildError(e.to_string()))?;
    builder
        .metadata("lang", language)
        .map_err(|e| EpubWriteError::BuildError(e.to_string()))?;

    for (i, ch) in chapters.iter().enumerate() {
        let paras = ch
            .content
            .split("\n\n")
            .filter(|l| !l.trim().is_empty())
            .map(|l| format!("<p>{}</p>", l))
            .collect::<Vec<_>>()
            .join("\n");

        let xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
             <head><title>{}</title></head>\n\
             <body><h1>{}</h1>\n\
             {}\n\
             </body></html>",
            ch.title, ch.title, paras
        );

        builder
            .add_content(
                EpubContent::new(format!("ch_{i}.xhtml"), xhtml.as_bytes()).title(&ch.title),
            )
            .map_err(|e| EpubWriteError::BuildError(e.to_string()))?;
    }

    let mut f = std::fs::File::create(output_path)?;
    builder
        .generate(&mut f)
        .map_err(|e| EpubWriteError::BuildError(e.to_string()))?;
    use std::io::Write;
    f.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn writes_valid_epub() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("test.epub");
        write_epub(
            "Test",
            "de",
            &[OutputChapter {
                title: "Chapter One".into(),
                content: "Hello world.".into(),
            }],
            &out,
        )
        .unwrap();
        assert!(out.exists());
        let f = std::fs::File::open(&out).unwrap();
        assert!(zip::ZipArchive::new(f).is_ok());
    }

    #[test]
    fn multi_chapter_epub() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("multi.epub");
        write_epub(
            "Multi",
            "en",
            &[
                OutputChapter {
                    title: "One".into(),
                    content: "First chapter content.".into(),
                },
                OutputChapter {
                    title: "Two".into(),
                    content: "Second chapter content.".into(),
                },
            ],
            &out,
        )
        .unwrap();
        assert!(out.exists());
    }
}
