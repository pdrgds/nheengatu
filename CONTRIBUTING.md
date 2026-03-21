# Contributing to Gunnlod

Thanks for your interest in contributing. This document covers what is in scope, how to get set up, and the expectations for pull requests.

## What is in scope

This repository contains two public crates:

- **`gunnlod-core`** — the library. Contributions welcome to:
  - `epub_parser` — EPUB parsing
  - `chunker` — text chunking logic
  - `translator` — the `Translator` trait and built-in backends (Groq, Ollama)
  - `pipeline` — the translation pipeline (`run_pipeline`, `PipelineConfig`)
- **`gunnlod-cli`** — the command-line tool built on top of `gunnlod-core`
- **New translator backends** — any LLM or translation API that implements the `Translator` trait

## What is out of scope

The hosted web service (`gunnlod-web`) lives in a separate private repository and is not open for external contributions.

## Local setup

Requires Rust (stable). Clone the repo and run:

```sh
cargo build
cargo test
```

There are no required environment variables to build and test the library or CLI. Tests that hit external APIs are integration tests and are not run by default.

## Adding a new translator backend

1. Add a struct in `core/src/translator.rs` (or a new module imported from there).
2. Implement the `Translator` trait:

```rust
#[async_trait]
impl Translator for MyTranslator {
    async fn translate_chunk(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
        level: &str,
    ) -> Result<String, TranslateError> {
        // call your API and return the translated text
    }
}
```

3. Optionally override `simplify_chunk` if your backend has a distinct rewrite-in-place capability (used in the two-pass A1/A2 pipeline).
4. Wire it up in the CLI (`cli/src/main.rs`) behind a flag if appropriate.
5. Add unit tests alongside your implementation.

## Pull request guidelines

- **Keep PRs small and focused.** One logical change per PR.
- **All tests must pass:** `cargo test` must be green before requesting review.
- **No breaking changes to the public API** (`Translator` trait, `PipelineConfig`) without opening an issue first to discuss the change.
- Commit messages should be short and descriptive (`feat:`, `fix:`, `refactor:` prefixes are welcome but not required).
- If you are fixing a bug, include a regression test where practical.
