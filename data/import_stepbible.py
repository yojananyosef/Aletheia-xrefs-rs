#!/usr/bin/env python3
"""Build the original-language layer from STEPBible Data (CC BY 4.0).

Inputs (download into data/raw/stepbible/):
  TAHOT Gen-Deu / Jos-Est / Job-Sng / Isa-Mal  -> Hebrew/Aramaic words
  TAGNT Mat-Jhn / Act-Rev                      -> Greek words
  TBESH / TBESG                                -> brief lexicons

Optional Spanish glosses:
  data/raw/spanish/spaRV1909_usfx.xml          -> Reina-Valera 1909 (USFX)
  pip install simplemma                        -> Spanish verb lemmas

Output:
  crates/app/data/words.bin.gz  (WRD1 v2: verse-indexed words + lexicon)

The format is documented in crates/core/src/words.rs. Run from the repository
root, then rebuild the app with Trunk.
"""
from __future__ import annotations

import gzip
import html
import re
import struct
import sys
import unicodedata
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

try:
    import simplemma
except ImportError:  # Optional: the pipeline still works with surface forms.
    simplemma = None

ROOT = Path(__file__).resolve().parent.parent
RAW = ROOT / "data" / "raw" / "stepbible"
SPANISH = ROOT / "data" / "raw" / "spanish"
RV1909 = SPANISH / "spaRV1909_usfx.xml"
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

# USFX book ids in canonical order (eBible's spaRV1909).
USFX_IDS = [
    "GEN", "EXO", "LEV", "NUM", "DEU", "JOS", "JDG", "RUT", "1SA", "2SA",
    "1KI", "2KI", "1CH", "2CH", "EZR", "NEH", "EST", "JOB", "PSA", "PRO",
    "ECC", "SNG", "ISA", "JER", "LAM", "EZK", "DAN", "HOS", "JOL", "AMO",
    "OBA", "JON", "MIC", "NAM", "HAB", "ZEP", "HAG", "ZEC", "MAL", "MAT",
    "MRK", "LUK", "JHN", "ACT", "ROM", "1CO", "2CO", "GAL", "EPH", "PHP",
    "COL", "1TH", "2TH", "1TI", "2TI", "TIT", "PHM", "HEB", "JAS", "1PE",
    "2PE", "1JN", "2JN", "3JN", "JUD", "REV",
]

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


def source_files(prefix: str) -> list[Path]:
    """Accept both the short names and the long STEPBible download names."""
    return sorted(
        path
        for path in RAW.iterdir()
        if path.is_file()
        and path.suffix.lower() == ".txt"
        and path.name.lower().startswith(prefix.lower())
    )


def load_lexicon() -> dict[str, dict[str, str]]:
    entries: dict[str, dict[str, str]] = {}
    paths = source_files("tbesh") + source_files("tbesg")
    if not paths:
        raise SystemExit(f"missing TBESH/TBESG in {RAW}")
    for path in paths:
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


SPANISH_STOP = {
    "y", "e", "o", "u", "de", "del", "a", "al", "á", "en", "con", "por",
    "para", "que", "el", "la", "los", "las", "un", "una", "unos", "unas",
    "lo", "se", "su", "sus", "mi", "mis", "tu", "tus", "le", "les", "me",
    "nos", "os", "te", "como", "sin", "sobre", "entre", "hasta", "desde",
    "ni", "pero", "mas", "sino", "oh", "he", "entonces", "así", "luego",
    "después", "también", "porque", "cuando", "si", "no", "sí",
}
SPANISH_BAD_PHRASE_START = {
    "él", "ella", "ellos", "ellas", "yo", "tú", "nosotros", "vosotros",
    "este", "esta", "estos", "estas", "ese", "esa", "esos", "esas",
    "aquel", "aquella", "aquellos", "aquellas", "mi", "mis", "tu", "tus",
    "su", "sus", "nuestro", "nuestra", "vuestro", "vuestra", "entonces",
    "así", "luego", "después", "también", "porque", "cuando", "si",
}
SPANISH_AUX = {
    "está", "estaba", "estaban", "estando", "estén", "esté", "estar",
    "estuvo", "habiendo", "había", "habían", "ha", "han", "he", "has",
    "hemos", "habéis", "fue", "fué", "fueron", "era", "eran", "es", "son",
    "ser", "sido", "sea", "sean", "tiene", "tienen", "tenía", "tenían",
}
SPANISH_SPECIAL = {
    # The direct-object marker is normally left untranslated. RV1909 tags
    # occasionally use it for pronouns, which would produce misleading glosses.
    "H0853": ("objeto directo", "objeto directo"),
    "G3588": ("el, la, los, las", "el · la · los · las"),
}
SPANISH_VERB_ENDING = re.compile(
    r"(o|as|a|amos|áis|an|e|es|emos|éis|en|é|aste|ó|aron|aba|abas|aban|"
    r"ía|ías|ían|aré|arás|ará|aremos|aréis|arán|aría|arías|arían|"
    r"ando|iendo|yendo|ado|ada|ados|adas|ido|ida|idos|idas)$"
)


def spanish_fold(text: str) -> str:
    return "".join(
        char
        for char in unicodedata.normalize("NFD", text.lower())
        if unicodedata.category(char) != "Mn"
    )


def spanish_clean(text: str) -> str:
    text = html.unescape(text)
    text = re.sub(r"<[^>]+>", "", text)
    text = re.sub(r"\s+", " ", text).strip()
    return text.strip('.,;:¡!¿?"«»“”‘’()[]')


def spanish_normalize(text: str) -> str:
    text = spanish_clean(text).lower()
    tokens = text.split()
    if not tokens:
        return ""
    if len(tokens) > 1:
        while len(tokens) > 1 and tokens[0] in SPANISH_STOP:
            tokens = tokens[1:]
        while len(tokens) > 1 and tokens[-1] in SPANISH_STOP:
            tokens = tokens[:-1]
    # "estando amando", "habiendo amado" -> the lexical verb.
    if len(tokens) >= 2 and tokens[0] in SPANISH_AUX:
        tail = tokens[-1]
        if re.search(r"(ando|iendo|yendo|ado|ada|ados|adas|ido|ida|idos|idas)$", tail):
            return lemmatize_spanish(tail)
    if len(tokens) == 1:
        return lemmatize_spanish(tokens[0])
    return " ".join(tokens)


def lemmatize_spanish(word: str) -> str:
    if simplemma is None:
        return word
    return simplemma.lemmatize(word, lang="es")


def spanish_display(norm: str, surfaces: Counter[str]) -> str:
    for surface, _ in surfaces.most_common(8):
        clean = spanish_clean(surface)
        if len(clean.split()) == 1 and spanish_fold(clean) == spanish_fold(norm):
            return clean
    return norm


def spanish_is_inflection(candidate: str, top: str) -> bool:
    if top.endswith(("ar", "er", "ir")) and candidate != top:
        stem = top[:-2]
        if candidate.startswith(stem) and SPANISH_VERB_ENDING.search(candidate):
            return True
    return False


def spanish_contains(candidate: str, top: str) -> bool:
    folded_candidate = spanish_fold(candidate)
    folded_top = spanish_fold(top)
    if len(folded_top) < 2:
        return False
    return (
        re.search(r"(^| )" + re.escape(folded_top) + r"( |$)", folded_candidate)
        is not None
    )


def spanish_senses(
    data: dict[str, dict[str, object]],
) -> tuple[str, str]:
    ranked = sorted(
        data.items(),
        key=lambda item: (-item[1]["n"], item[1]["first"], len(item[0])),
    )
    if not ranked:
        return "", ""
    top_norm, top_data = ranked[0]
    top = spanish_display(top_norm, top_data["surfaces"])
    selected = [top]
    for norm, entry in ranked[1:]:
        if len(selected) >= 3:
            break
        display = spanish_display(norm, entry["surfaces"])
        tokens = norm.split()
        if spanish_contains(display, top):
            continue
        if spanish_is_inflection(display, top):
            continue
        if len(tokens) > 1 and tokens[0] in SPANISH_BAD_PHRASE_START:
            continue
        if len(tokens) > 1 and tokens[0] in SPANISH_AUX:
            continue
        if (
            len(tokens) == 1
            and spanish_fold(display) in SPANISH_BAD_PHRASE_START
            and spanish_fold(top) not in SPANISH_BAD_PHRASE_START
        ):
            continue
        # A second proper name next to a proper name is usually a compound
        # ("Jehová Dios"), not a separate sense.
        if (
            top[:1].isupper()
            and display[:1].isupper()
            and len(display.split()) == 1
        ):
            continue
        # A common noun next to a proper name is usually alignment noise
        # ("Dios · cielo"), not a second sense of the name.
        if top[:1].isupper() and len(tokens) == 1 and not display[:1].isupper():
            continue
        if entry["n"] < 2 and top_data["n"] > 10:
            continue
        selected.append(display)
    return top, " · ".join(selected)


def load_spanish_glosses(
    verses: list[list[Word]],
) -> dict[str, tuple[str, str]]:
    """Align the public-domain Reina-Valera 1909 to the tagged Hebrew/Greek.

    Each RV1909 `<w s="H7225">` carries a Strong number and the Spanish phrase
    chosen by the translators. Matching those tags to the TAHOT/TAGNT word
    order gives a Spanish gloss per disambiguated Strong key and a short list
    of usage senses. It is not a translation of STEPBible's full definitions;
    those remain available in English.
    """
    if not RV1909.exists():
        print(
            f"note: {RV1909} not found; Spanish glosses will be empty "
            "(download eBible's spaRV1909 USFX)",
            file=sys.stderr,
        )
        return {}
    if simplemma is None:
        print(
            "note: install simplemma for Spanish verb lemmas "
            "(pip install simplemma); using surface forms",
            file=sys.stderr,
        )

    text = RV1909.read_text(encoding="utf-8")
    books = list(re.finditer(r'<book id="([A-Z0-9]+)">', text))
    rv: dict[tuple[int, int, int], list[tuple[list[str], str]]] = defaultdict(list)
    for index, match in enumerate(books):
        book_id = match.group(1)
        if book_id not in USFX_IDS:
            continue
        book = USFX_IDS.index(book_id)
        start = match.end()
        end = books[index + 1].start() if index + 1 < len(books) else len(text)
        body = text[start:end]
        chapter = 0
        verse = 0
        for token in re.finditer(
            r'<c id="(\d+)"\s*/>|<v id="(\d+)"\s*/>|<w s="([^"]+)">(.*?)</w>',
            body,
            re.S,
        ):
            if token.group(1) is not None:
                chapter = int(token.group(1))
                verse = 0
            elif token.group(2) is not None:
                verse = int(token.group(2))
            else:
                keys = re.findall(r"([GH]\d+)", token.group(3))
                rv[(book, chapter, verse)].append((keys, token.group(4)))

    full: dict[str, dict[str, dict[str, object]]] = defaultdict(
        lambda: defaultdict(lambda: {"n": 0, "first": 10**9, "surfaces": Counter()})
    )
    for (book, chapter, verse), tags in rv.items():
        # Reuse the canonical KJV coordinates rather than constructing a ref
        # string for every verse.
        if (
            chapter < 1
            or chapter > len(COUNTS[book])
            or verse < 1
            or verse > COUNTS[book][chapter - 1]
        ):
            continue
        index = BOOK_STARTS[book] + sum(COUNTS[book][: chapter - 1]) + verse - 1
        words = verses[index]
        cursor = 0
        for keys, raw_text in tags:
            if len(keys) != 1:
                continue
            found = None
            for candidate in range(cursor, len(words)):
                match = re.match(r"([GH]\d+)", words[candidate].strong)
                if match and match.group(1) == keys[0]:
                    found = candidate
                    break
            if found is None:
                continue
            norm = spanish_normalize(raw_text)
            if not norm or len(norm) > 60:
                continue
            surface = spanish_clean(raw_text)
            entry = full[words[found].strong][norm]
            entry["n"] += 1
            entry["first"] = min(entry["first"], index)
            entry["surfaces"][surface] += 1
            cursor = found + 1

    all_keys = {word.strong for words in verses for word in words}
    result: dict[str, tuple[str, str]] = {}
    for key in sorted(all_keys):
        if key in SPANISH_SPECIAL:
            result[key] = SPANISH_SPECIAL[key]
            continue
        data = full.get(key)
        if data:
            result[key] = spanish_senses(data)
    covered = sum(1 for gloss, _ in result.values() if gloss)
    print(
        f"Spanish glosses: {covered} of {len(all_keys)} keys "
        f"({len(full)} full-key alignments)"
    )
    return result


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

    tahot_files = source_files("tahot")
    if not tahot_files:
        raise SystemExit(f"missing TAHOT files in {RAW}")
    for path in tahot_files:
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

    tagnt_files = source_files("tagnt")
    if not tagnt_files:
        raise SystemExit(f"missing TAGNT files in {RAW}")
    for path in tagnt_files:
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
    spanish: dict[str, tuple[str, str]],
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
        es_gloss, es_senses = spanish.get(key, ("", ""))
        values = [
            key,
            entry["lemma"],
            entry["translit"],
            entry["morph"],
            entry["gloss"],
            entry["meaning"],
            es_gloss,
            es_senses,
        ]
        parts: list[int] = []
        for value in values:
            offset, length = add_text(value)
            parts.extend((offset, length))
        lex_records.extend(struct.pack("<16I", *parts))

    header = struct.pack(
        "<4sHHIIIIII",
        b"WRD1",
        2,
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
    spanish = load_spanish_glosses(verses)
    raw = build_binary(verses, used, morph_codes, lexicon, spanish)
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
