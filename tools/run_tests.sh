#!/bin/bash
# Prompt × model test matrix for gunnlod.
#
# Usage:
#   ./tools/run_tests.sh <input.epub> <chapter> <target_lang> <level>
#
# Example:
#   ./tools/run_tests.sh book.epub 1 pt A2
#
# Requires:
#   - Ollama running with llama3.1:8b and llama3.3:70b
#   - GROQ_API_KEY set in environment (for big-model-cloud runs)
#   - cargo build -p gunnlod-cli --release already run

set -e

INPUT="${1:?Usage: run_tests.sh <input.epub> <chapter> <target_lang> <level>}"
CHAPTER="${2:?}"
LANG="${3:?}"
LEVEL="${4:?}"
OUT_DIR="/tmp/gunnlod_tests/$(basename "$INPUT" .epub)_ch${CHAPTER}_${LANG}_${LEVEL}"

mkdir -p "$OUT_DIR"

CLI="cargo run -p gunnlod-cli --release --"

echo "=== Test matrix: chapter $CHAPTER | lang $LANG | level $LEVEL ==="
echo "Output dir: $OUT_DIR"
echo ""

run() {
    local label="$1"; shift
    local out="$OUT_DIR/${label}.epub"
    echo -n "Running [$label]... "
    if $CLI "$@" --chapters "$CHAPTER" -i "$INPUT" -o "$out" -t "$LANG" -l "$LEVEL" 2>&1 | grep -E "^\s+\[|Done:"; then
        echo "  -> $out"
    fi
}

# 1. Simple prompt, small model (local)
run "simple_small" -b ollama -m llama3.1:8b --prompt simple

# 2. Detailed prompt, small model (local)
run "detailed_small" -b ollama -m llama3.1:8b --prompt detailed

# 3. Simple prompt, big model (Groq)
if [ -n "$GROQ_API_KEY" ]; then
    run "simple_big_groq" -b groq --prompt simple
else
    echo "Skipping simple_big_groq (GROQ_API_KEY not set)"
fi

# 4. Detailed prompt, big model (Groq)
if [ -n "$GROQ_API_KEY" ]; then
    run "detailed_big_groq" -b groq --prompt detailed
else
    echo "Skipping detailed_big_groq (GROQ_API_KEY not set)"
fi

# 5. Detailed prompt, big model (local, optional)
if ollama list 2>/dev/null | grep -q "llama3.3:70b"; then
    run "detailed_big_local" -b ollama -m llama3.3:70b --prompt detailed
else
    echo "Skipping detailed_big_local (llama3.3:70b not pulled)"
fi

echo ""
echo "=== Done. Open these files side by side to compare: ==="
ls "$OUT_DIR"/*.epub 2>/dev/null
