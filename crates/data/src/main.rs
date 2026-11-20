//! Data pipeline: raw OpenBible/Platense/ASV files -> compact binary datasets.
//!
//! Usage:
//!   bible-data xrefs <cross_references.txt> <xrefs.bin>
//!   bible-data vpl   <version_vpl.txt>      <version.bin>
//!   bible-data bsb   <bsb.txt>              <bsb.bin>   (optional TSV importer)
//!   bible-data all   <raw-dir>              <out-dir>

use std::collections::HashSet;
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use bible_core::format::{TranslationText, XrefData};
use bible_core::{parse_osis_id, tables, VerseRef, Versification, VERSE_COUNT};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, input, output] if command == "xrefs" => {
            build_xrefs(Path::new(input), Path::new(output))
        }
        [command, input, output] if command == "bsb" => {
            build_text(Path::new(input), Path::new(output))
        }
        [command, input, output] if command == "vpl" => {
            build_vpl(Path::new(input), Path::new(output))
        }
        [command, raw, output] if command == "all" => {
            let raw = Path::new(raw);
            let output = Path::new(output);
            fs::create_dir_all(output).map_err(|error| error.to_string())?;
            build_xrefs(
                &raw.join("cross_references.txt"),
                &output.join("xrefs.bin"),
            )?;
            build_vpl(&raw.join("spaplatense_vpl.txt"), &output.join("plat.bin"))?;
            build_vpl(&raw.join("eng-asv_vpl.txt"), &output.join("asv.bin"))?;
            Ok(())
        }
        [command, dir] if command == "verify" => verify(Path::new(dir)),
        _ => {
            eprintln!(
                "usage:\n  bible-data xrefs <cross_references.txt> <xrefs.bin>\n  \
                 bible-data bsb   <bsb.txt>              <bsb.bin>\n  \
                 bible-data vpl   <version_vpl.txt>      <version.bin>\n  \
                 bible-data all   <raw-dir>              <out-dir>\n  \
                 bible-data verify <out-dir>"
            );
            Ok(())
        }
    }
}

/// Validates the generated binaries against the invariants the app relies on.
fn verify(dir: &Path) -> Result<(), String> {
    let versification = Versification::kjv();
    let xref_bytes =
        fs::read(dir.join("xrefs.bin")).map_err(|error| format!("xrefs.bin: {error}"))?;
    let xrefs = XrefData::parse(&xref_bytes).map_err(|error| format!("xrefs.bin: {error}"))?;
    if xrefs.verse_count != VERSE_COUNT {
        return Err(format!(
            "xrefs: {} verses, expected {VERSE_COUNT}",
            xrefs.verse_count
        ));
    }
    if xrefs.offsets.first().copied() != Some(0)
        || xrefs.offsets.last().copied() != Some(xrefs.link_count() as u32)
    {
        return Err("xrefs: offsets do not partition the link columns".into());
    }
    let mut published = 0usize;
    let mut previous_offset = 0;
    for source in 0..xrefs.verse_count as usize {
        let start = xrefs.offsets[source] as usize;
        let end = xrefs.offsets[source + 1] as usize;
        if start < previous_offset || end < start {
            return Err(format!("xrefs: offsets are not monotonic at verse {source}"));
        }
        previous_offset = end;
        let mut previous_votes: Option<i16> = None;
        for link in start..end {
            let target_start = xrefs.target_start[link];
            let target_end = xrefs.target_end[link];
            if target_start > target_end || target_end >= VERSE_COUNT {
                return Err(format!("xrefs: bad target range at link {link}"));
            }
            if let Some(previous) = previous_votes {
                if xrefs.votes[link] > previous {
                    return Err(format!(
                        "xrefs: votes are not descending inside source verse {source}"
                    ));
                }
            }
            previous_votes = Some(xrefs.votes[link]);
            if xrefs.votes[link] >= 4 {
                published += 1;
            }
        }
    }
    if xrefs.link_count() != 344_799 {
        return Err(format!(
            "xrefs: {} links, expected 344799",
            xrefs.link_count()
        ));
    }
    if published != 153_419 {
        return Err(format!(
            "xrefs: {published} published links, expected 153419"
        ));
    }

    let index_of = |osis: &str| -> Result<u32, String> {
        let (book, chapter, verse) =
            parse_osis_id(osis).ok_or_else(|| format!("bad sample id {osis}"))?;
        versification
            .index_of(VerseRef {
                book,
                chapter,
                verse,
            })
            .ok_or_else(|| format!("sample {osis} is not in the versification"))
    };
    let sample = (index_of("Gen.1.1")?, index_of("Rev.3.14")?);
    let found = (xrefs.offsets[sample.0 as usize]..xrefs.offsets[sample.0 as usize + 1])
        .any(|link| {
            let link = link as usize;
            xrefs.target_start[link] == sample.1
                && xrefs.target_end[link] == sample.1
                && xrefs.votes[link] == 47
        });
    if !found {
        return Err("xrefs: Gen.1.1 -> Rev.3.14 (47 votes) is missing".into());
    }

    let john = index_of("John.3.16")?;
    let mut text_bytes = 0usize;
    for (file, expected_first, expected_john) in [
        ("plat.bin", "Al principio", "Porque así amó Dios al mundo"),
        ("asv.bin", "In the beginning", "For God so loved"),
    ] {
        let path = dir.join(file);
        let bytes = fs::read(&path).map_err(|error| format!("{file}: {error}"))?;
        let text = TranslationText::parse(&bytes).map_err(|error| format!("{file}: {error}"))?;
        if text.verse_count != VERSE_COUNT {
            return Err(format!(
                "{file}: {} verses, expected {VERSE_COUNT}",
                text.verse_count
            ));
        }
        let first = text.text(0).ok_or_else(|| format!("{file}: missing Genesis 1:1"))?;
        if !first.starts_with(expected_first) {
            return Err(format!("{file}: unexpected Genesis 1:1 {first:?}"));
        }
        let john_text = text.text(john).ok_or_else(|| format!("{file}: missing John 3:16"))?;
        if !john_text.contains(expected_john) {
            return Err(format!("{file}: unexpected John 3:16 {john_text:?}"));
        }
        text_bytes += text.blob.len();
    }

    println!(
        "ok: {} verses, {} links ({} published), {} bytes of text",
        xrefs.verse_count,
        xrefs.link_count(),
        published,
        text_bytes
    );
    Ok(())
}

fn read_text(path: &Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

fn resolve_osis(
    id: &str,
    versification: &Versification,
    remapped: &mut usize,
) -> Result<u32, String> {
    // The KJV versification folds 3 John 1:15 into 1:14.
    let id = if id == "3John.1.15" {
        *remapped += 1;
        "3John.1.14"
    } else {
        id
    };
    let (book, chapter, verse) =
        parse_osis_id(id).ok_or_else(|| format!("unknown OSIS verse id {id:?}"))?;
    versification
        .index_of(VerseRef {
            book,
            chapter,
            verse,
        })
        .ok_or_else(|| format!("verse {id:?} is not in the KJV versification"))
}

fn build_xrefs(input: &Path, output: &Path) -> Result<(), String> {
    let text = read_text(input)?;
    let mut lines = text.lines();
    let header = lines.next().ok_or("OpenBible: empty input")?;
    if !header.starts_with("From Verse\tTo Verse\tVotes") {
        return Err(format!("OpenBible: unexpected header {:?}", &header[..80.min(header.len())]));
    }

    let versification = Versification::kjv();
    let mut links: Vec<(u32, u32, u32, i16)> = Vec::with_capacity(344_799);
    let mut seen = HashSet::with_capacity(344_799);
    let mut source_rows = 0usize;
    let mut duplicates = 0usize;
    let mut remapped = 0usize;

    for (offset, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        source_rows += 1;
        let line_number = offset + 2;
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 3 {
            return Err(format!("OpenBible line {line_number}: malformed row {line:?}"));
        }
        let from = resolve_osis(fields[0], &versification, &mut remapped)
            .map_err(|error| format!("OpenBible line {line_number}: {error}"))?;
        let (start_id, end_id) = match fields[1].split_once('-') {
            Some((start, end)) => (start, end),
            None => (fields[1], fields[1]),
        };
        let target_start = resolve_osis(start_id, &versification, &mut remapped)
            .map_err(|error| format!("OpenBible line {line_number}: {error}"))?;
        let target_end = resolve_osis(end_id, &versification, &mut remapped)
            .map_err(|error| format!("OpenBible line {line_number}: {error}"))?;
        if target_start > target_end {
            return Err(format!(
                "OpenBible line {line_number}: reversed range {:?}",
                fields[1]
            ));
        }
        let votes: i32 = fields[2]
            .parse()
            .map_err(|_| format!("OpenBible line {line_number}: invalid votes {:?}", fields[2]))?;
        let votes = i16::try_from(votes)
            .map_err(|_| format!("OpenBible line {line_number}: votes out of range"))?;

        if !seen.insert((from, target_start, target_end)) {
            duplicates += 1;
            continue;
        }
        links.push((from, target_start, target_end, votes));
    }

    // Canonical order: source, votes descending, target.
    links.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.3.cmp(&a.3))
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });

    let mut offsets = vec![0u32; VERSE_COUNT as usize + 1];
    for (from, _, _, _) in &links {
        offsets[*from as usize + 1] += 1;
    }
    for index in 0..VERSE_COUNT as usize {
        offsets[index + 1] += offsets[index];
    }

    let data = XrefData {
        verse_count: VERSE_COUNT,
        offsets,
        target_start: links.iter().map(|link| link.1).collect(),
        target_end: links.iter().map(|link| link.2).collect(),
        votes: links.iter().map(|link| link.3).collect(),
    };
    write_bin(output, |writer| data.write(writer))?;

    let filtered = data.votes.iter().filter(|votes| **votes >= 4).count();
    println!(
        "xrefs: {source_rows} rows, {} links, {} with votes >= 4, {duplicates} duplicates, {remapped} remapped",
        data.link_count(),
        filtered
    );
    println!(
        "xrefs: wrote {} ({} bytes)",
        output.display(),
        fs::metadata(output).map(|m| m.len()).unwrap_or(0)
    );
    Ok(())
}

fn build_text(input: &Path, output: &Path) -> Result<(), String> {
    let text = read_text(input)?;
    let lines: Vec<&str> = text.lines().collect();
    let header_index = lines
        .iter()
        .position(|line| line.starts_with("Verse\t"))
        .ok_or("BSB: column header (\"Verse\\t…\") not found")?;

    let versification = Versification::kjv();
    let mut verses: Vec<Option<&str>> = vec![None; VERSE_COUNT as usize];
    let mut duplicates = 0usize;

    for (offset, line) in lines[header_index + 1..].iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_number = header_index + offset + 2;
        let (left, body) = line
            .split_once('\t')
            .ok_or_else(|| format!("BSB line {line_number}: missing tab separator"))?;
        let (book_name, chapter_verse) = left
            .rsplit_once(' ')
            .ok_or_else(|| format!("BSB line {line_number}: bad reference {left:?}"))?;
        let (chapter, verse) = chapter_verse
            .split_once(':')
            .ok_or_else(|| format!("BSB line {line_number}: bad reference {left:?}"))?;
        let book = bible_core::book_index_by_name(book_name)
            .ok_or_else(|| format!("BSB line {line_number}: unknown book {book_name:?}"))?;
        let chapter: u16 = chapter
            .parse()
            .map_err(|_| format!("BSB line {line_number}: bad chapter"))?;
        let verse: u16 = verse
            .parse()
            .map_err(|_| format!("BSB line {line_number}: bad verse"))?;
        let index = versification
            .index_of(VerseRef {
                book,
                chapter,
                verse,
            })
            .ok_or_else(|| format!("BSB line {line_number}: {book_name} {chapter}:{verse} is not in KJV"))?;
        if verses[index as usize].replace(body.trim()).is_some() {
            duplicates += 1;
        }
    }

    let missing = verses.iter().filter(|verse| verse.is_none()).count();
    if missing > 0 {
        return Err(format!("BSB: {missing} verses are missing"));
    }

    let mut blob = Vec::with_capacity(text.len());
    let mut offsets = Vec::with_capacity(VERSE_COUNT as usize + 1);
    offsets.push(0u32);
    for verse in verses {
        blob.extend_from_slice(verse.unwrap_or_default().as_bytes());
        offsets.push(blob.len() as u32);
    }

    let data = TranslationText {
        verse_count: VERSE_COUNT,
        offsets,
        blob,
    };
    write_bin(output, |writer| data.write(writer))?;
    println!(
        "bsb: {} verses, {} bytes of text, {duplicates} duplicates; wrote {} ({} bytes)",
        data.verse_count,
        data.blob.len(),
        output.display(),
        fs::metadata(output).map(|m| m.len()).unwrap_or(0)
    );
    Ok(())
}

/// eBible.org verse-per-line editions: `<CODE> <chapter>:<verse> <text>`.
fn build_vpl(input: &Path, output: &Path) -> Result<(), String> {
    let text = read_text(input)?;
    let versification = Versification::kjv();
    let mut verses: Vec<&str> = vec![""; VERSE_COUNT as usize];
    let mut seen = vec![false; VERSE_COUNT as usize];
    let mut duplicates = 0usize;

    for (offset, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_number = offset + 1;
        let (code, rest) = line
            .split_once(' ')
            .ok_or_else(|| format!("VPL line {line_number}: missing book code"))?;
        let book = bible_core::book_index_by_code(code)
            .ok_or_else(|| format!("VPL line {line_number}: unknown code {code:?}"))?;
        let (address, body) = rest
            .split_once(' ')
            .ok_or_else(|| format!("VPL line {line_number}: missing verse text"))?;
        let (chapter, verse) = address
            .split_once(':')
            .ok_or_else(|| format!("VPL line {line_number}: bad address {address:?}"))?;
        let chapter: u16 = chapter
            .parse()
            .map_err(|_| format!("VPL line {line_number}: bad chapter"))?;
        let verse: u16 = verse
            .parse()
            .map_err(|_| format!("VPL line {line_number}: bad verse"))?;
        let index = versification
            .index_of(VerseRef {
                book,
                chapter,
                verse,
            })
            .ok_or_else(|| {
                format!("VPL line {line_number}: {code} {chapter}:{verse} is not in KJV")
            })?;
        if seen[index as usize] {
            duplicates += 1;
        }
        seen[index as usize] = true;
        verses[index as usize] = body.trim();
    }

    let missing = seen.iter().filter(|present| !**present).count();
    let mut blob = Vec::with_capacity(text.len());
    let mut offsets = Vec::with_capacity(VERSE_COUNT as usize + 1);
    offsets.push(0u32);
    for verse in verses {
        blob.extend_from_slice(verse.as_bytes());
        offsets.push(blob.len() as u32);
    }
    let data = TranslationText {
        verse_count: VERSE_COUNT,
        offsets,
        blob,
    };
    write_bin(output, |writer| data.write(writer))?;
    println!(
        "vpl: {} verses ({missing} missing, {duplicates} duplicates), {} bytes of text; wrote {} ({} bytes)",
        data.verse_count,
        data.blob.len(),
        output.display(),
        fs::metadata(output).map(|m| m.len()).unwrap_or(0)
    );
    Ok(())
}

fn write_bin<F>(path: &Path, write: F) -> Result<(), String>
where
    F: FnOnce(&mut BufWriter<File>) -> std::io::Result<()>,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let file = File::create(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut writer = BufWriter::new(file);
    write(&mut writer).map_err(|error| format!("{}: {error}", path.display()))?;
    writer.flush().map_err(|error| error.to_string())
}

/// Kept so the generator can be validated against the canonical book count.
#[allow(dead_code)]
fn assert_canonical_books() {
    assert_eq!(tables::BOOK_IDS.len(), 66);
}
