#!/bin/bash
# Prompt × model × level test matrix for gunnlod.
# Local (Ollama) and remote (Groq) runs execute in parallel.
#
# Usage:
#   ./tools/run_tests.sh <input.epub> <chapter> <target_lang>
#
# Example:
#   ./tools/run_tests.sh book.epub 1 pt
#
# Requires:
#   - Ollama running with llama3.1:8b
#   - GROQ_API_KEY in env or .env file (for cloud runs)

set -e

INPUT="${1:?Usage: run_tests.sh <input.epub> <chapter> <target_lang>}"
CHAPTER="${2:?}"
LANG="${3:?}"
OUT_DIR="/tmp/gunnlod_tests/$(basename "$INPUT" .epub)_ch${CHAPTER}_${LANG}"

mkdir -p "$OUT_DIR"

CLI="cargo run -p gunnlod-cli --release --"

# Load .env if present
if [ -f .env ]; then
    set -a; source .env; set +a
fi

LEVELS=("A1" "A2")

HAS_BIG_LOCAL=false
ollama list 2>/dev/null | grep -q "llama3.3:70b" && HAS_BIG_LOCAL=true

# Model combos: "label simplify_model translate_model"
# label becomes part of output filename: A1_<label>.epub
COMBOS=()
COMBOS+=("8b_8b llama3.1:8b llama3.1:8b")
if $HAS_BIG_LOCAL; then
    COMBOS+=("70b_8b llama3.3:70b llama3.1:8b")
    COMBOS+=("70b_70b llama3.3:70b llama3.3:70b")
fi

GRAND_TOTAL=$(( ${#LEVELS[@]} * ${#COMBOS[@]} ))

echo "=== gunnlod test matrix (two-pass, all local) ==="
echo "Input   : $INPUT  (chapter $CHAPTER, lang $LANG)"
echo "Levels  : ${LEVELS[*]}"
echo "Combos  : ${#COMBOS[@]}  (70b local: $($HAS_BIG_LOCAL && echo yes || echo "no — only 8b+8b"))"
for c in "${COMBOS[@]}"; do
    read -r lbl sm tm <<< "$c"
    echo "           $lbl  → simplify=$sm  translate=$tm"
done
echo "Total   : $GRAND_TOTAL runs"
echo "Output  : $OUT_DIR"
echo ""

# Temp files to collect pass/fail results from subshells
RESULTS_FILE=$(mktemp)

# Build flat run list: "level label simplify_model translate_model"
ALL_RUNS=()
for level in "${LEVELS[@]}"; do
    for combo in "${COMBOS[@]}"; do
        read -r lbl sm tm <<< "$combo"
        ALL_RUNS+=("$level $lbl $sm $tm")
    done
done

# Run all sequentially (local Ollama can't run two models in parallel)
run_all() {
    local total=${#ALL_RUNS[@]}
    local idx=0
    for run_spec in "${ALL_RUNS[@]}"; do
        idx=$((idx + 1))
        read -r level lbl sm tm <<< "$run_spec"
        label="${level}_${lbl}"
        out="$OUT_DIR/${label}.epub"
        echo "[$idx/$total] $label  (simplify=$sm → translate=$tm)"
        set +e
        output=$(
            $CLI -b ollama \
                -m "$sm" --translate-model "$tm" \
                --prompt detailed \
                --chapters "$CHAPTER" \
                -i "$INPUT" -o "$out" \
                -t "$LANG" -l "$level" 2>&1
        )
        status=$?
        set -e
        echo "$output" | grep -E "chunks to translate|Done:|Pass [12]"
        if [ $status -eq 0 ]; then
            echo "[$idx/$total] $label -> $out"
            echo "ok $label" >> "$RESULTS_FILE"
        else
            echo "[$idx/$total] $label FAILED"
            echo "$output" | tail -3
            echo "fail $label" >> "$RESULTS_FILE"
        fi
    done
}

run_all

# Summarise
PASSED=()
FAILED=()
while IFS= read -r line; do
    if [[ "$line" == ok* ]]; then
        PASSED+=("${line#ok }")
    elif [[ "$line" == fail* ]]; then
        FAILED+=("${line#fail }")
    fi
done < "$RESULTS_FILE"
rm -f "$RESULTS_FILE"

echo ""
echo "=== Done: ${#PASSED[@]} passed, ${#FAILED[@]} failed ==="

if [ ${#PASSED[@]} -gt 0 ]; then
    echo ""
    echo "Output files:"
    ls "$OUT_DIR"/*.epub 2>/dev/null
fi

if [ ${#FAILED[@]} -gt 0 ]; then
    echo ""
    echo "Failed runs:"
    for f in "${FAILED[@]}"; do echo "  - $f"; done
    exit 1
fi
