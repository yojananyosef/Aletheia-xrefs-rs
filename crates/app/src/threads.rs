//! Threads view: a chapter is read on the left; each verse's cross-references
//! curve across the gutter to the whole-Bible spine on the right.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlElement};

use bible_core::format::{TranslationText, XrefData};
use bible_core::{tables, VerseRef, Versification};

use crate::axis::GENRE_HEX;
use crate::i18n;

const MIN_VOTES: i16 = 4;
const SPINE_TOP: f64 = 16.0;
const SPINE_BOTTOM: f64 = 14.0;
const TEXT: &str = "#ecebe6";
const MUTED: &str = "#9a9890";
const LABEL_FONT: &str = "10px system-ui, -apple-system, \"Segoe UI\", sans-serif";
/// Safety cap for the threads of one chapter.
const MAX_THREADS: usize = 12_000;

struct Link {
    source: u32,
    start: u32,
    end: u32,
    votes: i16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Outgoing,
    Incoming,
    Both,
}

struct Thread {
    here: u32,
    target: f32,
    genre: usize,
    weight: f32,
}

struct Segment {
    book: usize,
    start: u32,
    end: u32,
    top: f64,
    bottom: f64,
}

struct Spine {
    segments: Vec<Segment>,
    pixels_per_verse: f64,
}

impl Spine {
    fn compute(versification: &Versification, height: f64) -> Self {
        let mut breaks = 0usize;
        for book in 1..tables::BOOK_IDS.len() {
            if tables::BOOK_GENRES[book] != tables::BOOK_GENRES[book - 1] {
                breaks += 1;
            }
        }
        let gap_total = (tables::BOOK_IDS.len() - 1 - breaks) as f64 + breaks as f64 * 5.0;
        let usable = (height - SPINE_TOP - SPINE_BOTTOM - gap_total).max(0.0);
        let pixels_per_verse = usable / f64::from(versification.verse_count());
        let mut segments = Vec::with_capacity(tables::BOOK_IDS.len());
        let mut y = SPINE_TOP;
        for book in 0..tables::BOOK_IDS.len() {
            if book > 0 {
                y += if tables::BOOK_GENRES[book] == tables::BOOK_GENRES[book - 1] {
                    1.0
                } else {
                    5.0
                };
            }
            let (start, end) = versification.book_range(book).unwrap_or((0, 0));
            let height = f64::from(end - start + 1) * pixels_per_verse;
            segments.push(Segment {
                book,
                start,
                end,
                top: y,
                bottom: y + height,
            });
            y += height;
        }
        Self {
            segments,
            pixels_per_verse,
        }
    }

    fn y_of(&self, verse: u32) -> f64 {
        for segment in &self.segments {
            if verse >= segment.start && verse <= segment.end {
                return segment.top + (f64::from(verse - segment.start) + 0.5) * self.pixels_per_verse;
            }
        }
        SPINE_TOP
    }

    fn book_at(&self, y: f64) -> Option<usize> {
        self.segments
            .iter()
            .find(|segment| y >= segment.top && y <= segment.bottom)
            .map(|segment| segment.book)
    }
}

struct State {
    book: usize,
    chapter: u16,
    direction: Direction,
    selected: Option<u32>,
    hover_verse: Option<u32>,
    hover_book: Option<usize>,
    spine: Option<Spine>,
    width: f64,
    height: f64,
    pixel_width: u32,
    pixel_height: u32,
    compact: bool,
}

pub struct Threads {
    links: Vec<Link>,
    text: TranslationText,
    versification: Versification,
    reader: HtmlElement,
    reader_scroll: HtmlElement,
    stage: HtmlElement,
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    state: RefCell<State>,
}

impl Threads {
    pub fn new(
        xrefs: &XrefData,
        text: TranslationText,
        versification: Versification,
    ) -> Result<Rc<Self>, JsValue> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| JsValue::from_str("no document"))?;
        let reader: HtmlElement = document
            .get_element_by_id("threads-reader")
            .ok_or_else(|| JsValue::from_str("missing #threads-reader"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("bad reader"))?;
        let reader_scroll: HtmlElement = document
            .get_element_by_id("threads-scroll")
            .ok_or_else(|| JsValue::from_str("missing #threads-scroll"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("bad reader scroll"))?;
        let stage: HtmlElement = document
            .get_element_by_id("threads-stage")
            .ok_or_else(|| JsValue::from_str("missing #threads-stage"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("bad stage"))?;
        let canvas: HtmlCanvasElement = document
            .get_element_by_id("threads-canvas")
            .ok_or_else(|| JsValue::from_str("missing #threads-canvas"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("bad canvas"))?;
        let context: CanvasRenderingContext2d = canvas
            .get_context("2d")
            .map_err(|_| JsValue::from_str("2d context failed"))?
            .ok_or_else(|| JsValue::from_str("no 2d context"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("bad 2d context"))?;
        let mut links = Vec::with_capacity(xrefs.link_count() / 2);
        for source in 0..xrefs.verse_count as usize {
            let start = xrefs.offsets[source] as usize;
            let end = xrefs.offsets[source + 1] as usize;
            for id in start..end {
                if xrefs.votes[id] < MIN_VOTES {
                    continue;
                }
                links.push(Link {
                    source: source as u32,
                    start: xrefs.target_start[id],
                    end: xrefs.target_end[id],
                    votes: xrefs.votes[id],
                });
            }
        }

        let state = State {
            book: 0,
            chapter: 1,
            direction: Direction::Outgoing,
            selected: None,
            hover_verse: None,
            hover_book: None,
            spine: None,
            width: 1.0,
            height: 1.0,
            pixel_width: 0,
            pixel_height: 0,
            compact: false,
        };
        let threads = Rc::new(Self {
            links,
            text,
            versification,
            reader,
            reader_scroll,
            stage,
            canvas,
            context,
            state: RefCell::new(state),
        });
        threads.attach();
        Ok(threads)
    }

    pub fn set_passage(self: &Rc<Self>, book: usize, chapter: u16) {
        {
            let mut state = self.state.borrow_mut();
            state.book = book;
            state.chapter = chapter;
            state.selected = None;
            state.hover_verse = None;
        }
        self.render_chapter();
        self.update_url();
    }

    fn attach(self: &Rc<Self>) {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        if let Some(button) = document.get_element_by_id("threads-prev") {
            let this = self.clone();
            let handler = Closure::<dyn FnMut()>::new(move || {
                if let Some((book, chapter)) = this.previous_chapter() {
                    this.set_passage(book, chapter);
                }
            });
            let _ = button.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
            handler.forget();
        }
        if let Some(button) = document.get_element_by_id("threads-next") {
            let this = self.clone();
            let handler = Closure::<dyn FnMut()>::new(move || {
                if let Some((book, chapter)) = this.next_chapter() {
                    this.set_passage(book, chapter);
                }
            });
            let _ = button.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
            handler.forget();
        }
        if let Some(button) = document.get_element_by_id("threads-close") {
            let this = self.clone();
            let handler = Closure::<dyn FnMut()>::new(move || {
                this.state.borrow_mut().selected = None;
                if let Some(panel) = document.get_element_by_id("threads-panel") {
                    let _ = panel.set_attribute("hidden", "");
                }
                this.draw();
            });
            let _ = button.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
            handler.forget();
        }

        // Hover and click on verses (event delegation: chapters replace the DOM).
        let this = self.clone();
        let hover = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |event: web_sys::PointerEvent| {
            let verse = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .and_then(|element| element.closest(".verse").ok().flatten())
                .and_then(|element| element.get_attribute("data-v"))
                .and_then(|value| value.parse::<u32>().ok());
            let changed = this.state.borrow().hover_verse != verse;
            if changed {
                this.state.borrow_mut().hover_verse = verse;
                this.draw();
            }
        });
        let _ = self.reader.add_event_listener_with_callback("pointermove", hover.as_ref().unchecked_ref());
        hover.forget();

        let this = self.clone();
        let leave = Closure::<dyn FnMut()>::new(move || {
            if this.state.borrow().hover_verse.is_some() {
                this.state.borrow_mut().hover_verse = None;
                this.draw();
            }
        });
        let _ = self.reader.add_event_listener_with_callback("pointerleave", leave.as_ref().unchecked_ref());
        leave.forget();

        let this = self.clone();
        let click = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let verse = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .and_then(|element| element.closest(".verse").ok().flatten())
                .and_then(|element| element.get_attribute("data-v"))
                .and_then(|value| value.parse::<u32>().ok());
            if let Some(verse) = verse {
                this.state.borrow_mut().selected = Some(verse);
                this.draw();
                this.open_panel(verse);
            }
        });
        let _ = self.reader.add_event_listener_with_callback("click", click.as_ref().unchecked_ref());
        click.forget();

        // Redraw on scroll: threads only leave from the verses on screen.
        let this = self.clone();
        let scroll = Closure::<dyn FnMut()>::new(move || {
            this.draw();
        });
        let _ = self.reader_scroll.add_event_listener_with_callback("scroll", scroll.as_ref().unchecked_ref());
        scroll.forget();

        // Spine interactions: hover a book, click to open its first chapter.
        let this = self.clone();
        let spine_move = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |event: web_sys::PointerEvent| {
            let rect = this.stage.get_bounding_client_rect();
            let x = event.client_x() as f64 - rect.left();
            let y = event.client_y() as f64 - rect.top();
            let state = this.state.borrow();
            let book = state
                .spine
                .as_ref()
                .filter(|_| x >= state.width - 110.0)
                .and_then(|spine| spine.book_at(y));
            drop(state);
            if this.state.borrow().hover_book != book {
                this.state.borrow_mut().hover_book = book;
                this.draw();
            }
        });
        let _ = self.stage.add_event_listener_with_callback("pointermove", spine_move.as_ref().unchecked_ref());
        spine_move.forget();

        let this = self.clone();
        let spine_click = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |event: web_sys::PointerEvent| {
            let rect = this.stage.get_bounding_client_rect();
            let x = event.client_x() as f64 - rect.left();
            let y = event.client_y() as f64 - rect.top();
            let state = this.state.borrow();
            if x < state.width - 110.0 {
                return;
            }
            let book = state.spine.as_ref().and_then(|spine| spine.book_at(y));
            drop(state);
            if let Some(book) = book {
                this.set_passage(book, 1);
            }
        });
        let _ = self.stage.add_event_listener_with_callback("click", spine_click.as_ref().unchecked_ref());
        spine_click.forget();

        let this = self.clone();
        let resize = Closure::<dyn FnMut()>::new(move || {
            this.draw();
        });
        let _ = window.add_event_listener_with_callback("resize", resize.as_ref().unchecked_ref());
        resize.forget();
    }

    fn previous_chapter(&self) -> Option<(usize, u16)> {
        let state = self.state.borrow();
        if state.chapter > 1 {
            return Some((state.book, state.chapter - 1));
        }
        if state.book > 0 {
            let book = state.book - 1;
            let chapters = self.versification.chapters_of(book) as u16;
            return Some((book, chapters));
        }
        None
    }

    fn next_chapter(&self) -> Option<(usize, u16)> {
        let state = self.state.borrow();
        let chapters = self.versification.chapters_of(state.book) as u16;
        if state.chapter < chapters {
            return Some((state.book, state.chapter + 1));
        }
        if state.book + 1 < tables::BOOK_IDS.len() {
            return Some((state.book + 1, 1));
        }
        None
    }

    fn render_chapter(self: &Rc<Self>) {
        let lang = i18n::lang();
        let (book, chapter) = {
            let state = self.state.borrow();
            (state.book, state.chapter)
        };
        let (start, end) = self
            .versification
            .chapter_range(book, chapter)
            .unwrap_or((0, 0));
        let genre = usize::from(tables::BOOK_GENRES[book]);
        let testament = i18n::testament(lang, book);
        let genre_name = i18n::genre_name(lang, genre);
        let name = format!("{} {}", i18n::book_name(lang, book), chapter);
        let prev = self
            .previous_chapter()
            .map(|(book, chapter)| {
                format!(
                    "{} {}",
                    i18n::book_name(lang, book),
                    chapter
                )
            })
            .unwrap_or_default();
        let next = self
            .next_chapter()
            .map(|(book, chapter)| {
                format!(
                    "{} {}",
                    i18n::book_name(lang, book),
                    chapter
                )
            })
            .unwrap_or_default();

        let direction = self.state.borrow().direction;
        let mut counts = vec![0usize; (end - start + 1) as usize];
        for link in &self.links {
            if direction != Direction::Incoming && link.source >= start && link.source <= end {
                counts[(link.source - start) as usize] += 1;
            }
            if direction != Direction::Outgoing && link.start <= end && link.end >= start {
                let from = link.start.max(start);
                let to = link.end.min(end);
                for verse in from..=to {
                    counts[(verse - start) as usize] += 1;
                }
            }
        }
        let mut verses = String::with_capacity(8192);
        for verse in start..=end {
            let content = self.text.text(verse).unwrap_or_default();
            let content = if content.trim().is_empty() {
                format!(
                    "<span class=\"verse-empty\">{}</span>",
                    i18n::empty_verse(lang)
                )
            } else {
                escape(content)
            };
            let number = self
                .versification
                .ref_at(verse)
                .map(|reference| reference.verse)
                .unwrap_or(1);
            let count = counts[(verse - start) as usize];
            verses.push_str(&format!(
                "<p class=\"verse\" data-v=\"{verse}\"><span class=\"verse-text\"><span class=\"verse-num\">{number}</span>{}</span><span class=\"verse-count\">{count}</span></p>",
                content
            ));
        }
        let header = format!(
            "<header class=\"chapter-head\"><span class=\"chapter-kicker\"><i style=\"background:{}\"></i>{} · {}</span><h2 id=\"threads-title\">{}</h2><nav class=\"chapter-nav\"><button id=\"threads-prev\" type=\"button\" title=\"{} ({prev})\">‹</button><button id=\"threads-next\" type=\"button\" title=\"{} ({next})\">›</button></nav><div class=\"threads-directions\"><button id=\"dir-out\" type=\"button\"{}>{}</button><button id=\"dir-in\" type=\"button\"{}>{}</button><button id=\"dir-both\" type=\"button\"{}>{}</button></div></header>",
            GENRE_HEX[genre],
            escape(genre_name),
            testament,
            escape(&name),
            i18n::tr(lang, "Capítulo anterior", "Previous chapter"),
            i18n::tr(lang, "Capítulo siguiente", "Next chapter"),
            if direction == Direction::Outgoing { " class=\"active\"" } else { "" },
            i18n::tr(lang, "Remite a", "Refers to"),
            if direction == Direction::Incoming { " class=\"active\"" } else { "" },
            i18n::tr(lang, "Citado por", "Cited by"),
            if direction == Direction::Both { " class=\"active\"" } else { "" },
            i18n::tr(lang, "Ambos", "Both"),
        );
        self.reader.set_inner_html(&format!("{header}<div class=\"verses\">{verses}</div>"));
        self.reader_scroll.set_scroll_top(0);
        // Fresh nodes: rebind the chapter buttons.
        self.attach_chapter_buttons();
        self.attach_direction_buttons();
        self.draw();
    }

    fn attach_direction_buttons(self: &Rc<Self>) {
        let document = web_sys::window().unwrap().document().unwrap();
        for (id, direction) in [
            ("dir-out", Direction::Outgoing),
            ("dir-in", Direction::Incoming),
            ("dir-both", Direction::Both),
        ] {
            let Some(button) = document.get_element_by_id(id) else {
                continue;
            };
            let this = self.clone();
            let handler = Closure::<dyn FnMut()>::new(move || {
                if this.state.borrow().direction != direction {
                    this.state.borrow_mut().direction = direction;
                    this.render_chapter();
                }
            });
            let _ = button.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
            handler.forget();
        }
    }

    fn attach_chapter_buttons(self: &Rc<Self>) {
        let document = web_sys::window().unwrap().document().unwrap();
        if let Some(button) = document.get_element_by_id("threads-prev") {
            let this = self.clone();
            let handler = Closure::<dyn FnMut()>::new(move || {
                if let Some((book, chapter)) = this.previous_chapter() {
                    this.set_passage(book, chapter);
                }
            });
            let _ = button.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
            handler.forget();
        }
        if let Some(button) = document.get_element_by_id("threads-next") {
            let this = self.clone();
            let handler = Closure::<dyn FnMut()>::new(move || {
                if let Some((book, chapter)) = this.next_chapter() {
                    this.set_passage(book, chapter);
                }
            });
            let _ = button.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
            handler.forget();
        }
    }

    fn connections(&self, start: u32, end: u32) -> Vec<Thread> {
        let direction = self.state.borrow().direction;
        let mut threads = Vec::new();
        let mut seen: Vec<(u32, u32, u32)> = Vec::new();
        for link in &self.links {
            let duplicate = seen.contains(&(link.source, link.start, link.end));
            if direction != Direction::Incoming && link.source >= start && link.source <= end {
                seen.push((link.source, link.start, link.end));
                threads.push(Thread {
                    here: link.source,
                    target: (link.start as f32 + link.end as f32 + 1.0) / 2.0,
                    genre: usize::from(
                        self.versification.genre_of_verse(link.start),
                    ),
                    weight: vote_weight(link.votes),
                });
            }
            if direction != Direction::Outgoing && link.start <= end && link.end >= start {
                if duplicate {
                    continue;
                }
                seen.push((link.source, link.start, link.end));
                let here = link.start.max(start).min(end);
                threads.push(Thread {
                    here,
                    target: (link.start as f32 + link.end as f32 + 1.0) / 2.0,
                    genre: usize::from(self.versification.genre_of_verse(link.start)),
                    weight: vote_weight(link.votes),
                });
            }
            if threads.len() >= MAX_THREADS {
                break;
            }
        }
        threads.sort_by(|a, b| a.weight.total_cmp(&b.weight));
        threads
    }

    fn draw(&self) {
        let rect = self.stage.get_bounding_client_rect();
        let width = rect.width().max(1.0);
        let height = rect.height().max(1.0);
        let compact = width < 160.0;
        let dpr = web_sys::window()
            .map(|window| window.device_pixel_ratio().min(2.0))
            .unwrap_or(1.0);
        let pixel_width = (width * dpr).round().max(1.0) as u32;
        let pixel_height = (height * dpr).round().max(1.0) as u32;
        {
            let mut state = self.state.borrow_mut();
            if state.pixel_width != pixel_width || state.pixel_height != pixel_height {
                state.pixel_width = pixel_width;
                state.pixel_height = pixel_height;
                self.canvas.set_width(pixel_width);
                self.canvas.set_height(pixel_height);
                self.context
                    .set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0)
                    .ok();
            }
        }
        self.context.clear_rect(0.0, 0.0, width, height);

        let spine = Spine::compute(&self.versification, height);
        let spine_width = if compact { 6.0 } else { 8.0 };
        let bar_left = width - if compact { 18.0 } else { 96.0 };
        let (book, chapter, selected, hover_verse, hover_book) = {
            let mut state = self.state.borrow_mut();
            state.spine = Some(Spine::compute(&self.versification, height));
            state.width = width;
            state.height = height;
            state.compact = compact;
            (
                state.book,
                state.chapter,
                state.selected,
                state.hover_verse,
                state.hover_book,
            )
        };
        let (start, end) = self
            .versification
            .chapter_range(book, chapter)
            .unwrap_or((0, 0));
        let threads = if start == 0 && end == 0 {
            Vec::new()
        } else {
            self.connections(start, end)
        };
        let emphasized_verse = hover_verse.or(selected);
        let has_emphasis = emphasized_verse.is_some() || hover_book.is_some();
        let stage_top = rect.top();
        let gutter = (bar_left - 2.0).max(8.0);
        let bend = gutter * 0.55;

        // Anchors: the first line of each visible verse.
        let document = web_sys::window().unwrap().document().unwrap();
        let nodes = document.query_selector_all(".verse").ok();
        let mut anchors: Vec<(u32, f64)> = Vec::new();
        if let Some(nodes) = nodes {
            for index in 0..nodes.length() {
                let Some(node) = nodes.item(index) else { continue };
                let Ok(element) = node.dyn_into::<web_sys::Element>() else {
                    continue;
                };
                let Some(verse) = element
                    .get_attribute("data-v")
                    .and_then(|value| value.parse::<u32>().ok())
                else {
                    continue;
                };
                let bounds = element.get_bounding_client_rect();
                let y = bounds.top() - stage_top + bounds.height().min(30.0) / 2.0;
                if y >= 0.0 && y <= height {
                    anchors.push((verse, y));
                }
            }
        }

        let mut by_verse: Vec<Vec<&Thread>> = vec![Vec::new(); (end - start + 1) as usize];
        for thread in &threads {
            if thread.here >= start && thread.here <= end {
                by_verse[(thread.here - start) as usize].push(thread);
            }
        }

        self.context.set_line_width(1.0);
        for (verse, y) in &anchors {
            let Some(list) = by_verse.get((*verse - start) as usize) else {
                continue;
            };
            for thread in list {
                let emphasized = emphasized_verse == Some(*verse)
                    || hover_book.is_some_and(|book| {
                        spine.segments.iter().any(|segment| {
                            segment.book == book
                                && thread.target >= segment.start as f32
                                && thread.target <= segment.end as f32
                        })
                    });
                let alpha = if emphasized {
                    0.95
                } else if has_emphasis {
                    0.06 + 0.1 * thread.weight
                } else {
                    0.18 + 0.5 * thread.weight
                };
                self.context.set_global_alpha(f64::from(alpha));
                self.context.set_stroke_style_str(GENRE_HEX[thread.genre]);
                let target = spine.y_of(thread.target as u32);
                self.context.begin_path();
                let _ = self.context.move_to(0.0, *y);
                let _ = self.context.bezier_curve_to(
                    bend,
                    *y,
                    gutter - bend,
                    target,
                    bar_left - 2.0,
                    target,
                );
                let _ = self.context.stroke();
                self.context.set_global_alpha(1.0);
                self.context.set_fill_style_str(GENRE_HEX[thread.genre]);
                self.context.fill_rect(bar_left - 5.0, target - 0.5, 4.0, 1.0);
            }
        }
        self.context.set_global_alpha(1.0);

        // Spine bars and labels.
        for (index, segment) in spine.segments.iter().enumerate() {
            self.context.set_global_alpha(if index % 2 == 0 { 1.0 } else { 0.72 });
            self.context
                .set_fill_style_str(GENRE_HEX[usize::from(tables::BOOK_GENRES[segment.book])]);
            self.context.fill_rect(
                bar_left,
                segment.top,
                spine_width,
                (segment.bottom - segment.top).max(0.5),
            );
        }
        self.context.set_global_alpha(1.0);
        self.context.set_font(LABEL_FONT);
        self.context.set_text_baseline("middle");
        self.context.set_text_align("left");
        let lang = i18n::lang();
        for segment in &spine.segments {
            let height = segment.bottom - segment.top;
            let hovered = hover_book == Some(segment.book);
            if height < 10.0 && !hovered {
                continue;
            }
            let room = width - (bar_left + spine_width + 5.0) - 2.0;
            let label = [i18n::book_name(lang, segment.book), i18n::book_abbr(lang, segment.book)]
                .into_iter()
                .find(|text| self.measure(text) <= room);
            let Some(label) = label else { continue };
            self.context
                .set_fill_style_str(if hovered { TEXT } else { MUTED });
            let _ = self.context.fill_text(
                label,
                bar_left + spine_width + 5.0,
                (segment.top + segment.bottom) / 2.0,
            );
        }

        // "You are here": a bracket around the current chapter.
        let top = spine.y_of(start) - 2.0;
        let bottom = spine.y_of(end).max(top + 4.0) + 2.0;
        self.context.set_fill_style_str(TEXT);
        self.context.fill_rect(bar_left - 3.0, top, 2.0, bottom - top);
        self.context.begin_path();
        let _ = self.context.move_to(bar_left - 3.0, (top + bottom) / 2.0 - 4.0);
        let _ = self.context.line_to(bar_left - 8.0, (top + bottom) / 2.0);
        let _ = self.context.line_to(bar_left - 3.0, (top + bottom) / 2.0 + 4.0);
        let _ = self.context.fill();
    }

    fn measure(&self, text: &str) -> f64 {
        self.context
            .measure_text(text)
            .map(|metrics| metrics.width())
            .unwrap_or_else(|_| text.len() as f64 * 5.5)
    }

    fn open_panel(self: &Rc<Self>, verse: u32) {
        let Some(panel) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("threads-panel"))
        else {
            return;
        };
        let Some(reference) = self.versification.ref_at(verse) else {
            return;
        };
        let lang = i18n::lang();
        let label =
            i18n::format_reference(lang, reference.book, reference.chapter, reference.verse, None);
        let genre = usize::from(self.versification.genre_of_verse(verse));
        let body = self.text.text(verse).unwrap_or_default();
        let body = if body.trim().is_empty() {
            format!("<span class=\"verse-empty\">{}</span>", i18n::empty_verse(lang))
        } else {
            escape(&body)
        };
        let mut outgoing = Vec::new();
        let mut incoming = Vec::new();
        for link in &self.links {
            if link.source == verse {
                outgoing.push((link.source, link.start, link.end, link.votes));
            } else if link.start <= verse && link.end >= verse {
                incoming.push((link.source, link.start, link.end, link.votes));
            }
        }
        outgoing.sort_by(|a, b| b.3.cmp(&a.3));
        incoming.sort_by(|a, b| b.3.cmp(&a.3));
        let render = |items: &[(u32, u32, u32, i16)], from: bool| {
            let mut html = String::new();
            for (source, start, end, votes) in items.iter().take(10) {
                let anchor = if from { *start } else { *source };
                let Some(reference) = self.versification.ref_at(anchor) else {
                    continue;
                };
                let last = self
                    .versification
                    .ref_at(*end)
                    .map(|r| r.verse)
                    .unwrap_or(reference.verse);
                let text = self
                    .text
                    .text(anchor)
                    .map(|content| truncate(content, 130))
                    .unwrap_or_default();
                html.push_str(&format!(
                    "<div class=\"panel-item\"><div class=\"panel-item-head\"><span class=\"panel-item-ref\">{}</span><span class=\"panel-item-votes\">{} {}</span></div><p>{}</p></div>",
                    escape(&i18n::format_reference(lang, reference.book, reference.chapter, reference.verse, Some(last))),
                    votes,
                    i18n::tr(lang, "votos", "votes"),
                    escape(&text)
                ));
            }
            if html.is_empty() {
                html.push_str(&format!(
                    "<p class=\"panel-empty\">{}</p>",
                    i18n::tr(lang, "Ninguno", "None")
                ));
            }
            html
        };
        panel.set_inner_html(&format!(
            "<header class=\"panel-head\"><div><span class=\"panel-kicker\"><i style=\"background:{}\"></i>{} · {}</span><h2>{}</h2></div><button id=\"threads-close\" class=\"panel-close\" type=\"button\" aria-label=\"{}\">×</button></header><p class=\"panel-verse\">{}</p><section class=\"panel-section\"><h3>{} <span>{}</span></h3>{}</section><section class=\"panel-section\"><h3>{} <span>{}</span></h3>{}</section>",
            GENRE_HEX[genre],
            escape(i18n::genre_name(lang, genre)),
            i18n::testament(lang, reference.book),
            escape(&label),
            i18n::tr(lang, "Cerrar", "Close"),
            body,
            i18n::tr(lang, "Remite a", "Refers to"),
            outgoing.len(),
            render(&outgoing, true),
            i18n::tr(lang, "Citado por", "Cited by"),
            incoming.len(),
            render(&incoming, false),
        ));
        let _ = panel.remove_attribute("hidden");
        self.draw();
        if let Some(close) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("threads-close"))
        {
            let this = self.clone();
            let handler = Closure::<dyn FnMut()>::new(move || {
                this.state.borrow_mut().selected = None;
                if let Some(panel) = web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| document.get_element_by_id("threads-panel"))
                {
                    let _ = panel.set_attribute("hidden", "");
                }
                this.draw();
            });
            let _ = close.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
            handler.forget();
        }
    }

    fn update_url(&self) {
        let (book, chapter) = {
            let state = self.state.borrow();
            (state.book, state.chapter)
        };
        let Some(window) = web_sys::window() else {
            return;
        };
        let reference = format!("{}.{}.1", tables::BOOK_IDS[book], chapter);
        let url = i18n::url_with(
            &window,
            &[("view", "threads"), ("ref", &reference)],
            &[],
        );
        if let Ok(history) = window.history() {
            let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&url));
        }
    }
}

fn vote_weight(votes: i16) -> f32 {
    let votes = f32::from(votes.max(0));
    (0.2 + 0.8 * (1.0 + votes).log10() / 2.0).min(1.0)
}

fn escape(text: &str) -> String {
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

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "…"
}

pub async fn run(xrefs: XrefData, text: TranslationText) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window.document().ok_or_else(|| JsValue::from_str("no document"))?;
    if let Some(arcs) = document.get_element_by_id("arcs-root") {
        let _ = arcs.set_attribute("hidden", "");
    }
    if let Some(threads) = document.get_element_by_id("threads-root") {
        let _ = threads.remove_attribute("hidden");
    }
    if let Some(search) = document.get_element_by_id("search-form") {
        let _ = search.set_attribute("hidden", "");
    }
    if let Some(arcs) = document.get_element_by_id("view-arcs") {
        arcs.set_class_name("view");
    }
    if let Some(threads) = document.get_element_by_id("view-threads") {
        threads.set_class_name("view active");
    }
    if let Some(filters) = document.get_element_by_id("filters-toggle") {
        let _ = filters.set_attribute("hidden", "");
    }

    let versification = Versification::kjv();
    let lang = i18n::lang_of(&window);
    if let Some(document) = window.document() {
        document.set_title(i18n::tr(
            lang,
            "Hilos · Referencias cruzadas de la Biblia",
            "Threads · Bible Cross References",
        ));
    }
    crate::render::attach_translation_select(&window);
    i18n::attach_language_select(&window);
    let threads = Threads::new(&xrefs, text, Versification::kjv())?;
    let initial = crate::render::query_param(&window, "ref")
        .and_then(|reference| crate::render::parse_reference(&reference, &versification))
        .and_then(|(start, _)| versification.ref_at(start))
        .unwrap_or(VerseRef {
            book: 42,
            chapter: 3,
            verse: 16,
        });
    threads.set_passage(initial.book, initial.chapter);
    // Keep the value alive for the session; the closures hold their own Rc.
    std::mem::forget(threads);
    Ok(())
}
