//! Canonical domain model shared by the data pipeline and the web app.
//!
//! Coordinates are dense, zero-based verse indices in KJV order (31,102 verses),
//! which keeps the data pipeline and the renderer addressing the same verses.

pub mod tables;
pub mod tables_es;
pub mod words;

pub const VERSE_COUNT: u32 = 31_102;
pub const CHAPTER_COUNT: u32 = 1_189;

/// Index of an OSIS book id (`Gen`, `1Cor`, ...) in canonical order.
pub fn book_index(id: &str) -> Option<usize> {
    tables::BOOK_IDS.iter().position(|candidate| *candidate == id)
}

/// Folds a user query to the form the alias tables use: lowercase, no
/// accents, no spaces and no dots. `Génesis 1:1` and `genesis 1.1` both fold
/// to `genesis1:1`.
pub fn normalize_query(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.to_lowercase().chars() {
        let folded = match ch {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            ' ' | '.' | ',' => continue,
            other => other,
        };
        out.push(folded);
    }
    out
}

/// Resolves the book named at the start of a normalized query, preferring the
/// longest alias so `1juan` beats `juan`.
pub fn book_from_query(query: &str) -> Option<(usize, &str)> {
    let mut best: Option<(usize, &str, usize)> = None;
    for (book, aliases) in tables_es::BOOK_ALIASES_ES.iter().enumerate() {
        for alias in *aliases {
            let alias = normalize_query(alias);
            if !alias.is_empty() && query.starts_with(&alias) {
                let rest = &query[alias.len()..];
                if best.is_none_or(|(_, _, length)| alias.len() > length) {
                    best = Some((book, rest, alias.len()));
                }
            }
        }
    }
    best.map(|(book, rest, _)| (book, rest))
}

pub fn book_name_es(book: usize) -> &'static str {
    tables_es::BOOK_NAMES_ES[book]
}

pub fn book_ref_name_es(book: usize) -> &'static str {
    tables_es::BOOK_REF_NAMES_ES[book]
}

pub fn book_abbr_es(book: usize) -> &'static str {
    tables_es::BOOK_ABBR_ES[book]
}

pub fn genre_name_es(genre: usize) -> &'static str {
    tables_es::GENRE_NAMES_ES[genre.min(tables_es::GENRE_NAMES_ES.len() - 1)]
}

/// eBible.org/BibleWorks book codes in canonical order (`GEN`, `1SA`, ...).
pub const BOOK_CODES: [&str; 66] = [
    "GEN", "EXO", "LEV", "NUM", "DEU", "JOS", "JDG", "RUT", "1SA", "2SA", "1KI", "2KI", "1CH",
    "2CH", "EZR", "NEH", "EST", "JOB", "PSA", "PRO", "ECC", "SOL", "ISA", "JER", "LAM", "EZE",
    "DAN", "HOS", "JOE", "AMO", "OBA", "JON", "MIC", "NAH", "HAB", "ZEP", "HAG", "ZEC", "MAL",
    "MAT", "MAR", "LUK", "JOH", "ACT", "ROM", "1CO", "2CO", "GAL", "EPH", "PHI", "COL", "1TH",
    "2TH", "1TI", "2TI", "TIT", "PHM", "HEB", "JAM", "1PE", "2PE", "1JO", "2JO", "3JO", "JUD",
    "REV",
];

/// Index of an eBible book code (`GEN`, `1CO`, ...).
pub fn book_index_by_code(code: &str) -> Option<usize> {
    BOOK_CODES.iter().position(|candidate| *candidate == code)
}

/// Index of a full English book name (`Genesis`, `Song of Solomon`, ...).
pub fn book_index_by_name(name: &str) -> Option<usize> {
    if name == "Psalm" {
        return book_index("Ps");
    }
    tables::BOOK_NAMES
        .iter()
        .position(|candidate| *candidate == name)
}

/// Parses an OSIS verse id such as `Gen.1.1` or `1Cor.13.4`.
pub fn parse_osis_id(id: &str) -> Option<(usize, u16, u16)> {
    let mut parts = id.rsplitn(3, '.');
    let verse = parts.next()?.parse().ok()?;
    let chapter = parts.next()?.parse().ok()?;
    let book = book_index(parts.next()?)?;
    Some((book, chapter, verse))
}

/// Formats an OSIS-style reference in Spanish: `Juan 3:16`,
/// `Romanos 1:19-20`, `Salmos 23`.
pub fn format_reference(book: usize, chapter: u16, start: u16, end: Option<u16>) -> String {
    let name = book_ref_name_es(book);
    match end {
        Some(end) if end != start => format!("{name} {chapter}:{start}-{end}"),
        _ => format!("{name} {chapter}:{start}"),
    }
}

/// Formats a dense verse index as a Spanish reference.
pub fn format_verse(versification: &Versification, index: u32) -> String {
    match versification.ref_at(index) {
        Some(reference) => format_reference(reference.book, reference.chapter, reference.verse, None),
        None => String::from("—"),
    }
}

/// A verse in human coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerseRef {
    pub book: usize,
    pub chapter: u16,
    pub verse: u16,
}

impl VerseRef {
    pub fn osis_id(self) -> String {
        format!(
            "{}.{}.{}",
            tables::BOOK_IDS[self.book], self.chapter, self.verse
        )
    }
}

/// The KJV versification: books, chapters and verses.
pub struct Versification {
    /// Start index of every book, plus a final sentinel.
    pub book_starts: Vec<u32>,
    /// Verse index at which each chapter starts.
    pub chapter_starts: Vec<Vec<u32>>,
    pub verse_book: Vec<u8>,
    pub verse_chapter: Vec<u8>,
    pub verse_number: Vec<u8>,
}

impl Default for Versification {
    fn default() -> Self {
        Self::kjv()
    }
}

impl Versification {
    pub fn kjv() -> Self {
        let mut book_starts = Vec::with_capacity(tables::BOOK_IDS.len() + 1);
        let mut chapter_starts = Vec::with_capacity(tables::BOOK_IDS.len());
        let mut verse_book = Vec::with_capacity(VERSE_COUNT as usize);
        let mut verse_chapter = Vec::with_capacity(VERSE_COUNT as usize);
        let mut verse_number = Vec::with_capacity(VERSE_COUNT as usize);

        let mut index = 0u32;
        for (book, counts) in tables::KJV_CHAPTER_VERSE_COUNTS.iter().enumerate() {
            book_starts.push(index);
            let mut starts = Vec::with_capacity(counts.len());
            for (chapter_offset, count) in counts.iter().copied().enumerate() {
                starts.push(index);
                for verse in 1..=count {
                    verse_book.push(book as u8);
                    verse_chapter.push((chapter_offset + 1) as u8);
                    verse_number.push(verse as u8);
                }
                index += u32::from(count);
            }
            chapter_starts.push(starts);
        }
        book_starts.push(index);
        assert_eq!(index, VERSE_COUNT);

        Self {
            book_starts,
            chapter_starts,
            verse_book,
            verse_chapter,
            verse_number,
        }
    }

    pub fn verse_count(&self) -> u32 {
        VERSE_COUNT
    }

    pub fn chapter_count(&self) -> u32 {
        CHAPTER_COUNT
    }

    /// Dense index of a verse, or `None` when it does not exist.
    pub fn index_of(&self, r: VerseRef) -> Option<u32> {
        let counts = *tables::KJV_CHAPTER_VERSE_COUNTS.get(r.book)?;
        if r.chapter == 0 || r.verse == 0 || usize::from(r.chapter) > counts.len() {
            return None;
        }
        let chapter_start = *self.chapter_starts[r.book].get(usize::from(r.chapter) - 1)?;
        if r.verse > counts[usize::from(r.chapter) - 1] {
            return None;
        }
        Some(chapter_start + u32::from(r.verse) - 1)
    }

    /// Inclusive index range of a chapter.
    pub fn chapter_range(&self, book: usize, chapter: u16) -> Option<(u32, u32)> {
        let counts = *tables::KJV_CHAPTER_VERSE_COUNTS.get(book)?;
        let start = *self.chapter_starts[book].get(usize::from(chapter).checked_sub(1)?)?;
        let count = *counts.get(usize::from(chapter).checked_sub(1)?)?;
        Some((start, start + u32::from(count) - 1))
    }

    /// Human coordinates of a dense verse index.
    pub fn ref_at(&self, index: u32) -> Option<VerseRef> {
        let index = usize::try_from(index).ok()?;
        Some(VerseRef {
            book: usize::from(*self.verse_book.get(index)?),
            chapter: u16::from(*self.verse_chapter.get(index)?),
            verse: u16::from(*self.verse_number.get(index)?),
        })
    }

    /// Inclusive verse range of a book.
    pub fn book_range(&self, book: usize) -> Option<(u32, u32)> {
        let start = *self.book_starts.get(book)?;
        let end = *self.book_starts.get(book + 1)?;
        Some((start, end.saturating_sub(1)))
    }

    /// Chapter count of a book.
    pub fn chapters_of(&self, book: usize) -> usize {
        self.chapter_starts.get(book).map_or(0, |chapters| chapters.len())
    }

    /// Genre ordinal of the book containing a verse.
    pub fn genre_of_verse(&self, index: u32) -> u8 {
        let book = self.verse_book[index as usize] as usize;
        tables::BOOK_GENRES[book]
    }

    /// Midtone brightness: well-voted links glow, disputed ones stay faint.
    pub fn vote_weight(votes: i16) -> f32 {
        let votes = f32::from(votes.max(0));
        (0.2 + 0.8 * (1.0 + votes).log10() / 2.0).min(1.0)
    }
}

/// Binary dataset formats.
pub mod format {
    use std::io::{self, Write};

    pub const XREF_MAGIC: [u8; 4] = *b"XRF1";
    pub const XREF_HEADER_LEN: usize = 20;
    pub const TEXT_MAGIC: [u8; 4] = *b"TXT1";
    pub const TEXT_HEADER_LEN: usize = 16;

    /// Columnar cross-reference dataset (CSR), sorted by source verse and,
    /// within a source, by descending votes.
    #[derive(Debug, Clone, Default)]
    pub struct XrefData {
        pub verse_count: u32,
        /// `verse_count + 1` offsets into the link columns.
        pub offsets: Vec<u32>,
        pub target_start: Vec<u32>,
        pub target_end: Vec<u32>,
        pub votes: Vec<i16>,
    }

    impl XrefData {
        pub fn link_count(&self) -> usize {
            self.target_start.len()
        }

        pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
            let links = self.link_count() as u32;
            w.write_all(&XREF_MAGIC)?;
            w.write_all(&1u16.to_le_bytes())?;
            w.write_all(&0u16.to_le_bytes())?;
            w.write_all(&self.verse_count.to_le_bytes())?;
            w.write_all(&links.to_le_bytes())?;
            w.write_all(&(self.offsets.len() as u32).to_le_bytes())?;
            for value in &self.offsets {
                w.write_all(&value.to_le_bytes())?;
            }
            for value in &self.target_start {
                w.write_all(&value.to_le_bytes())?;
            }
            for value in &self.target_end {
                w.write_all(&value.to_le_bytes())?;
            }
            for value in &self.votes {
                w.write_all(&value.to_le_bytes())?;
            }
            Ok(())
        }

        pub fn parse(bytes: &[u8]) -> Result<Self, String> {
            if bytes.len() < XREF_HEADER_LEN {
                return Err("xrefs: file too short".into());
            }
            if bytes[0..4] != XREF_MAGIC {
                return Err("xrefs: bad magic (expected XRF1)".into());
            }
            let version = u16::from_le_bytes([bytes[4], bytes[5]]);
            if version != 1 {
                return Err(format!("xrefs: unsupported version {version}"));
            }
            let verse_count = read_u32(bytes, 8)?;
            let link_count = read_u32(bytes, 12)? as usize;
            let offset_count = read_u32(bytes, 16)? as usize;
            if offset_count != verse_count as usize + 1 {
                return Err(format!(
                    "xrefs: offset count {offset_count} does not match {verse_count} verses"
                ));
            }
            let offsets_len = offset_count * 4;
            let starts_len = link_count * 4;
            let votes_len = link_count * 2;
            let expected = XREF_HEADER_LEN + offsets_len + starts_len + starts_len + votes_len;
            if bytes.len() != expected {
                return Err(format!(
                    "xrefs: expected {expected} bytes, got {}",
                    bytes.len()
                ));
            }

            let mut cursor = XREF_HEADER_LEN;
            let offsets = read_u32_vec(bytes, &mut cursor, offset_count)?;
            let target_start = read_u32_vec(bytes, &mut cursor, link_count)?;
            let target_end = read_u32_vec(bytes, &mut cursor, link_count)?;
            let mut votes = Vec::with_capacity(link_count);
            for _ in 0..link_count {
                votes.push(i16::from_le_bytes([
                    bytes[cursor],
                    bytes[cursor + 1],
                ]));
                cursor += 2;
            }

            if offsets.first().copied() != Some(0)
                || offsets.last().copied() != Some(link_count as u32)
            {
                return Err("xrefs: CSR offsets are not a partition of the link columns".into());
            }
            Ok(Self {
                verse_count,
                offsets,
                target_start,
                target_end,
                votes,
            })
        }
    }

    /// Verse texts in canonical order plus a UTF-8 blob.
    #[derive(Debug, Clone, Default)]
    pub struct TranslationText {
        pub verse_count: u32,
        /// `verse_count + 1` byte offsets into `blob`.
        pub offsets: Vec<u32>,
        pub blob: Vec<u8>,
    }

    impl TranslationText {
        pub fn text(&self, index: u32) -> Option<&str> {
            let start = *self.offsets.get(index as usize)? as usize;
            let end = *self.offsets.get(index as usize + 1)? as usize;
            std::str::from_utf8(self.blob.get(start..end)?).ok()
        }

        pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
            w.write_all(&TEXT_MAGIC)?;
            w.write_all(&1u16.to_le_bytes())?;
            w.write_all(&0u16.to_le_bytes())?;
            w.write_all(&self.verse_count.to_le_bytes())?;
            w.write_all(&(self.blob.len() as u32).to_le_bytes())?;
            for value in &self.offsets {
                w.write_all(&value.to_le_bytes())?;
            }
            w.write_all(&self.blob)?;
            Ok(())
        }

        pub fn parse(bytes: &[u8]) -> Result<Self, String> {
            if bytes.len() < TEXT_HEADER_LEN {
                return Err("text: file too short".into());
            }
            if bytes[0..4] != TEXT_MAGIC && bytes[0..4] != *b"BSB1" {
                return Err("text: bad magic (expected TXT1)".into());
            }
            if u16::from_le_bytes([bytes[4], bytes[5]]) != 1 {
                return Err("text: unsupported version".into());
            }
            let verse_count = read_u32(bytes, 8)?;
            let blob_len = read_u32(bytes, 12)? as usize;
            let offset_count = verse_count as usize + 1;
            let offsets_len = offset_count * 4;
            let expected = TEXT_HEADER_LEN + offsets_len + blob_len;
            if bytes.len() != expected {
                return Err(format!(
                    "text: expected {expected} bytes, got {}",
                    bytes.len()
                ));
            }
            let mut cursor = TEXT_HEADER_LEN;
            let offsets = read_u32_vec(bytes, &mut cursor, offset_count)?;
            let blob = bytes[cursor..].to_vec();
            if offsets.first().copied() != Some(0)
                || offsets.last().copied() != Some(blob_len as u32)
            {
                return Err("text: offsets do not cover the blob".into());
            }
            Ok(Self {
                verse_count,
                offsets,
                blob,
            })
        }
    }

    fn read_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
        let slice = bytes
            .get(at..at + 4)
            .ok_or_else(|| "unexpected end of file".to_string())?;
        Ok(u32::from_le_bytes([
            slice[0], slice[1], slice[2], slice[3],
        ]))
    }

    fn read_u32_vec(bytes: &[u8], cursor: &mut usize, count: usize) -> Result<Vec<u32>, String> {
        let end = cursor
            .checked_add(count * 4)
            .ok_or_else(|| "length overflow".to_string())?;
        let slice = bytes
            .get(*cursor..end)
            .ok_or_else(|| "unexpected end of file".to_string())?;
        let mut out = Vec::with_capacity(count);
        for chunk in slice.chunks_exact(4) {
            out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        *cursor = end;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versification_round_trips() {
        let v = Versification::kjv();
        assert_eq!(v.verse_count(), 31_102);
        assert_eq!(v.chapter_count(), 1_189);
        let john_3_16 = VerseRef {
            book: book_index("John").unwrap(),
            chapter: 3,
            verse: 16,
        };
        let index = v.index_of(john_3_16).unwrap();
        assert_eq!(v.ref_at(index), Some(john_3_16));
    }

    #[test]
    fn osis_ids_parse() {
        assert_eq!(parse_osis_id("Gen.1.1"), Some((0, 1, 1)));
        assert_eq!(
            parse_osis_id("1Cor.13.4").map(|(b, _, _)| tables::BOOK_IDS[b]),
            Some("1Cor")
        );
        assert!(parse_osis_id("Nope.1.1").is_none());
    }
}
