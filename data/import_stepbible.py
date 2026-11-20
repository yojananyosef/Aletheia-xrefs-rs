#!/usr/bin/env python3
"""Build the original-language layer from STEPBible Data (CC BY 4.0).

Inputs (download into data/raw/stepbible/):
  TAHOT Gen-Deu / Jos-Est / Job-Sng / Isa-Mal  -> Hebrew/Aramaic words
  TAGNT Mat-Jhn / Act-Rev                      -> Greek words
  TBESH / TBESG                                -> brief lexicons

Output:
  crates/app/data/words.bin.gz  (WRD1: verse-indexed words + lexicon)

The format is documented in crates/core/src/words.rs. Run from the repository
root, then rebuild the app with Trunk.
"""
from __future__ import annotations

import gzip
import html
import re
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RAW = ROOT / "data" / "raw" / "stepbible"
OUT = ROOT / "crates" / "app" / "data"

VERSE_COUNT = 31_102
MORPH_NONE = 0xFFFF

# STEPBible abbreviations -> canonical index (0..65), KJV order.
BOOKS = {
    "Gen": 0, "Exo": 1, "Lev": 2, "Num": 3, "Deu": 4, "Jos": 5,
    "Jdg": 6, "Rut": 7, "1Sa": 8, "2Sa": 9, "1Ki": 10, "2Ki": 11,
    "1Ch": 12, "2Ch": 13, "Ezr": 14, "Neh": 15, "Est": 16, "Job": 17,
    "Psa": 18, "Pro": 19, "Ecc": 20, "Sng": 21, "Isa": 22, "Jer": 23,
    "Lam": 24, "Ezk": 25, "Dan": 26, "Hos": 27, "Jol": 28, "Amo": 29,
    "Oba": 30, "Jon": 31, "Mic": 32, "Nam": 33, "Hab": 34, "Zep": 35,
    "Hag": 36, "Zec": 37, "Mal": 38, "Mat": 39, "Mrk": 40, "Luk": 41,
    "Jhn": 42, "Act": 43, "Rom": 44, "1Co": 45, "2Co": 46, "Gal": 47,
    "Eph": 48, "Php": 49, "Col": 50, "1Th": 51, "2Th": 52, "1Ti": 53,
    "2Ti": 54, "Tit": 55, "Phm": 56, "Heb": 57, "Jas": 58, "1Pe": 59,
    "2Pe": 60, "1Jn": 61, "2Jn": 62, "3Jn": 63, "Jud": 64, "Rev": 65,
}

HEBREW_REF = re.compile(r"^([1-3]?[A-Za-z]{2,3}\.\d+\.\d+)#(\d+)=([^\s]+)")
GREEK_REF = re.compile(
    r"^([1-3]?[A-Za-z]{2,3}\.\d+\.\d+)(?:\[([^\]]+)\])?#(\d+)=([^\s]+)"
)
STRONG_RE = re.compile(r"[GH]\d+[A-Za-z]?")
LEX_KEY_RE = re.compile(r"^[GH]\d+[A-Za-z]?$")


@dataclass
class Word:
    surface: str
    strong: str
    morph: str
    lang: int  # 0 Hebrew, 1 Aramaic, 2 Greek
    na: bool = True  # part of the NA27/28 base text (Greek only)
    kjv: bool = True  # part of the KJV/TR text (Greek only)
    fallback: bool = False  # verse uses the Byzantine/TR text instead of NA


def kjv_counts() -> list[list[int]]:
    text = (ROOT / "crates/core/src/tables.rs").read_text(encoding="utf-8")
    match = re.search(
        r"KJV_CHAPTER_VERSE_COUNTS: \[&\[u16\]; 66\] = \[(.*?)\n\];",
        text,
        re.S,
    )
    if not match:
        raise SystemExit("tables.rs: KJV_CHAPTER_VERSE_COUNTS not found")
    counts = [
        [int(value) for value in re.findall(r"\d+", group)]
        for group in re.findall(r"&\[([0-9, ]+)\]", match.group(1))
    ]
    if len(counts) != 66 or sum(map(sum, counts)) != VERSE_COUNT:
        raise SystemExit("tables.rs: unexpected KJV table")
    return counts


COUNTS = kjv_counts()
BOOK_STARTS = [sum(map(sum, COUNTS[:book])) for book in range(66)]


def verse_index(ref: str) -> int | None:
    match = re.match(r"^([1-3]?[A-Za-z]{2,3})\.(\d+)\.(\d+)$", ref)
    if not match:
        return None
    book = BOOKS.get(match.group(1))
    if book is None:
        return None
    chapter, verse = int(match.group(2)), int(match.group(3))
    if verse == 0 or chapter < 1 or chapter > len(COUNTS[book]):
        return None
    if verse > COUNTS[book][chapter - 1]:
        return None
    return BOOK_STARTS[book] + sum(COUNTS[book][: chapter - 1]) + verse - 1


def clean_strong(key: str) -> str:
    key = key.strip().rstrip(",;")
    return re.sub(r"_[A-Za-z]+$", "", key)


def hebrew_strong(dstrongs: str, root: str) -> str | None:
    # "Root dStrong+Instance" is the lexical root; fall back to the braced
    # element of the full tag (prefixes/suffixes are separate Strongs).
    for candidate in STRONG_RE.findall(root or ""):
        return clean_strong(candidate)
    braced = re.findall(r"\{\s*([GH]\d+[A-Za-z]?)\s*\}", dstrongs or "")
    if braced:
        return clean_strong(braced[-1])
    candidates = STRONG_RE.findall(dstrongs or "")
    return clean_strong(candidates[-1]) if candidates else None


def greek_strong(dstrongs: str, sstrong: str) -> str | None:
    for value in (sstrong, dstrongs):
        match = re.match(r"([GH]\d+[A-Za-z]?)", value or "")
        if match:
            return clean_strong(match.group(1))
    return None


def clean_hebrew(surface: str) -> str:
    # Backslash marks punctuation and paragraph markers: \׃ \־ \׀ \ס \פ.
    surface = re.sub(r"\\([־׃׀ספ\s])", "", surface)
    surface = surface.replace("/", "")
    surface = surface.replace("\\", "")
    return surface.strip()


def clean_greek(surface: str) -> str:
    # Field 1 is "surface (transliteration)"; punctuation is stripped so the
    # reader can decide its own spacing and sentence marks.
    surface = surface.split(" (", 1)[0].strip()
    surface = surface.replace("¶", "")
    surface = surface.replace("[", "").replace("]", "").replace("¬", "")
    surface = re.sub(r"[\s.,;·]+$", "", surface)
    return surface.strip()


def clean_lexicon_text(text: str, *, multiline: bool = True) -> str:
    text = re.sub(r"(?i)<br\s*/?>", "\n", text)
    text = re.sub(r"<[^>]+>", "", text)
    text = html.unescape(text)
    text = text.replace("\xa0", " ")
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r" *\n *", "\n", text)
    text = re.sub(r"\n{3,}", "\n\n", text)
    if not multiline:
        text = text.replace("\n", " ")
    return text.strip()


def load_lexicon() -> dict[str, dict[str, str]]:
    entries: dict[str, dict[str, str]] = {}
    for filename in ("tbesh.txt", "tbesg.txt"):
        path = RAW / filename
        if not path.exists():
            raise SystemExit(f"missing {path}")
        with path.open(encoding="utf-8-sig") as handle:
            for line in handle:
                fields = line.rstrip("\n").split("\t")
                if len(fields) < 8:
                    continue
                key = re.split(r"[\s=]+", fields[1].strip())[0]
                if not LEX_KEY_RE.match(key):
                    continue
                entry = {
                    "lemma": clean_lexicon_text(fields[3], multiline=False),
                    "translit": clean_lexicon_text(fields[4], multiline=False),
                    "morph": clean_lexicon_text(fields[5], multiline=False),
                    "gloss": clean_lexicon_text(fields[6], multiline=False),
                    "meaning": clean_lexicon_text(fields[7]),
                }
                current = entries.get(key)
                if current is None or (not current["meaning"] and entry["meaning"]):
                    entries[key] = entry
    return entries


def match_lexicon(key: str, entries: dict[str, dict[str, str]]) -> dict[str, str] | None:
    if key in entries:
        return entries[key]
    # Word tagging sometimes uses a base number where the lexicon splits the
    # sense into H7225G/H7225H (or G0001G/H). Pick the first sibling.
    base = re.match(r"^([GH]\d+)", key)
    if not base:
        return None
    prefix = re.escape(base.group(1))
    siblings = sorted(
        candidate
        for candidate in entries
        if re.fullmatch(prefix + r"[A-Za-z]", candidate)
    )
    return entries[siblings[0]] if siblings else None


def parse() -> tuple[list[list[Word]], dict[str, int], list[str]]:
    verses: list[list[Word]] = [[] for _ in range(VERSE_COUNT)]
    skipped_extra = 0
    skipped_unknown = 0

    for filename in (
        "tahot_gen-deu.txt",
        "tahot_jos-est.txt",
        "tahot_job-sng.txt",
        "tahot_isa-mal.txt",
    ):
        path = RAW / filename
        with path.open(encoding="utf-8-sig") as handle:
            for line in handle:
                if line.startswith("#") or "\t" not in line:
                    continue
                fields = line.rstrip("\n").split("\t")
                if len(fields) < 12:
                    continue
                match = HEBREW_REF.match(fields[0])
                if not match:
                    continue
                ref, text_type = match.group(1), match.group(3)
                if ref.endswith(".0") or text_type.startswith("X"):
                    skipped_extra += 1
                    continue
                index = verse_index(ref)
                if index is None:
                    skipped_unknown += 1
                    continue
                surface = clean_hebrew(fields[1])
                if not surface:
                    continue
                strong = hebrew_strong(fields[4], fields[8])
                if not strong:
                    continue
                morph = fields[5].strip()
                verses[index].append(
                    Word(
                        surface=surface,
                        strong=strong,
                        morph=morph,
                        lang=1 if morph.startswith("A") else 0,
                    )
                )

    for filename in ("tagnt_mat-jhn.txt", "tagnt_act-rev.txt"):
        path = RAW / filename
        with path.open(encoding="utf-8-sig") as handle:
            for line in handle:
                if line.startswith("#") or "\t" not in line:
                    continue
                fields = line.rstrip("\n").split("\t")
                if len(fields) < 12:
                    continue
                match = GREEK_REF.match(fields[0])
                if not match:
                    continue
                primary, bracket, text_type = match.group(1), match.group(2), match.group(4)
                if bracket:
                    parts = bracket.split(".")
                    use = (
                        f"{primary.split('.')[0]}.{bracket}"
                        if len(parts) == 2
                        else bracket
                    )
                else:
                    use = primary
                # KJV folds these verses into the previous chapter/verse.
                if primary == "3Jn.1.15":
                    use = "3Jn.1.14"
                elif primary == "Rev.12.18":
                    use = "Rev.13.1"
                index = verse_index(use)
                if index is None:
                    skipped_unknown += 1
                    continue
                surface = clean_greek(fields[1])
                if not surface:
                    continue
                strong = greek_strong(fields[3], fields[11])
                if not strong:
                    continue
                morph = fields[3].split("=", 1)[1] if "=" in fields[3] else ""
                verses[index].append(
                    Word(
                        surface=surface,
                        strong=strong,
                        morph=morph.strip(),
                        lang=2,
                        na=("N" in text_type or "n" in text_type),
                        kjv=("K" in text_type or "k" in text_type),
                    )
                )

    # Greek base text: keep NA27/28 words. If a KJV verse has no NA witness
    # (Acts 8:37, Mark 9:44/46, ...), fall back to the Byzantine/TR words.
    for index, words in enumerate(verses):
        if index < BOOK_STARTS[39]:
            continue
        if any(word.na for word in words):
            verses[index] = [word for word in words if word.na]
        elif any(word.kjv for word in words):
            kept = [word for word in words if word.kjv]
            for word in kept:
                word.fallback = True
            verses[index] = kept

    used: dict[str, int] = {}
    morphs: set[str] = set()
    for words in verses:
        for word in words:
            used[word.strong] = used.get(word.strong, 0) + 1
            if word.morph:
                morphs.add(word.morph)

    print(
        f"parsed {sum(map(len, verses))} words, {len(used)} Strong keys, "
        f"{len(morphs)} morphology codes "
        f"(skipped {skipped_extra} extra/title rows, {skipped_unknown} unknown refs)"
    )
    return verses, used, sorted(morphs)


def build_binary(
    verses: list[list[Word]],
    used: dict[str, int],
    morph_codes: list[str],
    lexicon: dict[str, dict[str, str]],
) -> bytes:
    if len(morph_codes) >= MORPH_NONE:
        raise SystemExit(f"too many morphology codes: {len(morph_codes)}")

    keys = sorted(used)
    lex_id = {key: index for index, key in enumerate(keys)}

    morph_id = {code: index for index, code in enumerate(morph_codes)}
    blob = bytearray()
    word_records = bytearray()
    verse_offsets = [0]
    for words in verses:
        for word in words:
            encoded = word.surface.encode("utf-8")
            offset = len(blob)
            blob.extend(encoded)
            morph = morph_id.get(word.morph, MORPH_NONE)
            meta = word.lang | (8 if word.fallback else 0)
            word_records.extend(
                struct.pack(
                    "<IHIHH", offset, len(encoded), lex_id[word.strong], morph, meta
                )
            )
        verse_offsets.append(len(word_records) // 14)

    morph_blob = bytearray()
    morph_offsets = [0]
    for code in morph_codes:
        morph_blob.extend(code.encode("utf-8"))
        morph_offsets.append(len(morph_blob))

    lex_blob = bytearray()
    lex_records = bytearray()

    def add_text(value: str) -> tuple[int, int]:
        encoded = value.encode("utf-8")
        offset = len(lex_blob)
        lex_blob.extend(encoded)
        return offset, len(encoded)

    for key in keys:
        entry = match_lexicon(key, lexicon) or {
            "lemma": key,
            "translit": "",
            "morph": "",
            "gloss": "",
            "meaning": "",
        }
        values = [
            key,
            entry["lemma"],
            entry["translit"],
            entry["morph"],
            entry["gloss"],
            entry["meaning"],
        ]
        parts: list[int] = []
        for value in values:
            offset, length = add_text(value)
            parts.extend((offset, length))
        lex_records.extend(struct.pack("<12I", *parts))

    header = struct.pack(
        "<4sHHIIIIII",
        b"WRD1",
        1,
        0,
        VERSE_COUNT,
        len(word_records) // 14,
        len(blob),
        len(keys),
        len(morph_codes),
        len(morph_blob),
    )
    raw = bytearray(header)
    raw.extend(struct.pack(f"<{len(verse_offsets)}I", *verse_offsets))
    raw.extend(word_records)
    raw.extend(blob)
    raw.extend(struct.pack(f"<{len(morph_offsets)}I", *morph_offsets))
    raw.extend(morph_blob)
    raw.extend(lex_records)
    raw.extend(lex_blob)
    return bytes(raw)


def main() -> None:
    if not RAW.exists():
        raise SystemExit(
            f"missing {RAW}; download TAHOT/TAGNT/TBESH/TBESG from "
            "github.com/STEPBible/STEPBible-Data"
        )
    lexicon = load_lexicon()
    verses, used, morph_codes = parse()
    raw = build_binary(verses, used, morph_codes, lexicon)
    compressed = gzip.compress(raw, compresslevel=9, mtime=0)
    OUT.mkdir(parents=True, exist_ok=True)
    output = OUT / "words.bin.gz"
    output.write_bytes(compressed)
    print(
        f"wrote {output}: {len(raw) / 1e6:.1f} MB raw, "
        f"{len(compressed) / 1e6:.1f} MB gzip, {len(used)} lexicon entries"
    )
    missing = [key for key in used if match_lexicon(key, lexicon) is None]
    if missing:
        print(f"note: {len(missing)} keys without a lexicon entry (stubs): {missing[:8]}")


if __name__ == "__main__":
    sys.exit(main())
