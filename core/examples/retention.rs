//! Runs a book through the two-pass pipeline with chunks in parallel and
//! records every intermediate text, so prompt changes can be measured on how
//! much of the book survives each pass.
//!
//! Usage: retention <epub> <target_lang> <level> <model> <endpoint_url> <api_key_env> <concurrency> <out.jsonl>

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nheengatu_core::anthropic::AnthropicTranslator;
use nheengatu_core::chunker::{chunk_chapters, ChunkerConfig};
use nheengatu_core::epub_parser::parse_epub;
use nheengatu_core::translator::{
    requires_two_pass, retry_delay_secs, OpenRouterTranslator, TranslateError, Translator,
};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;

async fn with_retries<F, Fut>(label: &str, mut call: F) -> Result<String, TranslateError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<String, TranslateError>>,
{
    let mut last_err = None;
    for attempt in 0..5u32 {
        match call().await {
            Ok(text) => return Ok(text),
            Err(e) => {
                let wait = retry_delay_secs(&e, attempt);
                eprintln!("{label}: attempt {attempt} failed ({e}), retrying in {wait}s");
                last_err = Some(e);
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
        }
    }
    Err(last_err.expect("loop ran at least once"))
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, epub, target, level, model, url, key_env, concurrency, out] = &args[..] else {
        eprintln!("usage: retention <epub> <target_lang> <level> <model> <endpoint_url> <api_key_env> <concurrency> <out.jsonl>");
        std::process::exit(2);
    };

    let api_key = std::env::var(key_env).unwrap_or_else(|_| panic!("{key_env} is not set"));
    let anthropic = url.contains("api.anthropic.com").then(|| {
        Arc::new(
            AnthropicTranslator::new(api_key.clone())
                .expect("api key")
                .with_model(model.clone())
                .with_base_url(url.clone()),
        )
    });
    let translator: Arc<dyn Translator> = match &anthropic {
        Some(t) => t.clone(),
        None => Arc::new(
            OpenRouterTranslator::new(api_key)
                .expect("api key")
                .with_base_url(url.clone())
                .with_model(model.clone()),
        ),
    };

    let book = parse_epub(&PathBuf::from(epub)).expect("parse epub");
    let source = book.metadata.source_language.clone().unwrap_or_else(|| "auto".into());
    let chunks = chunk_chapters(&book.chapters, &ChunkerConfig::default());
    let two_pass = requires_two_pass(level);
    eprintln!(
        "\"{}\": {} words, {} chunks, source {source}, two-pass {two_pass}",
        book.metadata.title,
        book.metadata.word_count,
        chunks.len()
    );

    let file = tokio::fs::File::create(out).await.expect("create output");
    let file = Arc::new(Mutex::new(file));
    let permits = Arc::new(Semaphore::new(concurrency.parse().expect("concurrency")));
    let total = chunks.len();
    let mut tasks = JoinSet::new();

    for chunk in chunks {
        let (translator, permits, file) = (translator.clone(), permits.clone(), file.clone());
        let (source, target, level) = (source.clone(), target.clone(), level.clone());
        tasks.spawn(async move {
            let _permit = permits.acquire_owned().await.expect("semaphore");
            let label = format!("ch{} chunk{}", chunk.chapter_index + 1, chunk.chunk_index + 1);
            let started = Instant::now();
            let t = translator.as_ref();

            let result = async {
                if two_pass {
                    let simplified = with_retries(&label, || t.simplify_chunk(&chunk.content, &source, &level)).await?;
                    let translated = with_retries(&label, || t.translate_chunk(&simplified, &source, &target, "")).await?;
                    Ok::<_, TranslateError>((Some(simplified), translated))
                } else {
                    let translated = with_retries(&label, || t.translate_chunk(&chunk.content, &source, &target, &level)).await?;
                    Ok((None, translated))
                }
            }
            .await;

            let record = match result {
                Ok((simplified, translated)) => serde_json::json!({
                    "chapter": chunk.chapter_index,
                    "chunk": chunk.chunk_index,
                    "source": chunk.content,
                    "simplified": simplified,
                    "translated": translated,
                    "secs": started.elapsed().as_secs_f64(),
                }),
                Err(e) => serde_json::json!({
                    "chapter": chunk.chapter_index,
                    "chunk": chunk.chunk_index,
                    "source": chunk.content,
                    "error": e.to_string(),
                }),
            };
            let mut line = record.to_string();
            line.push('\n');
            file.lock().await.write_all(line.as_bytes()).await.expect("write record");
            eprintln!("{label} done in {:.0}s", started.elapsed().as_secs_f64());
        });
    }

    let mut done = 0;
    while tasks.join_next().await.is_some() {
        done += 1;
        eprintln!("[{done}/{total}]");
    }
    file.lock().await.flush().await.expect("flush");

    if let Some(t) = anthropic {
        let usage = t.usage();
        let summary = serde_json::json!({
            "model": model,
            "input_tokens": usage.input_tokens,
            "output_tokens": usage.output_tokens,
            "fallback_responses": usage.fallback_responses,
        });
        eprintln!("usage: {summary}");
        tokio::fs::write(format!("{out}.usage.json"), summary.to_string())
            .await
            .expect("write usage");
    }
}
