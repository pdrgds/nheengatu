# nheengatu

In Tupi mythology, Nheengatu — "the good language" — was the lingua geral that bridged dozens of peoples across the Amazon basin, turning mutual incomprehension into shared understanding.

Translate and simplify EPUB books to a target CEFR language level using an LLM.

Give it a German novel and ask for Portuguese at A2 — it parses the EPUB, chunks the text, instructs the model to rewrite each chunk at the right level, and produces a new EPUB ready for your Kindle.

---

**Want the easy way?** A hosted version is available at [nheengatu.com](https://nheengatu.com) — upload, pay, download. No Rust, no API keys, no setup.

**Want to run it yourself?** Read on.

---

## How it works

1. Parses the EPUB and splits chapters into chunks (~2500 words each)
2. Sends each chunk to an LLM with CEFR-level instructions
3. Reassembles the translated/simplified chunks into a new EPUB

For **A1 and A2**, a two-pass pipeline runs automatically:
- **Pass 1 — Simplify** in the source language (vocabulary and structure only)
- **Pass 2 — Translate** the simplified text to the target language

Separating these steps produces significantly better results at low levels — in testing, two-pass A2 output showed 15–25% higher coverage of standard vocabulary lists compared to single-pass. It also prevents the language leakage that occurs when a model tries to simplify and translate simultaneously.

For **B1 and above**, a single pass is used. Forcing two passes at B1 hurts quality: Pass 1 over-simplifies sentence structure and Pass 2 cannot recover the natural flow.

## Quick start

### With Groq (recommended)

Get a free API key at [console.groq.com](https://console.groq.com).

```bash
export GROQ_API_KEY=gsk_...
cargo run -p nheengatu-cli --release -- \
  -i book.epub -o book-pt-a2.epub \
  -t pt -l A2
```

### With Ollama (local, no API key)

```bash
ollama pull llama3.1:8b
cargo run -p nheengatu-cli --release -- \
  -b ollama -i book.epub -o book-pt-a2.epub \
  -t pt -l A2
```

## CLI options

```
-i, --input <FILE>              Input EPUB
-o, --output <FILE>             Output EPUB
-t, --target-lang <LANG>        Target language code (de, pt, en, fr, es, …)
-l, --level <LEVEL>             CEFR level: A1 A2 B1 B2 C1 C2
    --source-lang <LANG>        Source language (auto-detected from EPUB if omitted)
-b, --backend <BACKEND>         groq or ollama [default: groq]
-m, --model <MODEL>             Model override
    --simplify-backend <B>      Backend for pass 1 (simplify). Defaults to --backend
    --translate-model <MODEL>   Model for pass 2 (translate)
    --chapters <N,N,…>          Only translate these chapters (1-based)
    --two-pass                  Force two-pass at any level
    --max-chunk-words <N>       Words per chunk [default: 2500]
    --groq-api-key <KEY>        [env: GROQ_API_KEY]
    --ollama-url <URL>          [default: http://localhost:11434]
```

## Supported languages

Any language the underlying model handles well. Tested: `de` `en` `fr` `es` `pt` `it` `nl` `pl` `ru` `ja` `zh`.

## CEFR levels

| Level | Description |
|-------|-------------|
| A1 | Beginner — very simple words, present tense only |
| A2 | Elementary — everyday topics, basic phrases |
| B1 | Intermediate — familiar topics, clear structure |
| B2 | Upper-intermediate — complex topics, some nuance |
| C1 | Advanced — nuanced, idiomatic |
| C2 | Mastery — near-native |

## Using the core library

`nheengatu-core` exposes the full pipeline for use in your own applications:

```rust
use nheengatu_core::{
    pipeline::{run_pipeline, PipelineConfig},
    translator::GroqTranslator,
};
use std::sync::Arc;

let translator = Arc::new(GroqTranslator::new(api_key)?);
let config = PipelineConfig {
    target_lang: "pt".into(),
    level: "A2".into(),
    ..Default::default()
};
run_pipeline(
    Path::new("book.epub"),
    Path::new("book-pt-a2.epub"),
    &config,
    translator.as_ref(),
    translator.as_ref(),
).await?;
```

Implement the `Translator` trait to plug in any LLM backend:

```rust
#[async_trait]
impl Translator for MyBackend {
    async fn translate_chunk(&self, text: &str, source: &str, target: &str, level: &str)
        -> Result<String, TranslateError> { … }
}
```

## Project structure

```
core/   Library crate — EPUB parsing, chunking, translator trait, pipeline
cli/    Binary crate — command-line interface
tests/  Integration tests and fixtures
```

The hosted service (`web/`) lives in a separate private repository.

## Building and testing

```bash
cargo build
cargo test
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).
