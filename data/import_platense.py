#!/usr/bin/env python3
"""Convert the CrossWire SpaPlatense OSIS file to KJV-ordered VPL.

The module is tagged Versification=Vulg, but the OSIS text uses a slightly
compressed Vulgate: some split verses are joined and Exodus 8:1-4 is numbered
as Exodus 7:26-29.  We map with SWORD's Vulgate change points, corrected so
that a change point shifts the following verses by target-start - source-start
(SWORD's forward conversion subtracts in the wrong direction for ranges).

Output format: one `<CODE> <chapter>:<verse> <text>` line per KJV verse.
"""
from __future__ import annotations

import html
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
RAW = ROOT / "raw"
OSIS = RAW / "spaplatense.osis.xml"
OUT = RAW / "spaplatense_vpl.txt"

# Vulgate book order used by canon_vulg.h.
VULG_BOOKS = [
    "Gen", "Exod", "Lev", "Num", "Deut", "Josh", "Judg", "Ruth", "1Sam", "2Sam",
    "1Kgs", "2Kgs", "1Chr", "2Chr", "Ezra", "Neh", "Tob", "Jdt", "Esth", "Job",
    "Ps", "Prov", "Eccl", "Song", "Wis", "Sir", "Isa", "Jer", "Lam", "Bar",
    "Ezek", "Dan", "Hos", "Joel", "Amos", "Obad", "Jonah", "Mic", "Nah", "Hab",
    "Zeph", "Hag", "Zech", "Mal", "1Macc", "2Macc", "Matt", "Mark", "Luke",
    "John", "Acts", "Rom", "1Cor", "2Cor", "Gal", "Eph", "Phil", "Col", "1Thess",
    "2Thess", "1Tim", "2Tim", "Titus", "Phlm", "Heb", "Jas", "1Pet", "2Pet",
    "1John", "2John", "3John", "Jude", "Rev", "PrMan", "1Esd", "2Esd", "AddPs",
    "EpLao",
]
VULG_NUMBER = {book: index + 1 for index, book in enumerate(VULG_BOOKS)}

# KJV order, 66 books.  The OSIS ids use these names.
KJV_BOOKS = [
    "Gen", "Exod", "Lev", "Num", "Deut", "Josh", "Judg", "Ruth", "1Sam", "2Sam",
    "1Kgs", "2Kgs", "1Chr", "2Chr", "Ezra", "Neh", "Esth", "Job", "Ps", "Prov",
    "Eccl", "Song", "Isa", "Jer", "Lam", "Ezek", "Dan", "Hos", "Joel", "Amos",
    "Obad", "Jonah", "Mic", "Nah", "Hab", "Zeph", "Hag", "Zech", "Mal", "Matt",
    "Mark", "Luke", "John", "Acts", "Rom", "1Cor", "2Cor", "Gal", "Eph", "Phil",
    "Col", "1Thess", "2Thess", "1Tim", "2Tim", "Titus", "Phlm", "Heb", "Jas",
    "1Pet", "2Pet", "1John", "2John", "3John", "Jude", "Rev",
]
KJV_CODE = {
    book: code
    for book, code in zip(
        KJV_BOOKS,
        [
            "GEN", "EXO", "LEV", "NUM", "DEU", "JOS", "JDG", "RUT", "1SA", "2SA",
            "1KI", "2KI", "1CH", "2CH", "EZR", "NEH", "EST", "JOB", "PSA", "PRO",
            "ECC", "SOL", "ISA", "JER", "LAM", "EZE", "DAN", "HOS", "JOE", "AMO",
            "OBA", "JON", "MIC", "NAH", "HAB", "ZEP", "HAG", "ZEC", "MAL", "MAT",
            "MAR", "LUK", "JOH", "ACT", "ROM", "1CO", "2CO", "GAL", "EPH", "PHI",
            "COL", "1TH", "2TH", "1TI", "2TI", "TIT", "PHM", "HEB", "JAM", "1PE",
            "2PE", "1JO", "2JO", "3JO", "JUD", "REV",
        ],
    )
}

# Books that are deuterocanonical or additions: not addressable from KJV.
SKIP_BOOKS = {"Tob", "Jdt", "Wis", "Sir", "Bar", "1Macc", "2Macc", "PrMan",
              "1Esd", "2Esd", "AddPs", "EpLao"}


def load_rules() -> dict[int, list[tuple[int, int, int, int, int, int]]]:
    """Parse canon_vulg.h mappings_vulg into (src_ch, src_v1, src_v2, dst_ch, dst_v1, dst_v2)."""
    source = (RAW / "canon_vulg.h").read_text(encoding="utf-8")
    block = source[source.index("unsigned char mappings_vulg[] = {") :]
    block = block[: block.index("};")]
    rules: dict[int, list[tuple[int, int, int, int, int, int]]] = {}
    for line in block.splitlines():
        match = re.match(
            r"\s*(\d+),\s*(\d+),\s*(\d+),\s*(\d+),\s*(\d+),\s*(\d+),\s*(\d+),",
            line,
        )
        if not match:
            continue
        book, source_ch, source_v1, source_v2, target_ch, target_v1, target_v2 = map(
            int, match.groups()
        )
        # In the table the first triple is the Vulgate change point and the
        # second is the KJV side (confirmed by Dan 3:91 -> 3:24 and Ps 22 -> 23).
        rules.setdefault(book, []).append(
            (source_ch, source_v1, source_v2, target_ch, target_v1, target_v2)
        )
    return rules


def map_verse(
    rules, book: str, chapter: int, verse: int
) -> list[tuple[str, int, int]]:
    """Map a Vulgate verse to one or more KJV coordinates."""
    book_number = VULG_NUMBER[book]
    for src_ch, src_v1, src_v2, dst_ch, dst_v1, dst_v2 in reversed(
        rules.get(book_number, [])
    ):
        if src_ch < chapter:
            return [(book, chapter, verse)]
        if src_ch != chapter or src_v1 > verse:
            continue
        if src_v1 == 0:
            return [(book, dst_ch, verse)]
        if src_v2 > 0 and verse <= src_v2:
            src_len = src_v2 - src_v1
            dst_len = dst_v2 - dst_v1
            if src_len > 0 and dst_len == src_len:
                return [(book, dst_ch, dst_v1 + (verse - src_v1))]
            if src_len == 0 and dst_len > 0:
                # One Vulgate verse covers several KJV verses.
                return [(book, dst_ch, dst_v1 + offset) for offset in range(dst_len + 1)]
            # Several Vulgate verses were merged into one KJV verse.
            return [(book, dst_ch, dst_v1)]
        if src_v2 > 0:
            return [(book, dst_ch, max(dst_v1, dst_v2) + (verse - src_v2))]
        return [(book, dst_ch, dst_v1 + (verse - src_v1))]
    return [(book, chapter, verse)]


def clean(fragment: str) -> str:
    fragment = re.sub(r"<note\b.*?</note>", " ", fragment, flags=re.S)
    fragment = re.sub(r"<title\b.*?</title>", " ", fragment, flags=re.S)
    fragment = re.sub(r"<lb\s*/?>", " ", fragment)
    fragment = re.sub(r"<[^>]+>", "", fragment)
    fragment = html.unescape(fragment)
    fragment = re.sub(r"\s+", " ", fragment).strip()
    # Removing notes and titles leaves spaces before punctuation, e.g.
    # "vida eterna ." or "dijo :". Spanish never separates them.
    fragment = re.sub(r"\s+([,.;:!?%»”’\)\]])", r"\1", fragment)
    fragment = re.sub(r"([«“‘\(¡¿])\s+", r"\1", fragment)
    return fragment.strip()


def has_content(text: str) -> bool:
    return any(ch.isalnum() for ch in text)


def extract_verses() -> dict[tuple[str, int, int], str]:
    source = OSIS.read_text(encoding="utf-8")
    verses: dict[tuple[str, int, int], str] = {}
    pattern = re.compile(
        r'<verse sID="([^"]+)"[^>]*/>(.*?)<verse eID="\1"\s*/>', re.S
    )
    for match in pattern.finditer(source):
        ref, body = match.group(1), match.group(2)
        parsed = re.match(r"^(.+?)\.(\d+)\.(\d+)$", ref)
        if not parsed:
            continue
        text = clean(body)
        if has_content(text):
            verses[(parsed.group(1), int(parsed.group(2)), int(parsed.group(3)))] = text
    # Mark 9:44 and 9:46 are omitted in this edition: only the closing bracket
    # of an empty block survives. Drop the dangling opening bracket too.
    for (book, chapter, verse), text in list(verses.items()):
        if text.endswith(" [") and (book, chapter, verse + 1) not in verses:
            verses[(book, chapter, verse)] = text[:-2].rstrip()
    return verses


def kjv_counts() -> list[list[int]]:
    source = (ROOT.parent / "crates/core/src/tables.rs").read_text(encoding="utf-8")
    start = source.index("KJV_CHAPTER_VERSE_COUNTS")
    start = source.index("= [", start) + 2
    end = source.index("];", start)
    return [
        [int(value) for value in block.split(",") if value.strip()]
        for block in re.findall(r"&\[([0-9,\s]+)\]", source[start:end], re.S)
    ]


# Rules in canon_vulg.h that do not fit this edition's actual verse splits.
DROP_RULES = {
    # This edition keeps the title inside Psalm 10:1, so no title verse is
    # merged there; without this rule Psalm 11 loses its last verse.
    (10, 1, 2, 11, 1, 0),
    (11, 0, 0, 12, 0, 0),
    (12, 1, 0, 13, 1, 0),
    (12, 2, 3, 13, 2, 0),
    # Psalm 145 has no separate title verse in this edition.
    (145, 1, 0, 146, 0, 0),
    (145, 2, 0, 146, 1, 2),
    # Superseded by the corrected 1-to-many rules below.
    (21, 36, 0, 21, 36, 37),
    (21, 37, 0, 21, 38, 39),
    (24, 1, 0, 23, 28, 29),
    (12, 33, 0, 12, 33, 34),
    (42, 16, 0, 42, 16, 17),
    (19, 40, 0, 19, 40, 41),
    # Mark already follows KJV numbering; only its last verse is compressed.
    (8, 39, 0, 9, 1, 0),
    (9, 1, 0, 9, 2, 0),
}

# Corrections for splits that canon_vulg.h does not describe for this edition.
# Tuples use the same shape as load_rules(): source chapter, first/last source
# verse, target chapter, first/last target verse.
EXTRA_RULES: dict[int, list[tuple[int, int, int, int, int, int]]] = {
    # Numbers: 30:1 is KJV 29:40 in this edition, so the rest shifts up.
    4: [(30, 2, 0, 30, 1, 0)],
    # Deuteronomy 33:27 contains KJV 33:27-28; 33:29 is absent.
    5: [(33, 27, 27, 33, 27, 28), (33, 29, 0, 33, 29, 0)],
    # Joshua 7:25 contains KJV 7:25-26.
    6: [
        (7, 25, 25, 7, 25, 26),
        (21, 36, 36, 21, 36, 37),
        (21, 37, 37, 21, 38, 39),
    ],
    # 1 Samuel 21:1 belongs to KJV 20:42; 21:2 starts KJV 21.
    9: [(21, 2, 0, 21, 1, 0), (24, 1, 0, 23, 29, 0)],
    # 1 Kings 22:43-44 are one KJV verse (22:43).
    11: [(22, 43, 44, 22, 43, 0)],
    # Nehemiah 12:33 contains KJV 12:33-34.
    16: [(12, 33, 33, 12, 33, 34)],
    # Job 42:16 contains KJV 42:16-17.
    20: [(42, 16, 16, 42, 16, 17)],
    # Psalms whose title/verse division differs from the standard Vulgate.
    21: [
        (10, 0, 0, 11, 0, 0),
        (11, 1, 2, 12, 1, 0),
        (12, 1, 2, 13, 1, 0),
        (12, 6, 6, 13, 5, 6),
        (14, 5, 6, 15, 5, 0),
        (66, 7, 7, 67, 6, 7),
        (103, 35, 36, 104, 35, 0),
        (145, 0, 0, 146, 0, 0),
    ],
    # Song 1:1 (KJV) is the book title, absent from the verse text.
    24: [(1, 1, 0, 1, 2, 0)],
    # Hosea 2:23-24 are one KJV verse (2:23).
    33: [(2, 23, 24, 2, 23, 0)],
    # Jonah 2:1 is KJV 1:17; 2:2 starts KJV 2; 2:9 contains KJV 2:8-9.
    37: [(2, 2, 0, 2, 1, 0), (2, 9, 9, 2, 8, 9)],
    # Mark 9:49 contains KJV 9:49-50.
    48: [(9, 49, 49, 9, 49, 50)],
    # Luke 3:37 contains KJV 3:37-38.
    49: [(3, 37, 37, 3, 37, 38)],
    # Acts 19:40 contains KJV 19:40-41.
    51: [(19, 40, 40, 19, 40, 41)],
    # 2 Corinthians 13:11 contains KJV 13:11-12; 13:12-13 are 13:13-14.
    54: [(13, 11, 11, 13, 11, 12)],
}

# Source verses that belong to another chapter in the KJV.
OVERRIDES: dict[tuple[str, int, int], list[tuple[str, int, int]]] = {
    ("Num", 30, 1): [("Num", 29, 40)],
    ("1Sam", 21, 1): [("1Sam", 20, 42)],
    ("1Sam", 24, 1): [("1Sam", 23, 29)],
    ("Jonah", 2, 1): [("Jonah", 1, 17)],
}

# Verses absent from the OSIS text but present in the KJV.
EXTRA_TEXTS: dict[tuple[str, int, int], str] = {
    ("Song", 1, 1): "Cantar de los cantares de Salomón.",
}


def adjusted_rules() -> dict[int, list[tuple[int, int, int, int, int, int]]]:
    rules = load_rules()
    for book, entries in rules.items():
        entries[:] = [entry for entry in entries if entry not in DROP_RULES]
    for book, entries in EXTRA_RULES.items():
        rules.setdefault(book, []).extend(entries)
    # The OSIS moves Vulgate Exodus 8:1-4 into Exodus 7:26-29.
    rules[2] = list(rules.get(2, [])) + [
        (7, 26, 29, 8, 1, 4),
        (8, 1, 28, 8, 5, 32),
    ]
    # map_verse() scans the rules in reverse, so keep them sorted by source
    # chapter and verse or the first "src_ch < chapter" early return wins.
    for entries in rules.values():
        entries.sort(key=lambda entry: (entry[0], entry[1], entry[2]))
    return rules


# Verses that one Vulgate verse merges but the KJV splits. The marker is the
# start of the second KJV verse inside the merged Spanish text; the text is
# divided there instead of duplicating the whole verse in both addresses.
MERGED_SPLITS: dict[tuple[str, int, int], str] = {
    ("Deut", 33, 27): "Israel habita",
    ("Josh", 7, 25): "levantaron sobre él",
    ("Josh", 21, 36): "Quedemot con su ejido",
    ("Josh", 21, 37): "Hesbón con su ejido",
    ("Neh", 12, 33): "Judá, Benjamín",
    ("Job", 42, 16): "Y murió Job",
    ("Ps", 12, 6): "cante yo a Yahvé",
    ("Ps", 66, 7): "¡Que Dios nos bendiga",
    ("Jonah", 2, 9): "Mas yo te ofreceré",
    ("Mark", 9, 49): "La sal es buena",
    ("Luke", 3, 37): "de Enós",
    ("Acts", 19, 40): "Dicho esto",
    ("2Cor", 13, 11): "Saludaos unos a otros",
}


def split_merged(
    text: str, source: tuple[str, int, int], targets: list[tuple[str, int, int]]
) -> list[str]:
    marker = MERGED_SPLITS.get(source)
    if marker is None or len(targets) != 2:
        return [text] * len(targets)
    first, second = targets
    if first[0] != second[0] or first[1] != second[1] or second[2] != first[2] + 1:
        return [text] * len(targets)
    before, found, after = text.partition(marker)
    if not found or not before.strip() or not after.strip():
        return [text] * len(targets)
    return [before.strip(), (found + after).strip()]


def main() -> int:
    rules = adjusted_rules()
    verses = extract_verses()
    print(f"extracted {len(verses)} OSIS verses", file=sys.stderr)

    counts = kjv_counts()
    kjv_index = {book: index for index, book in enumerate(KJV_BOOKS)}
    osis_counts: dict[tuple[str, int], int] = {}
    for book, chapter, _verse in verses:
        key = (book, chapter)
        osis_counts[key] = osis_counts.get(key, 0) + 1
    override_chapters = {(book, chapter) for book, chapter, _verse in OVERRIDES}

    mapped: dict[tuple[str, int, int], list[str]] = {}
    skipped_counts = 0
    for (book, chapter, verse), text in verses.items():
        if book in SKIP_BOOKS or book not in KJV_CODE:
            continue
        # Deuterocanonical inserts inside Daniel and Esther.
        if book == "Dan" and chapter in (13, 14):
            continue
        if book == "Dan" and chapter == 3 and 24 <= verse <= 90:
            continue
        if book == "Esth" and (chapter > 10 or (chapter == 10 and verse >= 4)):
            continue
        override = OVERRIDES.get((book, chapter, verse))
        if override is not None:
            targets = override
        else:
            book_counts = counts[kjv_index[book]]
            same_count = (
                1 <= chapter <= len(book_counts)
                and (book, chapter) not in override_chapters
                and osis_counts.get((book, chapter))
                == book_counts[chapter - 1]
            )
            if same_count and book not in ("Ps", "Dan"):
                skipped_counts += 1
                targets = [(book, chapter, verse)]
            else:
                targets = map_verse(rules, book, chapter, verse)
        bodies = split_merged(text, (book, chapter, verse), targets)
        for (target_book, target_chapter, target_verse), body in zip(targets, bodies):
            mapped.setdefault(
                (target_book, target_chapter, target_verse), []
            ).append(body)

    for key, text in EXTRA_TEXTS.items():
        mapped.setdefault(key, []).append(text)

    print(f"{skipped_counts} verses kept at their OSIS address", file=sys.stderr)
    missing: list[tuple[str, int, int]] = []
    seen: set[tuple[str, int, int]] = set()
    lines: list[str] = []
    for book_index, book in enumerate(KJV_BOOKS):
        for chapter, count in enumerate(counts[book_index], 1):
            for verse in range(1, count + 1):
                key = (book, chapter, verse)
                texts = mapped.get(key, [])
                if texts:
                    seen.add(key)
                    lines.append(f"{KJV_CODE[book]} {chapter}:{verse} {' '.join(texts)}")
                else:
                    missing.append(key)
                    lines.append(f"{KJV_CODE[book]} {chapter}:{verse} ")

    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    extras = sorted(set(mapped) - seen)
    print(
        f"wrote {OUT}: {len(lines)} verses, {len(missing)} missing, "
        f"{len(extras)} out-of-range targets",
        file=sys.stderr,
    )
    if missing:
        print("missing:", json.dumps(missing[:80]), file=sys.stderr)
    if extras:
        print("out-of-range:", json.dumps(extras[:40]), file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
