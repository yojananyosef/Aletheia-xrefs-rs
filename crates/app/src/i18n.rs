//! App language and Bible translation registry.
//!
//! Spanish is the default language. The selected app language only decides
//! which translation is loaded when the URL does not name one explicitly;
//! `?t=` always wins so a shared link keeps its Bible version.

use bible_core::tables_es;
use bible_core::{tables as canonical, VerseRef, Versification};
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Es => "es",
            Self::En => "en",
        }
    }
}

pub struct Translation {
    pub id: &'static str,
    pub label: &'static str,
    pub lang: Lang,
}

pub const TRANSLATIONS: [Translation; 2] = [
    Translation {
        id: "plat",
        label: "Platense",
        lang: Lang::Es,
    },
    Translation {
        id: "asv",
        label: "ASV",
        lang: Lang::En,
    },
];

pub fn lang() -> Lang {
    web_sys::window()
        .as_ref()
        .map(lang_of)
        .unwrap_or(Lang::Es)
}

pub fn lang_of(window: &web_sys::Window) -> Lang {
    match query_param(window, "lang").as_deref() {
        Some("en") => Lang::En,
        _ => Lang::Es,
    }
}

pub fn default_translation(lang: Lang) -> &'static str {
    TRANSLATIONS
        .iter()
        .find(|translation| translation.lang == lang)
        .map(|translation| translation.id)
        .unwrap_or("plat")
}

/// Resolves `?t=` against the registry, accepting the old ids as aliases.
pub fn translation_id(window: &web_sys::Window) -> &'static str {
    let requested = query_param(window, "t");
    let id = match requested.as_deref() {
        Some("plat" | "spa" | "vbl") => "plat",
        Some("asv" | "bsb" | "kjv") => "asv",
        _ => default_translation(lang_of(window)),
    };
    TRANSLATIONS
        .iter()
        .find(|translation| translation.id == id)
        .map(|translation| translation.id)
        .unwrap_or_else(|| default_translation(lang_of(window)))
}

pub fn tr(lang: Lang, es: &'static str, en: &'static str) -> &'static str {
    match lang {
        Lang::Es => es,
        Lang::En => en,
    }
}

pub fn empty_verse(lang: Lang) -> &'static str {
    tr(lang, "No incluido en esta versión", "Not included in this version")
}

pub fn book_name(lang: Lang, book: usize) -> &'static str {
    match lang {
        Lang::Es => bible_core::book_name_es(book),
        Lang::En => canonical::BOOK_NAMES[book],
    }
}

pub fn book_ref_name(lang: Lang, book: usize) -> &'static str {
    match lang {
        Lang::Es => bible_core::book_ref_name_es(book),
        Lang::En if book == 18 => "Psalm",
        Lang::En => canonical::BOOK_NAMES[book],
    }
}

pub fn book_abbr(lang: Lang, book: usize) -> &'static str {
    match lang {
        Lang::Es => tables_es::BOOK_ABBR_ES[book],
        Lang::En => BOOK_ABBR_EN[book],
    }
}

pub fn genre_name(lang: Lang, genre: usize) -> &'static str {
    match lang {
        Lang::Es => bible_core::genre_name_es(genre),
        Lang::En => canonical::GENRE_NAMES[genre.min(canonical::GENRE_NAMES.len() - 1)],
    }
}

pub fn testament(lang: Lang, book: usize) -> &'static str {
    if book < 39 {
        tr(lang, "Antiguo Testamento", "Old Testament")
    } else {
        tr(lang, "Nuevo Testamento", "New Testament")
    }
}

pub fn format_reference(
    lang: Lang,
    book: usize,
    chapter: u16,
    start: u16,
    end: Option<u16>,
) -> String {
    let name = book_ref_name(lang, book);
    match end {
        Some(end) if end != start => format!("{name} {chapter}:{start}-{end}"),
        _ => format!("{name} {chapter}:{start}"),
    }
}

pub fn format_verse(lang: Lang, versification: &Versification, index: u32) -> String {
    match versification.ref_at(index) {
        Some(VerseRef {
            book,
            chapter,
            verse,
        }) => format_reference(lang, book, chapter, verse, None),
        None => String::from("—"),
    }
}

/// Keeps existing query parameters, replaces the given keys and drops the
/// listed ones. Used by focus, translation and language navigation so a URL
/// never loses `lang` or `t` by accident.
pub fn url_with(window: &web_sys::Window, set: &[(&str, &str)], remove: &[&str]) -> String {
    let search = window.location().search().unwrap_or_default();
    let mut pairs: Vec<(String, String)> = search
        .trim_start_matches('?')
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| pair.split_once('=').map(|(key, value)| (key.to_string(), value.to_string())))
        .collect();
    pairs.retain(|(key, _)| !remove.contains(&key.as_str()));
    for (name, value) in set {
        if let Some((_, current)) = pairs.iter_mut().find(|(key, _)| key == name) {
            *current = (*value).to_string();
        } else {
            pairs.push(((*name).to_string(), (*value).to_string()));
        }
    }
    let query = pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    if query.is_empty() {
        "./".to_string()
    } else {
        format!("?{query}")
    }
}

pub fn query_param(window: &web_sys::Window, name: &str) -> Option<String> {
    let search = window.location().search().ok()?;
    for pair in search.trim_start_matches('?').split('&') {
        if let Some(value) = pair.strip_prefix(&format!("{name}=")) {
            return Some(decode_component(value));
        }
    }
    None
}

/// Decodes `%XX` escapes and `+` in query values (UTF-8 aware).
fn decode_component(value: &str) -> String {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                    out.push(high * 16 + low);
                    index += 3;
                } else {
                    out.push(b'%');
                    index += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Ask the progressive-enhancement layer to refresh its themed dropdowns
/// after Rust changes a native select value programmatically.
pub fn sync_selects(window: &web_sys::Window) {
    if let Ok(event) = web_sys::Event::new("aletheia:sync-selects") {
        let _ = window.dispatch_event(&event);
    }
}

pub fn apply_document_language(window: &web_sys::Window) {
    let lang = lang_of(window);
    if let Some(document) = window.document() {
        if let Some(root) = document.document_element() {
            let _ = root.set_attribute("lang", lang.code());
        }
    }
}

pub fn attach_language_select(window: &web_sys::Window) {
    apply_document_language(window);
    let Some(select) = window
        .document()
        .and_then(|document| document.get_element_by_id("language"))
        .and_then(|node| node.dyn_into::<web_sys::HtmlSelectElement>().ok())
    else {
        return;
    };
    let current = lang_of(window);
    select.set_value(current.code());
    // Keep the language and translation when moving between the two views.
    if let Some(document) = window.document() {
        if let Some(arcs) = document.get_element_by_id("view-arcs") {
            let href = url_with(window, &[], &["view"]);
            let _ = arcs.set_attribute("href", &href);
        }
        if let Some(threads) = document.get_element_by_id("view-threads") {
            let href = url_with(window, &[("view", "threads")], &[]);
            let _ = threads.set_attribute("href", &href);
        }
    }
    let handler = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
        move |event: web_sys::Event| {
            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::HtmlSelectElement>().ok())
            else {
                return;
            };
            let Some(window) = web_sys::window() else {
                return;
            };
            let lang = if target.value() == "en" {
                Lang::En
            } else {
                Lang::Es
            };
            let translation = default_translation(lang);
            let url = url_with(
                &window,
                &[("lang", lang.code()), ("t", translation)],
                &["lang", "t"],
            );
            let _ = window.location().set_href(&url);
        },
    );
    let _ = select.add_event_listener_with_callback("change", handler.as_ref().unchecked_ref());
    handler.forget();
}

const BOOK_ABBR_EN: [&str; 66] = [
    "Gen", "Exod", "Lev", "Num", "Deut", "Josh", "Judg", "Ruth", "1Sam", "2Sam", "1Kgs",
    "2Kgs", "1Chr", "2Chr", "Ezra", "Neh", "Esth", "Job", "Ps", "Prov", "Eccl", "Song", "Isa",
    "Jer", "Lam", "Ezek", "Dan", "Hos", "Joel", "Amos", "Obad", "Jonah", "Mic", "Nah", "Hab",
    "Zeph", "Hag", "Zech", "Mal", "Matt", "Mark", "Luke", "John", "Acts", "Rom", "1Cor",
    "2Cor", "Gal", "Eph", "Phil", "Col", "1Thess", "2Thess", "1Tim", "2Tim", "Titus", "Phlm",
    "Heb", "Jas", "1Pet", "2Pet", "1John", "2John", "3John", "Jude", "Rev",
];
