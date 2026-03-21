# gunnlod

Translate and simplify EPUB books to a target CEFR language level.

Parses the source EPUB, splits it into chunks, sends each chunk to an LLM with instructions to simplify to the requested CEFR level, and assembles the result into a new EPUB.

## Requirements

- Rust (stable)
- [Ollama](https://ollama.com) for local inference, or a Groq API key for cloud inference

## Quick start with Ollama

1. Install Ollama and pull a model:

```bash
ollama pull llama3.1:8b
```

2. Build the CLI:

```bash
cargo build -p gunnlod-cli --release
```

3. Translate a book:

```bash
cargo run -p gunnlod-cli --release -- \
  -b ollama \
  -i book.epub \
  -o book-simplified.epub \
  -t de \
  -l A2
```

That simplifies `book.epub` to German at CEFR A2 level.

## Options

```
-i, --input <INPUT>                    Input EPUB file
-o, --output <OUTPUT>                  Output EPUB file
-t, --target-lang <TARGET_LANG>        Target language (e.g. de, en, fr)
-l, --level <LEVEL>                    CEFR level (A1, A2, B1, B2, C1, C2)
    --source-lang <SOURCE_LANG>        Source language override (auto-detected if omitted)
-b, --backend <BACKEND>                groq or ollama [default: groq]
    --groq-api-key <GROQ_API_KEY>      Groq API key [env: GROQ_API_KEY]
    --ollama-url <OLLAMA_URL>          Ollama base URL [default: http://localhost:11434]
-m, --model <MODEL>                    Model override (default: llama3.1:8b for Ollama, llama-3.3-70b-versatile for Groq)
    --max-chunk-words <MAX_CHUNK_WORDS> Max words per translation chunk [default: 2500]
    --chapters <CHAPTERS>              Only translate these chapters (1-based, comma-separated)
```

## Backends

### Ollama (local)

Requires Ollama running locally. Any chat model works — larger models produce better simplifications.

```bash
# Pull a model first
ollama pull llama3.1:8b        # 8 billion parameters — good balance of speed and quality
ollama pull llama3.3:70b       # 70 billion parameters — better quality, needs ~64 GB RAM

# Run with a specific model
cargo run -p gunnlod-cli -- -b ollama -m llama3.3:70b -i book.epub -o out.epub -t de -l B1
```

### Groq (cloud)

Requires a [Groq API key](https://console.groq.com). Uses `llama-3.3-70b-versatile` by default.

```bash
export GROQ_API_KEY=gsk_...
cargo run -p gunnlod-cli -- -i book.epub -o out.epub -t de -l A2
```

Note: Groq's free tier (~6K tokens/min) is too slow for full books. Upgrade to a paid tier or use Ollama.

## Testing with a single chapter

Use `--chapters` to translate only specific chapters (1-based). Useful for quickly checking output quality without waiting for a full book:

```bash
# Translate only chapter 1
cargo run -p gunnlod-cli -- -b ollama -i book.epub -o out.epub -t de -l A2 --chapters 1

# Translate chapters 1 and 3
cargo run -p gunnlod-cli -- -b ollama -i book.epub -o out.epub -t de -l A2 --chapters 1,3
```

## Supported languages

Any language the underlying model knows. Well-tested:

| Code | Language |
|------|----------|
| de | German |
| en | English |
| fr | French |
| es | Spanish |
| pt | Portuguese |
| it | Italian |
| nl | Dutch |
| pl | Polish |
| ru | Russian |
| ja | Japanese |
| zh | Chinese |

Pass the code with `-t` (target) and optionally `--source-lang` (auto-detected from the EPUB metadata if omitted).

## CEFR levels

| Level | Description |
|-------|-------------|
| A1 | Beginner — very simple words, present tense |
| A2 | Elementary — everyday topics, basic phrases |
| B1 | Intermediate — familiar topics, straightforward text |
| B2 | Upper-intermediate — complex topics, detailed text |
| C1 | Advanced — nuanced, idiomatic |
| C2 | Mastery — effectively equivalent to native speaker |

### Translation pipeline

For **A1 and A2**, gunnlod uses a two-pass pipeline automatically:

1. **Simplify** — the model rewrites each chunk in the source language at the target CEFR level, focusing purely on structure and vocabulary without switching languages.
2. **Translate** — the simplified text is then translated faithfully to the target language.

Separating these steps produces significantly better results at low levels: in testing, two-pass A1/A2 output showed 15–25% higher coverage of Goethe-Institut vocabulary lists compared to single-pass. It also prevents the looping and language leakage that occurs when a model tries to simplify and translate simultaneously.

For **B1 and above**, a single pass is used — and this is intentional, not just a cost saving. Testing shows that forcing two-pass at B1 *hurts* quality: Pass 1 aggressively breaks sentences down to A1/A2 structure, and Pass 2 cannot recover the natural flow that B1 prose requires. Single-pass with B1 instructions produces more idiomatic, naturally-flowing text.

You can override the default with `--two-pass` to force two-pass at any level, but it is not recommended for B1+.

You can use different models for each pass with `--simplify-backend` and `--translate-model`:

```bash
# Simplify with a large cloud model, translate with a fast local model
cargo run -p gunnlod-cli -- \
  --simplify-backend groq \
  -b ollama --translate-model llama3.1:8b \
  -i book.epub -o out.epub -t pt -l A2
```

## Project structure

```
core/    Library crate — parsing, chunking, translation, EPUB output
cli/     Binary crate — command-line interface
tests/   Test fixtures (test.epub)
```

## Testing

```bash
cargo test
```
