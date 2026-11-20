//! Axis overlay: genre-coloured book bars, chapter ticks, labels, highlights
//! and the overview strip.

use bible_core::{tables, Versification};
use web_sys::CanvasRenderingContext2d;

use crate::i18n;

pub const GENRE_HEX: [&str; 10] = [
    "#eb568f", "#cd4805", "#b48f05", "#cd489d", "#88a00d", "#17a7b4", "#119245", "#0593e6",
    "#03ac8f", "#a06aec",
];

const TEXT: &str = "#ecebe6";
const MUTED: &str = "#9a9890";
const AXIS: &str = "#3a3935";
const BAR_OFFSET: f64 = 3.0;
const BAR_THICKNESS: f64 = 8.0;
const LABEL_FONT: &str = "11px system-ui, -apple-system, \"Segoe UI\", sans-serif";
const GENRE_FONT: &str = "600 10px system-ui, -apple-system, \"Segoe UI\", sans-serif";

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub vertical: bool,
    pub width: f32,
    pub height: f32,
    pub axis_start: f32,
    pub axis_end: f32,
    pub baseline: f32,
    pub max_height: f32,
}

const EDGE_MARGIN: f32 = 16.0;
const VERTICAL_TOP_MARGIN: f32 = 48.0;
const HORIZONTAL_LABEL_BAND: f32 = 58.0;
const VERTICAL_LABEL_BAND: f32 = 64.0;

impl Layout {
    pub fn compute(width: f32, height: f32) -> Self {
        if height > width * 1.15 {
            let baseline = VERTICAL_LABEL_BAND;
            Self {
                vertical: true,
                width,
                height,
                axis_start: VERTICAL_TOP_MARGIN,
                axis_end: height - EDGE_MARGIN,
                baseline,
                max_height: (width - baseline - EDGE_MARGIN).max(0.0),
            }
        } else {
            let baseline = height - HORIZONTAL_LABEL_BAND;
            Self {
                vertical: false,
                width,
                height,
                axis_start: EDGE_MARGIN * 2.0,
                axis_end: width - EDGE_MARGIN * 2.0,
                baseline,
                max_height: (baseline - EDGE_MARGIN).max(0.0),
            }
        }
    }

    pub fn pixels_per_verse(&self, view: (f32, f32)) -> f32 {
        (self.axis_end - self.axis_start) / (view.1 - view.0).max(f32::EPSILON)
    }

    pub fn along_of(&self, x: f32, y: f32) -> f32 {
        if self.vertical { y } else { x }
    }

    pub fn verse_to_along(&self, view: (f32, f32), verse: f32) -> f32 {
        self.axis_start + (verse - view.0) * self.pixels_per_verse(view)
    }

    pub fn along_to_verse(&self, view: (f32, f32), along: f32) -> f32 {
        view.0 + (along - self.axis_start) / self.pixels_per_verse(view).max(f32::EPSILON)
    }
}

/// Fills a rectangle in along/across coordinates. "Across" grows away from
/// the arcs: downward on a horizontal axis, mirrored left on a vertical one.
fn fill_along(
    context: &CanvasRenderingContext2d,
    layout: &Layout,
    along_from: f64,
    along_to: f64,
    across_from: f64,
    across_to: f64,
) {
    if !layout.vertical {
        context.fill_rect(along_from, across_from, along_to - along_from, across_to - across_from);
        return;
    }
    let mirrored_left = 2.0 * f64::from(layout.baseline) - across_to;
    context.fill_rect(mirrored_left, along_from, across_to - across_from, along_to - along_from);
}

pub fn paint(
    context: &CanvasRenderingContext2d,
    layout: &Layout,
    view: (f32, f32),
    versification: &Versification,
    hover: Option<(u32, u32)>,
    focus: Option<(u32, u32)>,
    dpr: f64,
) {
    let width = f64::from(layout.width);
    let height = f64::from(layout.height);
    context.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0).ok();
    context.clear_rect(0.0, 0.0, width, height);

    let along = |verse: f32| f64::from(layout.verse_to_along(view, verse));
    let visible = |from: f64, to: f64| {
        to >= f64::from(layout.axis_start) && from <= f64::from(layout.axis_end)
    };
    let clamp_along = |value: f64| {
        value
            .max(f64::from(layout.axis_start))
            .min(f64::from(layout.axis_end))
    };
    let bar_from = f64::from(layout.baseline) + BAR_OFFSET;
    let bar_to = bar_from + BAR_THICKNESS;

    // Axis line.
    context.set_fill_style_str(AXIS);
    fill_along(
        context,
        layout,
        f64::from(layout.axis_start),
        f64::from(layout.axis_end),
        f64::from(layout.baseline),
        f64::from(layout.baseline) + 1.0,
    );

    // Book bars; alternate books of a genre are dimmer so neighbours differ.
    for book in 0..tables::BOOK_IDS.len() {
        let Some((start, end)) = versification.book_range(book) else {
            continue;
        };
        let from = along(start as f32);
        let to = along(end as f32 + 1.0);
        if !visible(from, to) {
            continue;
        }
        let gap = if to - from > 4.0 { 1.0 } else { 0.0 };
        context.set_global_alpha(if book % 2 == 0 { 1.0 } else { 0.72 });
        context.set_fill_style_str(GENRE_HEX[usize::from(tables::BOOK_GENRES[book])]);
        fill_along(
            context,
            layout,
            clamp_along(from) + gap,
            clamp_along(to) - gap,
            bar_from,
            bar_to,
        );
    }
    context.set_global_alpha(1.0);

    // Chapter ticks once chapters are wide enough to tell apart.
    let chapter_pixels = f64::from(layout.pixels_per_verse(view)) * 26.0;
    if chapter_pixels > 10.0 {
        context.set_fill_style_str(MUTED);
        for book in 0..tables::BOOK_IDS.len() {
            for chapter in 1..=versification.chapters_of(book) {
                let Some((start, _)) = versification.chapter_range(book, chapter as u16) else {
                    continue;
                };
                let position = along(start as f32);
                if position >= f64::from(layout.axis_start) && position <= f64::from(layout.axis_end)
                {
                    fill_along(context, layout, position, position + 1.0, bar_to + 1.0, bar_to + 4.0);
                }
            }
        }
    }

    let highlight = |range: Option<(u32, u32)>, color: &str, min_width: f64| {
        let Some((start, end)) = range else {
            return;
        };
        let from = along(start as f32);
        let to = along(end as f32 + 1.0);
        if !visible(from, to) {
            return;
        }
        context.set_fill_style_str(color);
        let center = (clamp_along(from) + clamp_along(to)) / 2.0;
        let half = ((clamp_along(to) - clamp_along(from)) / 2.0).max(min_width);
        fill_along(context, layout, center - half, center + half, bar_from - 2.0, bar_to + 2.0);
    };
    highlight(hover, "rgb(236 235 230 / 0.45)", 1.5);
    highlight(focus, TEXT, 2.0);

    paint_book_labels(context, layout, view, versification, hover, focus, &along);
    if !layout.vertical {
        paint_genre_labels(context, layout, versification, &along);
    }
    paint_overview(context, layout, view, versification);
}

fn paint_book_labels(
    context: &CanvasRenderingContext2d,
    layout: &Layout,
    _view: (f32, f32),
    versification: &Versification,
    hover: Option<(u32, u32)>,
    focus: Option<(u32, u32)>,
    along: &impl Fn(f32) -> f64,
) {
    let label_offset = BAR_OFFSET + BAR_THICKNESS;
    let lang = i18n::lang();
    let side_room = f64::from(layout.baseline) - label_offset - 8.0;
    context.set_font(LABEL_FONT);
    context.set_text_baseline("middle");
    context.set_text_align(if layout.vertical { "right" } else { "center" });
    let touches = |range: Option<(u32, u32)>, start: u32, end: u32| {
        range.is_some_and(|(from, to)| from <= end && to >= start)
    };

    for book in 0..tables::BOOK_IDS.len() {
        let Some((book_start, book_end)) = versification.book_range(book) else {
            continue;
        };
        let from = along(book_start as f32).max(f64::from(layout.axis_start));
        let to = along(book_end as f32 + 1.0).min(f64::from(layout.axis_end));
        let room = to - from;
        if room <= 0.0 {
            continue;
        }
        let short = i18n::book_abbr(lang, book);
        let candidates = [i18n::book_name(lang, book), short];
        let label = if !layout.vertical {
            candidates
                .iter()
                .find(|text| measure(context, text) + 8.0 <= room)
        } else if room >= 12.0 {
            candidates
                .iter()
                .find(|text| measure(context, text) <= side_room)
        } else {
            None
        };
        let Some(label) = label else {
            continue;
        };
        let active = touches(focus, book_start, book_end) || touches(hover, book_start, book_end);
        context.set_fill_style_str(if active { TEXT } else { MUTED });
        let center = (from + to) / 2.0;
        if !layout.vertical {
            let _ = context.fill_text(label, center, f64::from(layout.baseline) + label_offset + 12.0);
        } else {
            let _ = context.fill_text(
                label,
                f64::from(layout.baseline) - label_offset - 5.0,
                center,
            );
        }
    }
}

fn paint_genre_labels(
    context: &CanvasRenderingContext2d,
    layout: &Layout,
    versification: &Versification,
    along: &impl Fn(f32) -> f64,
) {
    let lang = i18n::lang();
    context.set_font(GENRE_FONT);
    context.set_text_align("center");
    let y = f64::from(layout.baseline) + BAR_OFFSET + BAR_THICKNESS + 32.0;
    for genre in 0..tables::GENRE_NAMES.len() {
        let books: Vec<usize> = (0..tables::BOOK_IDS.len())
            .filter(|book| usize::from(tables::BOOK_GENRES[*book]) == genre)
            .collect();
        let (Some(first), Some(last)) = (books.first(), books.last()) else {
            continue;
        };
        let Some((first_start, _)) = versification.book_range(*first) else {
            continue;
        };
        let Some((_, last_end)) = versification.book_range(*last) else {
            continue;
        };
        let from = along(first_start as f32).max(f64::from(layout.axis_start));
        let to = along(last_end as f32 + 1.0).min(f64::from(layout.axis_end));
        let label = i18n::genre_name(lang, genre).to_uppercase();
        let text_width = measure(context, &label);
        if to - from < text_width + 16.0 {
            continue;
        }
        let center = (from + to) / 2.0;
        context.set_fill_style_str(GENRE_HEX[genre]);
        context.fill_rect(center - text_width / 2.0 - 12.0, y - 1.0, 8.0, 2.0);
        context.set_fill_style_str(MUTED);
        let _ = context.fill_text(&label, center + 2.0, y);
    }
}

fn paint_overview(
    context: &CanvasRenderingContext2d,
    layout: &Layout,
    view: (f32, f32),
    versification: &Versification,
) {
    let total = 31_102.0f32;
    if view.1 - view.0 >= total - 0.5 {
        return;
    }
    let horizontal = !layout.vertical;
    let length = f64::from(if horizontal { layout.width - 64.0 } else { layout.height - 64.0 });
    let offset = 32.0f64;
    let position = |verse: f32| offset + f64::from(verse / total) * length;
    for genre in 0..tables::GENRE_NAMES.len() {
        let books: Vec<usize> = (0..tables::BOOK_IDS.len())
            .filter(|book| usize::from(tables::BOOK_GENRES[*book]) == genre)
            .collect();
        let (Some(first), Some(last)) = (books.first(), books.last()) else {
            continue;
        };
        let (Some((first_start, _)), Some((_, last_end))) = (
            versification.book_range(*first),
            versification.book_range(*last),
        ) else {
            continue;
        };
        let from = position(first_start as f32);
        let to = position(last_end as f32 + 1.0);
        context.set_fill_style_str(GENRE_HEX[genre]);
        if horizontal {
            context.fill_rect(from, 6.0, to - from, 3.0);
        } else {
            context.fill_rect(f64::from(layout.width) - 9.0, from, 3.0, to - from);
        }
    }
    context.set_stroke_style_str(TEXT);
    context.set_line_width(1.0);
    let from = position(view.0);
    let to = position(view.1);
    if horizontal {
        context.stroke_rect(from - 0.5, 2.5, (to - from).max(3.0) + 1.0, 10.0);
    } else {
        context.stroke_rect(
            f64::from(layout.width) - 13.5,
            from - 0.5,
            10.0,
            (to - from).max(3.0) + 1.0,
        );
    }
}

fn measure(context: &CanvasRenderingContext2d, text: &str) -> f64 {
    context
        .measure_text(text)
        .map(|metrics| metrics.width())
        .unwrap_or_else(|_| text.len() as f64 * 6.0)
}
