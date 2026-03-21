#!/usr/bin/env python3
"""
Evaluate vocabulary level coverage of an EPUB against Goethe-Institut word lists.

Usage:
    python tools/eval_level.py book.epub [--lang de] [--chapters 1-4:A1 5-10:A2 11-17:B1]

Requirements:
    pip install requests

Word lists are downloaded automatically from the ilkermeliksitki/goethe-institute-wordlist
GitHub repo (official Goethe-Institut TSV files) and cached locally.
"""

import argparse
import re
import sys
import zipfile
import json
from pathlib import Path
from collections import defaultdict
from urllib.request import urlopen
from xml.etree import ElementTree as ET

CACHE_DIR = Path.home() / ".cache" / "gunnlod" / "wordlists"
WORDLIST_BASE = "https://raw.githubusercontent.com/ilkermeliksitki/goethe-institute-wordlist/main"
LEVEL_DIRS = {"A1": "a1", "A2": "a2", "B1": "b1"}
LETTERS = list("abcdefghijklmnoprstuvwz")


def download_wordlists():
    import urllib.request, json
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    lists = {}
    for level, folder in LEVEL_DIRS.items():
        cache_file = CACHE_DIR / f"{level}.txt"
        if not cache_file.exists():
            print(f"Downloading {level} word list...", file=sys.stderr)
            words_all = set()
            for letter in LETTERS:
                url = f"{WORDLIST_BASE}/{folder}/{letter}.tsv"
                try:
                    with urllib.request.urlopen(url) as r:
                        for line in r.read().decode("utf-8").splitlines():
                            parts = line.strip().split("\t")
                            if parts and parts[0].strip():
                                words_all.add(parts[0].lower().strip())
                except Exception:
                    pass  # letter file may not exist
            cache_file.write_text("\n".join(sorted(words_all)), encoding="utf-8")
        words = set(cache_file.read_text(encoding="utf-8").splitlines())
        lists[level] = words
    return lists


def extract_chapters(epub_path):
    chapters = []
    with zipfile.ZipFile(epub_path) as zf:
        container = ET.fromstring(zf.read("META-INF/container.xml"))
        ns = {"c": "urn:oasis:names:tc:opendocument:xmlns:container"}
        opf_path = container.find(".//c:rootfile", ns).get("full-path")
        opf_dir = str(Path(opf_path).parent)

        opf = ET.fromstring(zf.read(opf_path))
        opf_ns = {"opf": "http://www.idpf.org/2007/opf"}
        manifest = {item.get("id"): item.get("href") for item in opf.findall(".//opf:item", opf_ns)}
        spine = [manifest[ref.get("idref")] for ref in opf.findall(".//opf:itemref", opf_ns)]

        for href in spine:
            full = f"{opf_dir}/{href}".lstrip("/") if opf_dir and opf_dir != "." else href
            try:
                html = zf.read(full).decode("utf-8", errors="replace")
            except KeyError:
                try:
                    html = zf.read(href).decode("utf-8", errors="replace")
                except KeyError:
                    continue
            text = re.sub(r"<[^>]+>", " ", html)
            text = re.sub(r"\s+", " ", text).strip()
            chapters.append(text)
    return chapters


def tokenize(text):
    # Extract German words: lowercase, strip punctuation
    tokens = re.findall(r"\b[a-zA-ZäöüÄÖÜß]{2,}\b", text)
    return [t.lower() for t in tokens]


def stem(word):
    # Very basic German inflection stripping for better matching
    for suffix in ["ungen", "ung", "keit", "heit", "lich", "isch", "sten", "sten",
                   "ern", "eln", "end", "est", "ten", "tes", "tem", "ten", "ter",
                   "nen", "nen", "en", "er", "es", "em", "et", "st", "e", "s"]:
        if word.endswith(suffix) and len(word) - len(suffix) >= 3:
            return word[:-len(suffix)]
    return word


def coverage(words, wordlist):
    if not words:
        return 0.0, set()
    matched = set()
    unmatched = set()
    unique = set(words)
    for w in unique:
        if w in wordlist or stem(w) in wordlist:
            matched.add(w)
        else:
            unmatched.add(w)
    return len(matched) / len(unique) * 100, unmatched


def parse_ranges(ranges_str):
    """Parse '1-4:A1 5-10:A2 11-17:B1' into {1: 'A1', 2: 'A1', ..., 17: 'B1'}"""
    chapter_levels = {}
    for part in ranges_str.split():
        rng, level = part.split(":")
        start, end = (int(x) for x in rng.split("-"))
        for i in range(start, end + 1):
            chapter_levels[i] = level
    return chapter_levels


def main():
    parser = argparse.ArgumentParser(description="Evaluate EPUB vocabulary against Goethe word lists")
    parser.add_argument("epub", help="Path to EPUB file")
    parser.add_argument("--chapters", default="",
                        help="Chapter level ranges, e.g. '1-4:A1 5-10:A2 11-17:B1'. "
                             "If omitted, all chapters are evaluated against all lists.")
    parser.add_argument("--json", action="store_true", help="Output as JSON")
    args = parser.parse_args()

    wordlists = download_wordlists()
    combined = {
        "A1":       wordlists["A1"],
        "A1+A2":    wordlists["A1"] | wordlists["A2"],
        "A1+A2+B1": wordlists["A1"] | wordlists["A2"] | wordlists["B1"],
    }

    chapters = extract_chapters(args.epub)
    print(f"Found {len(chapters)} spine items in {Path(args.epub).name}", file=sys.stderr)

    chapter_levels = parse_ranges(args.chapters) if args.chapters else {}

    results = []
    level_groups = defaultdict(list)

    for i, text in enumerate(chapters, 1):
        words = tokenize(text)
        unique = set(words)
        declared = chapter_levels.get(i)

        cov = {}
        for label, wl in combined.items():
            pct, unmatched = coverage(words, wl)
            cov[label] = {"pct": round(pct, 1), "unmatched_sample": sorted(unmatched)[:10]}

        result = {
            "chapter": i,
            "declared_level": declared,
            "total_words": len(words),
            "unique_words": len(unique),
            "coverage": cov,
        }
        results.append(result)
        if declared:
            level_groups[declared].append(result)

    if args.json:
        print(json.dumps(results, ensure_ascii=False, indent=2))
        return

    # Pretty print
    for r in results:
        declared = r["declared_level"] or "?"
        print(f"\nChapter {r['chapter']:2d} [{declared}]  {r['total_words']} words, {r['unique_words']} unique")
        for label, data in r["coverage"].items():
            bar = "█" * int(data["pct"] / 5)
            print(f"  {label:10s} {data['pct']:5.1f}%  {bar}")
        if r["coverage"]["A1+A2+B1"]["unmatched_sample"]:
            print(f"  Above B1 sample: {', '.join(r['coverage']['A1+A2+B1']['unmatched_sample'][:8])}")

    if level_groups:
        print("\n=== Summary ===")
        for level in ["A1", "A2", "B1"]:
            if level not in level_groups:
                continue
            group = level_groups[level]
            key = {"A1": "A1", "A2": "A1+A2", "B1": "A1+A2+B1"}[level]
            avg = sum(r["coverage"][key]["pct"] for r in group) / len(group)
            print(f"{level} chapters ({len(group)}): avg {avg:.1f}% coverage of {key} list")


if __name__ == "__main__":
    main()
