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
const DATE_FONT: &str = "8px system-ui, -apple-system, \"Segoe UI\", sans-serif";
const GENRE_FONT: &str = "600 10px system-ui, -apple-system, \"Segoe UI\", sans-serif";

/// Composition palette: cool for the oldest books, warm for the newest.
const DATE_RAMP: [(f64, [u8; 3]); 5] = [
    (0.00, [91, 124, 250]),
    (0.25, [66, 179, 165]),
    (0.50, [201, 180, 88]),
    (0.75, [224, 138, 85]),
    (1.00, [215, 106, 134]),
];
const DATE_MIN: f64 = -1500.0;
const DATE_MAX: f64 = 100.0;

fn date_t(year: i16) -> f64 {
    ((f64::from(year) - DATE_MIN) / (DATE_MAX - DATE_MIN)).clamp(0.0, 1.0)
}

fn date_color(year: i16) -> String {
    let t = date_t(year);
    let mut low = DATE_RAMP[0];
    let mut high = DATE_RAMP[DATE_RAMP.len() - 1];
    for pair in DATE_RAMP.windows(2) {
        if t >= pair[0].0 && t <= pair[1].0 {
            low = pair[0];
            high = pair[1];
            break;
        }
    }
    let span = (high.0 - low.0).max(f64::EPSILON);
    let k = (t - low.0) / span;
    let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * k).round() as u8;
    format!(
        "rgb({} {} {})",
        mix(low.1[0], high.1[0]),
        mix(low.1[1], high.1[1]),
        mix(low.1[2], high.1[2])
    )
}

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
/// Horizontal arcs must clear the stats/hints overlays, otherwise the widest
/// spans are flattened against the top edge and their glow is clipped on wide
/// desktop windows.
const HORIZONTAL_TOP_MARGIN: f32 = 64.0;
const HORIZONTAL_LABEL_BAND: f32 = 66.0;
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
                max_height: (baseline - HORIZONTAL_TOP_MARGIN).max(0.0),
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
    measure_dates: bool,
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
        if measure_dates {
            let color = date_color(tables::COMPOSITION_YEARS[book].midpoint());
            context.set_fill_style_str(&color);
        } else {
            context.set_fill_style_str(GENRE_HEX[usize::from(tables::BOOK_GENRES[book])]);
        }
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
    if measure_dates {
        paint_date_ruler(context, layout);
    } else if !layout.vertical {
        paint_genre_labels(context, layout, versification, &along);
    }
    paint_overview(context, layout, view, versification, measure_dates);
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

fn paint_date_ruler(context: &CanvasRenderingContext2d, layout: &Layout) {
    if layout.vertical {
        // On portrait screens the left label band is too narrow for a second
        // scale; a compact ruler above the diagram still explains the colours.
        if f64::from(layout.width) >= 1180.0 {
            return;
        }
        let width = (f64::from(layout.width) - 32.0).min(210.0);
        let lang = i18n::lang();
        context.set_font("600 7px system-ui, -apple-system, \"Segoe UI\", sans-serif");
        context.set_fill_style_str(MUTED);
        context.set_text_baseline("top");
        context.set_text_align("left");
        let _ = context.fill_text(
            i18n::tr(lang, "FECHAS DE COMPOSICIÓN", "COMPOSITION DATES"),
            16.0,
            5.0,
        );
        paint_date_scale(context, 16.0, 16.0 + width, 17.0, true);
    } else {
        let start = f64::from(layout.axis_start);
        let end = f64::from(layout.axis_end);
        let y = f64::from(layout.baseline) + BAR_OFFSET + BAR_THICKNESS + 32.0;
        paint_date_scale(context, start, end, y, false);
    }
}

fn paint_date_scale(
    context: &CanvasRenderingContext2d,
    start: f64,
    end: f64,
    y: f64,
    gradient: bool,
) {
    let width = end - start;
    if width < 90.0 {
        return;
    }

    if gradient {
        // Portrait screens hide the legend card, so their compact ruler keeps
        // the colour ramp to explain what the bars mean.
        const SEGMENTS: usize = 64;
        for segment in 0..SEGMENTS {
            let t0 = segment as f64 / SEGMENTS as f64;
            let t1 = (segment + 1) as f64 / SEGMENTS as f64;
            let year = (DATE_MIN + (DATE_MAX - DATE_MIN) * ((t0 + t1) / 2.0)).round() as i16;
            context.set_fill_style_str(&date_color(year));
            context.fill_rect(start + width * t0, y, width * (t1 - t0) + 0.5, 3.0);
        }
    } else {
        // Desktop already has the ramp in the legend card; below the axis only
        // the year scale is repeated, not a second gradient.
        context.set_fill_style_str(AXIS);
        context.fill_rect(start, y, width, 1.0);
    }

    context.set_fill_style_str(MUTED);
    for year in [-1500i16, -1000, -500, 0, 100] {
        let x = start + width * date_t(year);
        let height = if year == 0 { 5.0 } else { 3.0 };
        context.fill_rect(x, y + if gradient { 3.0 } else { 1.0 }, 1.0, height);
    }

    let lang = i18n::lang();
    let first = i18n::tr(lang, "1500 a.C.", "1500 BC");
    let last = i18n::tr(lang, "100 d.C.", "AD 100");
    context.set_font(DATE_FONT);
    context.set_text_baseline("top");
    let mut occupied_right = f64::NEG_INFINITY;
    for (year, text, align) in [
        (-1500i16, first, "left"),
        (-1000, "1000", "center"),
        (-500, "500", "center"),
        (100, last, "right"),
    ] {
        let x = start + width * date_t(year);
        let text_width = measure(context, text);
        let left = match align {
            "left" => x,
            "right" => x - text_width,
            _ => x - text_width / 2.0,
        };
        if left < occupied_right + 8.0 {
            continue;
        }
        context.set_text_align(align);
        let _ = context.fill_text(text, x, y + 6.0);
        occupied_right = left + text_width;
    }
    context.set_text_align("center");
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
    measure_dates: bool,
) {
    let total = 31_102.0f32;
    if view.1 - view.0 >= total - 0.5 {
        return;
    }
    let horizontal = !layout.vertical;
    let length = f64::from(if horizontal { layout.width - 64.0 } else { layout.height - 64.0 });
    let offset = 32.0f64;
    let position = |verse: f32| offset + f64::from(verse / total) * length;
    if measure_dates {
        for book in 0..tables::BOOK_IDS.len() {
            let Some((book_start, book_end)) = versification.book_range(book) else {
                continue;
            };
            let from = position(book_start as f32);
            let to = position(book_end as f32 + 1.0);
            let color = date_color(tables::COMPOSITION_YEARS[book].midpoint());
            context.set_fill_style_str(&color);
            if horizontal {
                context.fill_rect(from, 6.0, (to - from).max(0.5), 3.0);
            } else {
                context.fill_rect(f64::from(layout.width) - 9.0, from, 3.0, (to - from).max(0.5));
            }
        }
    } else {
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
