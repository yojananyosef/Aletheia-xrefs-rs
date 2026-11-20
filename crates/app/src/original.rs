//! Shared UI for the original-language layer and its Spanish glosses.
//!
//! The heavy `words.bin.gz` file is loaded once per page (Arcs or Threads) and
//! cached in a thread-local. Both views use the same lexicon card so a word
//! behaves the same wherever it is clicked.

use std::cell::RefCell;
use std::io::Read;
use std::rc::Rc;

use bible_core::words::WordsData;

use crate::i18n::{self, Lang};

thread_local! {
    static WORDS_CACHE: RefCell<Option<Rc<WordsData>>> = const { RefCell::new(None) };
}

/// Downloads, inflates and parses `data/words.bin.gz` once.
pub async fn load() -> Result<Rc<WordsData>, String> {
    if let Some(cached) = WORDS_CACHE.with(|cache| cache.borrow().clone()) {
        return Ok(cached);
    }
    // The query keeps an older cached WRD1 v1 file from being parsed after a
    // format change; the server ignores it.
    let bytes = crate::render::fetch_bytes("data/words.bin.gz?v=2")
        .await
        .map_err(|error| format!("{error:?}"))?;
    let raw = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = flate2::read::GzDecoder::new(bytes.as_slice());
        let mut raw = Vec::new();
        decoder
            .read_to_end(&mut raw)
            .map_err(|error| format!("words.bin.gz: {error}"))?;
        raw
    } else {
        bytes
    };
    let data = Rc::new(WordsData::parse(&raw)?);
    WORDS_CACHE.with(|cache| *cache.borrow_mut() = Some(data.clone()));
    Ok(data)
}

/// Word buttons for one verse. The caller decides the container class and
/// whether to show the Byzantine/TR fallback note.
pub fn words_html(words: &WordsData, verse: u32, class: &str) -> String {
    let list = words.words_in_verse(verse);
    if list.is_empty() {
        return String::new();
    }
    let language = words.verse_language(verse).unwrap_or(0);
    let (dir, lang_attr) = match language {
        1 => ("rtl", "arc"),
        2 => ("ltr", "grc"),
        _ => ("rtl", "he"),
    };
    let mut html = format!("<span class=\"{class}\" dir=\"{dir}\" lang=\"{lang_attr}\">");
    for word in list {
        let Some(surface) = words.surface(word) else {
            continue;
        };
        let translit = words
            .lex(word.strong_id)
            .map(|entry| entry.translit)
            .unwrap_or("");
        let morph = words.morph(word).unwrap_or("");
        let title = if translit.is_empty() {
            String::new()
        } else {
            format!(" title=\"{}\"", escape(translit))
        };
        html.push_str(&format!(
            "<button class=\"word\" type=\"button\" data-strong=\"{}\" data-surface=\"{}\" data-morph=\"{}\"{}>{}</button>",
            word.strong_id,
            escape(surface),
            escape(morph),
            title,
            escape(surface)
        ));
    }
    html.push_str("</span>");
    html
}

pub fn fallback_note(lang: Lang) -> String {
    format!(
        "<p class=\"original-fallback\">{}</p>",
        i18n::tr(
            lang,
            "Texto bizantino/TR (ausente en NA28).",
            "Byzantine/TR text (absent from NA28)."
        )
    )
}

/// The full lexicon card, including the Spanish gloss/senses, the original
/// STEPBible article (English) and the occurrence button.
#[allow(clippy::too_many_arguments)]
pub fn card_html(
    words: &WordsData,
    strong_id: u32,
    surface: &str,
    morph: &str,
    lang: Lang,
    occurrences_id: &str,
    reference: Option<&str>,
) -> String {
    let Some(entry) = words.lex(strong_id) else {
        return String::new();
    };
    let count = words.occurrence_count(strong_id);
    let hebrew = entry.key.starts_with('H');
    let dir = if hebrew { "rtl" } else { "ltr" };
    let lang_attr = if hebrew { "he" } else { "grc" };
    let description = morph_description(morph, hebrew);
    let mut html = String::from("<div class=\"lex-head\">");
    html.push_str(&format!(
        "<span class=\"lex-key\">{}</span><strong class=\"lex-lemma\" dir=\"{}\" lang=\"{}\">{}</strong>",
        escape(entry.key),
        dir,
        lang_attr,
        escape(entry.lemma)
    ));
    if !entry.translit.is_empty() {
        html.push_str(&format!(
            "<span class=\"lex-translit\">{}</span>",
            escape(entry.translit)
        ));
    }
    html.push_str("</div>");

    if !surface.is_empty() {
        html.push_str(&format!(
            "<p class=\"lex-form\"><span class=\"lex-label\">{}</span> <strong dir=\"{}\" lang=\"{}\">{}</strong>",
            i18n::tr(lang, "Forma", "Form"),
            dir,
            lang_attr,
            escape(surface)
        ));
        if !morph.is_empty() {
            html.push_str(&format!(" <code>{}</code>", escape(morph)));
        }
        if !description.is_empty() {
            html.push_str(&format!(" · {}", escape(&description)));
        }
        html.push_str("</p>");
    }

    if entry.es_gloss.is_empty() {
        html.push_str(&format!(
            "<p class=\"lex-gloss\"><span class=\"lex-label\">{}</span> {} <span class=\"lex-lang\">{}</span></p>",
            i18n::tr(lang, "Glosa", "Gloss"),
            if entry.gloss.is_empty() {
                "—".to_string()
            } else {
                escape(entry.gloss)
            },
            i18n::tr(lang, "inglés", "English")
        ));
    } else {
        html.push_str(&format!(
            "<p class=\"lex-gloss\"><span class=\"lex-label\">{}</span> {}</p>",
            i18n::tr(lang, "Glosa", "Spanish gloss"),
            escape(entry.es_gloss)
        ));
    }

    if !entry.es_senses.is_empty() {
        let senses = format!("{}.", entry.es_senses.replace(" · ", "; "));
        html.push_str(&format!(
            "<p class=\"lex-definition\"><span class=\"lex-label\">{}</span> {}</p>",
            i18n::tr(lang, "Definición", "Spanish definition"),
            escape(&senses)
        ));
        html.push_str(&format!(
            "<p class=\"lex-source-note\">{}</p>",
            i18n::tr(
                lang,
                "Acepciones alineadas de la Reina-Valera 1909 (dominio público).",
                "Senses aligned from the Reina-Valera 1909 (public domain)."
            )
        ));
        if !entry.meaning.is_empty() {
            html.push_str(&format!(
                "<details class=\"lex-details\"><summary>{}</summary><p class=\"lex-meaning\">{}</p></details>",
                i18n::tr(
                    lang,
                    "Léxico STEPBible (inglés)",
                    "STEPBible lexicon (English)"
                ),
                escape(entry.meaning)
            ));
        }
    } else if !entry.meaning.is_empty() {
        html.push_str(&format!(
            "<p class=\"lex-definition lex-english\"><span class=\"lex-label\">{}</span> {}</p>",
            i18n::tr(lang, "Definición (inglés)", "Definition (English)"),
            escape(entry.meaning)
        ));
        html.push_str(&format!(
            "<p class=\"lex-source-note\">{}</p>",
            i18n::tr(
                lang,
                "Sin equivalentes españoles alineados en la RV1909 para este lema.",
                "No Spanish equivalents aligned from the RV1909 for this lemma."
            )
        ));
    } else {
        html.push_str(&format!(
            "<p class=\"panel-note\">{}</p>",
            i18n::tr(lang, "Sin definición en el léxico.", "No lexicon definition.")
        ));
    }

    let category = lex_category(entry.morph, lang);
    if !category.is_empty() && description.is_empty() {
        html.push_str(&format!(
            "<p class=\"lex-morph\"><span class=\"lex-label\">{}</span> {}</p>",
            i18n::tr(lang, "Categoría", "Category"),
            category
        ));
    }

    html.push_str(&format!(
        "<button class=\"lex-occurrences\" type=\"button\" data-occurrences=\"{}\">{}</button>",
        strong_id,
        format!(
            "{} ({})",
            i18n::tr(lang, "Ver ocurrencias", "Show occurrences"),
            count
        )
    ));
    html.push_str(&format!("<div id=\"{occurrences_id}\"></div>"));
    let reference_attr = reference
        .map(|reference| format!(" data-ref=\"{}\"", escape(reference)))
        .unwrap_or_default();
    html.push_str(&format!(
        "<button class=\"lex-arcs\" type=\"button\" data-lemma=\"{}\"{}>{}</button>",
        escape(entry.key),
        reference_attr,
        i18n::tr(
            lang,
            "Ver arcos con este lema",
            "Show arcs with this lemma"
        )
    ));
    html.push_str(&format!(
        "<p class=\"lex-credit\">{}</p>",
        i18n::tr(
            lang,
            "Glosas: RV1909, dominio público · Léxico: STEPBible, CC BY 4.0.",
            "Glosses: RV1909, public domain · Lexicon: STEPBible, CC BY 4.0."
        )
    ));
    html
}

pub fn occurrences_html(words: &WordsData, strong_id: u32, lang: Lang) -> String {
    let versification = bible_core::Versification::kjv();
    let all = words.occurrences(strong_id);
    let total = all.len();
    if total == 0 {
        return format!(
            "<p class=\"panel-note\">{}</p>",
            i18n::tr(lang, "Sin ocurrencias.", "No occurrences.")
        );
    }
    let mut html = format!(
        "<p class=\"lex-occurrences-title\">{} {}</p><div class=\"lex-occurrences-list\">",
        total,
        i18n::tr(lang, "ocurrencias", "occurrences")
    );
    for verse in all.iter().take(80) {
        let reference = i18n::format_verse(lang, &versification, *verse);
        html.push_str(&format!(
            "<button class=\"occurrence\" type=\"button\" data-occurrence=\"{}\">{}</button>",
            verse,
            escape(&reference)
        ));
    }
    html.push_str("</div>");
    if total > 80 {
        html.push_str(&format!(
            "<p class=\"panel-note\">{} 80 {} {}</p>",
            i18n::tr(lang, "Mostrando", "Showing"),
            i18n::tr(lang, "de", "of"),
            total
        ));
    }
    html
}

pub fn fill(container_id: &str, html: &str, show: bool) {
    let Some(container) = element(container_id) else {
        return;
    };
    container.set_inner_html(html);
    if show {
        let _ = container.remove_attribute("hidden");
    } else {
        let _ = container.set_attribute("hidden", "");
    }
}

/// URL that opens the Arcs view filtered by a lemma and optionally focused on
/// the verse the user was reading.
pub fn lemma_url(window: &web_sys::Window, key: &str, reference: Option<&str>) -> String {
    let mut set: Vec<(&str, &str)> = vec![("lemma", key)];
    if let Some(reference) = reference {
        set.push(("ref", reference));
    }
    // From the Threads reader this link must return to the Arcs view.
    i18n::url_with(window, &set, &["view", "original"])
}

pub fn word_of(element: &web_sys::Element) -> Option<(u32, String, String)> {
    let strong = element
        .get_attribute("data-strong")
        .and_then(|value| value.parse::<u32>().ok())?;
    let surface = element.get_attribute("data-surface").unwrap_or_default();
    let morph = element.get_attribute("data-morph").unwrap_or_default();
    Some((strong, surface, morph))
}

fn element(id: &str) -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
}

pub fn morph_description(code: &str, hebrew: bool) -> String {
    if code.is_empty() {
        return String::new();
    }
    let lang = i18n::lang();
    if hebrew {
        let mut chars = code.chars();
        let _ = chars.next();
        return match chars.next() {
            Some('V') => i18n::tr(lang, "verbo", "verb"),
            Some('N') => match chars.next() {
                Some('p') => i18n::tr(lang, "nombre propio", "proper noun"),
                Some('c') => i18n::tr(lang, "nombre común", "common noun"),
                _ => i18n::tr(lang, "nombre", "noun"),
            },
            Some('A') => i18n::tr(lang, "adjetivo", "adjective"),
            Some('R') => i18n::tr(lang, "preposición", "preposition"),
            Some('T') => i18n::tr(lang, "partícula", "particle"),
            Some('C') => i18n::tr(lang, "conjunción", "conjunction"),
            Some('D') => i18n::tr(lang, "adverbio", "adverb"),
            Some('P') => i18n::tr(lang, "pronombre", "pronoun"),
            Some('S') => i18n::tr(lang, "sufijo", "suffix"),
            _ => "",
        }
        .to_string();
    }

    let (part_of_speech, rest) = code.split_once('-').unwrap_or((code, ""));
    let noun = match part_of_speech {
        "N" => i18n::tr(lang, "nombre", "noun"),
        "V" => i18n::tr(lang, "verbo", "verb"),
        "A" => i18n::tr(lang, "adjetivo", "adjective"),
        "T" => i18n::tr(lang, "artículo", "article"),
        "P" => i18n::tr(lang, "pronombre", "pronoun"),
        "D" => i18n::tr(lang, "demostrativo", "demonstrative"),
        "R" => i18n::tr(lang, "relativo", "relative"),
        "PREP" => i18n::tr(lang, "preposición", "preposition"),
        "CONJ" => i18n::tr(lang, "conjunción", "conjunction"),
        "ADV" => i18n::tr(lang, "adverbio", "adverb"),
        "X" | "PRT" => i18n::tr(lang, "partícula", "particle"),
        "I" | "INJ" => i18n::tr(lang, "interjección", "interjection"),
        _ => "",
    };
    if noun.is_empty() {
        return String::new();
    }
    if part_of_speech != "V" {
        if rest.is_empty() {
            return noun.to_string();
        }
        let mut tail = rest;
        let mut person = None;
        if tail.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            person = tail.chars().next();
            tail = &tail[1..];
        }
        let mut out = String::from(noun);
        if let Some(person) = person {
            out.push(' ');
            out.push_str(greek_person(person, lang));
            out.push_str(i18n::tr(lang, " persona", " person"));
        }
        let chars: Vec<char> = tail.chars().collect();
        if person.is_some() && chars.len() >= 2 {
            out.push(' ');
            out.push_str(greek_case(chars[0], lang));
            out.push(' ');
            out.push_str(greek_number(chars[1]));
        } else if chars.len() >= 3 {
            out.push(' ');
            out.push_str(greek_case(chars[0], lang));
            out.push(' ');
            out.push_str(greek_number(chars[1]));
            out.push(' ');
            out.push_str(greek_gender(chars[2], lang));
        }
        return out.trim().to_string();
    }

    let mut rest = rest;
    let mut second = false;
    if let Some(stripped) = rest.strip_prefix('2') {
        rest = stripped;
        second = true;
    }
    let chars: Vec<char> = rest.chars().collect();
    if chars.len() < 3 {
        return noun.to_string();
    }
    let tense = greek_tense(chars[0], lang);
    let voice = greek_voice(chars[1], lang);
    let mood = greek_mood(chars[2], lang);
    let tail = rest[3..].strip_prefix('-').unwrap_or(&rest[3..]);
    let mut out = String::from(noun);
    if second {
        out.push_str(i18n::tr(lang, " segundo", " second"));
    }
    out.push(' ');
    out.push_str(tense);
    out.push(' ');
    out.push_str(voice);
    out.push(' ');
    out.push_str(mood);
    let tail_chars: Vec<char> = tail.chars().collect();
    if chars[2] == 'P' && tail_chars.len() >= 3 {
        out.push(' ');
        out.push_str(greek_case(tail_chars[0], lang));
        out.push(' ');
        out.push_str(greek_number(tail_chars[1]));
        out.push(' ');
        out.push_str(greek_gender(tail_chars[2], lang));
    } else if tail_chars.len() >= 2 && tail_chars[0].is_ascii_digit() {
        out.push(' ');
        out.push_str(greek_person(tail_chars[0], lang));
        out.push(' ');
        out.push_str(greek_number(tail_chars[1]));
    }
    out.trim().to_string()
}

pub fn lex_category(code: &str, lang: Lang) -> &'static str {
    let part = code.split_once(':').map(|(_, rest)| rest).unwrap_or(code);
    match part.chars().next() {
        Some('N') => i18n::tr(lang, "nombre", "noun"),
        Some('V') => i18n::tr(lang, "verbo", "verb"),
        Some('A') => i18n::tr(lang, "adjetivo", "adjective"),
        Some('R') => i18n::tr(lang, "preposición", "preposition"),
        Some('T') => i18n::tr(lang, "partícula", "particle"),
        Some('C') => i18n::tr(lang, "conjunción", "conjunction"),
        Some('D') => i18n::tr(lang, "adverbio", "adverb"),
        Some('P') => i18n::tr(lang, "pronombre", "pronoun"),
        Some('S') => i18n::tr(lang, "sufijo", "suffix"),
        _ => "",
    }
}

fn greek_case(code: char, lang: Lang) -> &'static str {
    match code {
        'N' => i18n::tr(lang, "nominativo", "nominative"),
        'G' => i18n::tr(lang, "genitivo", "genitive"),
        'D' => i18n::tr(lang, "dativo", "dative"),
        'A' => i18n::tr(lang, "acusativo", "accusative"),
        'V' => i18n::tr(lang, "vocativo", "vocative"),
        _ => "",
    }
}

fn greek_number(code: char) -> &'static str {
    match code {
        'S' => "singular",
        'P' => "plural",
        _ => "",
    }
}

fn greek_gender(code: char, lang: Lang) -> &'static str {
    match code {
        'M' => i18n::tr(lang, "masculino", "masculine"),
        'F' => i18n::tr(lang, "femenino", "feminine"),
        'N' => i18n::tr(lang, "neutro", "neuter"),
        _ => "",
    }
}

fn greek_person(code: char, lang: Lang) -> &'static str {
    match code {
        '1' => i18n::tr(lang, "1.ª", "1st"),
        '2' => i18n::tr(lang, "2.ª", "2nd"),
        '3' => i18n::tr(lang, "3.ª", "3rd"),
        _ => "",
    }
}

fn greek_tense(code: char, lang: Lang) -> &'static str {
    match code {
        'P' => i18n::tr(lang, "presente", "present"),
        'I' => i18n::tr(lang, "imperfecto", "imperfect"),
        'F' => i18n::tr(lang, "futuro", "future"),
        'A' => i18n::tr(lang, "aoristo", "aorist"),
        'X' => i18n::tr(lang, "perfecto", "perfect"),
        'Y' => i18n::tr(lang, "pluscuamperfecto", "pluperfect"),
        _ => "",
    }
}

fn greek_voice(code: char, lang: Lang) -> &'static str {
    match code {
        'A' => i18n::tr(lang, "activo", "active"),
        'M' => i18n::tr(lang, "medio", "middle"),
        'P' => i18n::tr(lang, "pasivo", "passive"),
        'E' | 'N' => i18n::tr(lang, "medio/pasivo", "middle/passive"),
        'D' => i18n::tr(lang, "deponente", "deponent"),
        _ => "",
    }
}

fn greek_mood(code: char, lang: Lang) -> &'static str {
    match code {
        'I' => i18n::tr(lang, "indicativo", "indicative"),
        'S' => i18n::tr(lang, "subjuntivo", "subjunctive"),
        'O' => i18n::tr(lang, "optativo", "optative"),
        'M' => i18n::tr(lang, "imperativo", "imperative"),
        'N' => i18n::tr(lang, "infinitivo", "infinitive"),
        'P' => i18n::tr(lang, "participio", "participle"),
        _ => "",
    }
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}
