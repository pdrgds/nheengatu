#!/bin/bash
# Two-pass test matrix for gunnlod — all runs via Groq.
#
# Combos tested:
#   70b_70b  — simplify=llama-3.3-70b  translate=llama-3.3-70b
#   8b_8b    — simplify=llama-3.1-8b   translate=llama-3.1-8b
#   70b_8b   — simplify=llama-3.3-70b  translate=llama-3.1-8b
#
# Usage:
#   ./tools/run_tests.sh <input.epub> <chapter> <target_lang>

set -e

INPUT="${1:?Usage: run_tests.sh <input.epub> <chapter> <target_lang>}"
CHAPTER="${2:?}"
LANG="${3:?}"
OUT_DIR="/tmp/gunnlod_tests/$(basename "$INPUT" .epub)_ch${CHAPTER}_${LANG}"

mkdir -p "$OUT_DIR"

BIN="./target/release/gunnlod-cli"

# Load .env if present
if [ -f .env ]; then
    set -a; source .env; set +a
fi

if [ -z "$GROQ_API_KEY" ]; then
    echo "Error: GROQ_API_KEY not set. Add it to .env or export it."
    exit 1
fi

LEVELS=("A1" "A2")

# "label simplify_model translate_model"
COMBOS=(
    "70b_70b llama-3.3-70b-versatile  llama-3.3-70b-versatile"
    "8b_8b   llama-3.1-8b-instant     llama-3.1-8b-instant"
    "70b_8b  llama-3.3-70b-versatile  llama-3.1-8b-instant"
)

GRAND_TOTAL=$(( ${#LEVELS[@]} * ${#COMBOS[@]} ))

echo "=== gunnlod test matrix (two-pass, all Groq) ==="
echo "Input  : $INPUT  (chapter $CHAPTER, lang $LANG)"
echo "Levels : ${LEVELS[*]}"
echo ""
printf "  %-12s %-30s %s\n" "label" "simplify" "translate"
for c in "${COMBOS[@]}"; do
    read -r lbl sm tm <<< "$c"
    printf "  %-12s %-30s %s\n" "$lbl" "$sm" "$tm"
done
echo ""
echo "Total  : $GRAND_TOTAL runs"
echo "Output : $OUT_DIR"
echo ""

RESULTS_FILE=$(mktemp)

run_one() {
    local level="$1" lbl="$2" sm="$3" tm="$4"
    local label="${level}_${lbl}"
    local out="$OUT_DIR/${label}.epub"

    echo "[$label] starting  (simplify=$sm → translate=$tm)"
    set +e
    output=$(
        $BIN \
            -b groq -m "$sm" --translate-model "$tm" \
            --simplify-backend groq \
            --prompt detailed \
            --chapters "$CHAPTER" \
            -i "$INPUT" -o "$out" \
            -t "$LANG" -l "$level" 2>&1
    )
    status=$?
    set -e

    echo "$output" | grep -E "chunks to translate|Done:" | sed "s/^/[$label] /"
    if [ $status -eq 0 ]; then
        echo "[$label] -> $out"
        echo "ok $label" >> "$RESULTS_FILE"
    else
        echo "[$label] FAILED"
        echo "$output" | tail -5 | sed "s/^/[$label]   /"
        echo "fail $label" >> "$RESULTS_FILE"
    fi
}

# Run all combos sequentially to respect Groq rate limits
for level in "${LEVELS[@]}"; do
    for combo in "${COMBOS[@]}"; do
        read -r lbl sm tm <<< "$combo"
        run_one "$level" "$lbl" "$sm" "$tm"
    done
done

# Summarise
PASSED=()
FAILED=()
while IFS= read -r line; do
    if [[ "$line" == ok* ]];   then PASSED+=("${line#ok }"); fi
    if [[ "$line" == fail* ]]; then FAILED+=("${line#fail }"); fi
done < "$RESULTS_FILE"
rm -f "$RESULTS_FILE"

echo ""
echo "=== Done: ${#PASSED[@]} passed, ${#FAILED[@]} failed ==="
echo ""
echo "Output files:"
ls "$OUT_DIR"/*.epub 2>/dev/null

if [ ${#FAILED[@]} -gt 0 ]; then
    echo ""
    echo "Failed:"
    for f in "${FAILED[@]}"; do echo "  - $f"; done
    exit 1
fi
