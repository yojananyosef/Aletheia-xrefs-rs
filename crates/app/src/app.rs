use leptos::prelude::*;

use bible_core::tables;

use crate::i18n;

#[component]
fn App() -> impl IntoView {
    let lang = i18n::lang();
    view! {
        <div class="app">
            <header class="topbar">
                <a class="brand" href="./" aria-label=i18n::tr(lang, "Inicio", "Home")>
                    <svg class="brand-mark" viewBox="0 0 28 28" fill="none" aria-hidden="true">
                        <path d="M3.5 23a10.5 10.5 0 0 1 21 0" stroke="#eb568f" stroke-width="1.7" stroke-linecap="round"/>
                        <path d="M8 23a6 6 0 0 1 12 0" stroke="#e4b363" stroke-width="1.7" stroke-linecap="round"/>
                        <path d="M12.2 23a1.8 1.8 0 0 1 3.6 0" stroke="#17a7b4" stroke-width="1.7" stroke-linecap="round"/>
                        <path d="M2 23h24" stroke="currentColor" stroke-opacity=".28" stroke-width="1"/>
                    </svg>
                    <span class="brand-text">
                        <span class="brand-kicker">"ALETHEIA ARCS"</span>
                        <h1>{i18n::tr(lang, "Referencias cruzadas", "Bible Cross References")}</h1>
                    </span>
                </a>
                <nav class="views" aria-label=i18n::tr(lang, "Vistas", "Views")>
                    <a class="view active" id="view-arcs" href="./">{i18n::tr(lang, "Arcos", "Arcs")}</a>
                    <a class="view" id="view-threads" href="?view=threads">{i18n::tr(lang, "Hilos", "Threads")}</a>
                </nav>
                <form class="search" id="search-form" onsubmit="return false">
                    <div class="search-field">
                        <svg class="search-icon" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                            <circle cx="8.8" cy="8.8" r="5.8" stroke="currentColor" stroke-width="1.4"/>
                            <path d="m13.2 13.2 4 4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
                        </svg>
                        <input
                            id="search"
                            type="search"
                            autocomplete="off"
                            spellcheck="false"
                            aria-label=i18n::tr(lang, "Buscar una referencia", "Search a reference")
                            placeholder=i18n::tr(lang, "Ir a…  Jn 3:16, Sal 23", "Go to…  John 3:16, Ps 23")
                        />
                    </div>
                </form>
                <select class="translation" id="translation" aria-label=i18n::tr(lang, "Versión de la Biblia", "Bible version")>
                    <option value=i18n::TRANSLATIONS[0].id>{i18n::TRANSLATIONS[0].label}</option>
                    <option value=i18n::TRANSLATIONS[1].id>{i18n::TRANSLATIONS[1].label}</option>
                </select>
                <select class="book-jump" id="book-jump" aria-label=i18n::tr(lang, "Ir a un libro", "Go to a book")>
                    <option value="">{i18n::tr(lang, "Libro…", "Book…")}</option>
                    <optgroup label=i18n::tr(lang, "Antiguo Testamento", "Old Testament")>
                        {(0..39).map(|book| view! {
                            <option value=tables::BOOK_IDS[book]>{i18n::book_name(lang, book)}</option>
                        }).collect_view()}
                    </optgroup>
                    <optgroup label=i18n::tr(lang, "Nuevo Testamento", "New Testament")>
                        {(39..66).map(|book| view! {
                            <option value=tables::BOOK_IDS[book]>{i18n::book_name(lang, book)}</option>
                        }).collect_view()}
                    </optgroup>
                </select>
                <select class="translation lang-select" id="language" aria-label=i18n::tr(lang, "Idioma de la aplicación", "Application language")>
                    <option value="es">"ES"</option>
                    <option value="en">"EN"</option>
                </select>
                <button
                    class="icon-button"
                    id="share"
                    type="button"
                    title=i18n::tr(lang, "Compartir este enlace", "Share this link")
                    aria-label=i18n::tr(lang, "Compartir este enlace", "Share this link")
                >
                    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
                        <circle cx="18" cy="5" r="2.7" stroke="currentColor" stroke-width="1.5"/>
                        <circle cx="6" cy="12" r="2.7" stroke="currentColor" stroke-width="1.5"/>
                        <circle cx="18" cy="19" r="2.7" stroke="currentColor" stroke-width="1.5"/>
                        <path d="m8.4 10.7 6.9-4.2M8.4 13.3l6.9 4.2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                    </svg>
                </button>
                <a
                    class="icon-button about-link"
                    href="./about/"
                    title=i18n::tr(lang, "Acerca de, fuentes y fechas", "About, sources and dates")
                    aria-label=i18n::tr(lang, "Acerca de, fuentes y fechas", "About, sources and dates")
                >
                    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
                        <circle cx="12" cy="12" r="8.6" stroke="currentColor" stroke-width="1.5"/>
                        <path d="M12 10.9v5.2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                        <circle cx="12" cy="7.9" r="1" fill="currentColor"/>
                    </svg>
                </a>
                <button
                    class="filters-toggle"
                    id="filters-toggle"
                    type="button"
                    aria-expanded="false"
                    aria-controls="filters"
                >
                    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
                        <path d="M4 7h16M7 12h10M10 17h4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                    </svg>
                    <span class="button-label">{i18n::tr(lang, "Filtros", "Filters")}</span>
                </button>
            </header>
            <div id="arcs-root" class="arcs-root">
            <div class="stage legend-collapsed" id="stage">
                <canvas id="arcs"></canvas>
                <canvas id="axis"></canvas>
                <div class="legend" id="legend-genres" hidden>
                    <button class="legend-close" type="button" aria-label=i18n::tr(lang, "Ocultar la leyenda", "Hide the legend")>"×"</button>
                    <div class="legend-column">
                        <span class="legend-title">{i18n::tr(lang, "ANTIGUO TESTAMENTO", "OLD TESTAMENT")}</span>
                        <span class="legend-item"><i style="background:#eb568f"></i>{i18n::genre_name(lang, 0)}</span>
                        <span class="legend-item"><i style="background:#cd4805"></i>{i18n::genre_name(lang, 1)}</span>
                        <span class="legend-item"><i style="background:#b48f05"></i>{i18n::genre_name(lang, 2)}</span>
                        <span class="legend-item"><i style="background:#cd489d"></i>{i18n::genre_name(lang, 3)}</span>
                        <span class="legend-item"><i style="background:#88a00d"></i>{i18n::genre_name(lang, 4)}</span>
                    </div>
                    <div class="legend-column">
                        <span class="legend-title">{i18n::tr(lang, "NUEVO TESTAMENTO", "NEW TESTAMENT")}</span>
                        <span class="legend-item"><i style="background:#17a7b4"></i>{i18n::genre_name(lang, 5)}</span>
                        <span class="legend-item"><i style="background:#119245"></i>{i18n::genre_name(lang, 6)}</span>
                        <span class="legend-item"><i style="background:#0593e6"></i>{i18n::genre_name(lang, 7)}</span>
                        <span class="legend-item"><i style="background:#03ac8f"></i>{i18n::genre_name(lang, 8)}</span>
                        <span class="legend-item"><i style="background:#a06aec"></i>{i18n::genre_name(lang, 9)}</span>
                    </div>
                    <button class="legend-action" type="button" data-measure="date">
                        {i18n::tr(lang, "Ver fechas de composición", "View composition dates")}
                    </button>
                </div>
                <div class="legend legend-dates" id="legend-dates">
                    <button class="legend-close" type="button" aria-label=i18n::tr(lang, "Ocultar la leyenda", "Hide the legend")>"×"</button>
                    <span class="legend-title">{i18n::tr(lang, "FECHAS DE COMPOSICIÓN", "COMPOSITION DATES")}</span>
                    <div class="date-ramp" aria-hidden="true"></div>
                    <div class="date-ticks" aria-hidden="true">
                        <span style="left:0%">{i18n::tr(lang, "1500 a.C.", "1500 BC")}</span>
                        <span style="left:31.25%">"1000"</span>
                        <span style="left:62.5%">"500"</span>
                        <span style="left:100%">{i18n::tr(lang, "100 d.C.", "AD 100")}</span>
                    </div>
                    <p class="legend-note">
                        {i18n::tr(
                            lang,
                            "El color indica cuándo se compuso el libro, no cuándo ocurrieron los hechos narrados. Pasa el cursor o mantén pulsado un versículo para ver su fecha.",
                            "The colour shows when the book was composed, not when its events took place. Hover or long-press a verse to see its date.",
                        )}
                    </p>
                    <a class="legend-source" href="./about/">
                        {i18n::tr(lang, "Metodología y tabla completa", "Method and full table")}
                    </a>
                    <button class="legend-action" type="button" data-measure="genre">
                        {i18n::tr(lang, "Colorear por géneros", "Colour by genre")}
                    </button>
                </div>
                <button class="legend-open" id="legend-open" type="button" title=i18n::tr(lang, "Mostrar la leyenda", "Show the legend")>
                    <span class="legend-open-swatch" id="legend-open-swatch" aria-hidden="true"></span>
                    <span id="legend-open-label">{i18n::tr(lang, "Composición", "Composition")}</span>
                </button>
                <div class="stats" id="stats"></div>
                <div class="lemma-banner" id="lemma-banner" role="status" aria-live="polite" hidden></div>
                <div class="hints">
                    <span>{i18n::tr(lang, "rueda para acercar", "scroll to zoom")}</span>
                    <span>{i18n::tr(lang, "arrastrar para desplazar", "drag to pan")}</span>
                    <span>{i18n::tr(lang, "clic para elegir", "click to select")}</span>
                </div>
                <div class="tooltip" id="tooltip" hidden></div>
                <div class="touch-hint" id="touch-hint">
                    {i18n::tr(lang, "Toca para abrir · Mantén pulsado para explorar", "Tap to open · Hold to explore")}
                </div>
                <div class="filters" id="filters" hidden>
                    <div class="filters-head">
                        <div>
                            <span class="filters-kicker">{i18n::tr(lang, "FILTROS", "FILTERS")}</span>
                            <h2>{i18n::tr(lang, "Acotar los arcos", "Narrow the arcs")}</h2>
                        </div>
                        <button class="icon-button" id="filters-close" type="button" aria-label=i18n::tr(lang, "Cerrar filtros", "Close filters")>"×"</button>
                    </div>
                    <label class="filter-field">
                        <span>{i18n::tr(lang, "Testamento", "Testament")}</span>
                        <select id="filter-testament">
                            <option value="">{i18n::tr(lang, "Todos", "All")}</option>
                            <option value="ot">{i18n::tr(lang, "Antiguo Testamento", "Old Testament")}</option>
                            <option value="nt">{i18n::tr(lang, "Nuevo Testamento", "New Testament")}</option>
                        </select>
                    </label>
                    <label class="filter-field">
                        <span>{i18n::tr(lang, "Género", "Genre")}</span>
                        <select id="filter-genre">
                            <option value="">{i18n::tr(lang, "Todos", "All")}</option>
                            {(0..tables::GENRE_NAMES.len()).map(|genre| view! {
                                <option value=genre.to_string()>{i18n::genre_name(lang, genre)}</option>
                            }).collect_view()}
                        </select>
                    </label>
                    <label class="filter-field">
                        <span>{i18n::tr(lang, "Libro", "Book")}</span>
                        <select id="filter-book">
                            <option value="">{i18n::tr(lang, "Todos", "All")}</option>
                            <optgroup label=i18n::tr(lang, "Antiguo Testamento", "Old Testament")>
                                {(0..39).map(|book| view! {
                                    <option value=tables::BOOK_IDS[book]>{i18n::book_name(lang, book)}</option>
                                }).collect_view()}
                            </optgroup>
                            <optgroup label=i18n::tr(lang, "Nuevo Testamento", "New Testament")>
                                {(39..66).map(|book| view! {
                                    <option value=tables::BOOK_IDS[book]>{i18n::book_name(lang, book)}</option>
                                }).collect_view()}
                            </optgroup>
                        </select>
                    </label>
                    <div class="filter-field">
                        <span>{i18n::tr(lang, "Escala del eje", "Axis scale")}</span>
                        <div class="measure-toggle" role="group" aria-label=i18n::tr(lang, "Escala del eje", "Axis scale")>
                            <button class="measure-option" type="button" data-measure="genre" aria-pressed="false">
                                {i18n::tr(lang, "Género", "Genre")}
                            </button>
                            <button class="measure-option" type="button" data-measure="date" aria-pressed="true">
                                {i18n::tr(lang, "Composición", "Composition")}
                            </button>
                        </div>
                    </div>
                    <div class="filters-divider"></div>
                    <label class="filters-row">
                        <span>{i18n::tr(lang, "Referencias con al menos", "References with at least")}</span>
                        <output id="votes-value">"4"</output>
                        <span>{i18n::tr(lang, "votos", "votes")}</span>
                    </label>
                    <input id="votes" type="range" min="0" max="100" step="1" value="4" />
                    <p class="filters-note" id="filter-count"></p>
                    <div class="filters-foot">
                        <span>{i18n::tr(lang, "Los filtros se aplican al versículo de origen.", "Filters apply to the source verse.")}</span>
                        <button id="filters-reset" type="button">{i18n::tr(lang, "Limpiar", "Reset")}</button>
                    </div>
                    <a class="filters-about" href="./about/">
                        {i18n::tr(lang, "Acerca de, fuentes y fechas de composición", "About, sources and composition dates")}
                    </a>
                </div>
                <div class="status" id="status">{i18n::tr(lang, "Cargando datos…", "Loading data…")}</div>
            </div>
            <aside class="panel docked" id="panel" hidden></aside>
            </div>
            <section id="threads-root" class="threads-root" hidden>
                <div class="reader" id="threads-scroll">
                    <div id="threads-reader"></div>
                </div>
                <div class="threads-stage" id="threads-stage">
                    <canvas id="threads-canvas"></canvas>
                </div>
                <aside class="panel docked" id="threads-panel" hidden></aside>
            </section>
            <div class="toast" id="toast" role="status" aria-live="polite" hidden></div>
        </div>
    }
}

pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
    wasm_bindgen_futures::spawn_local(async {
        if let Err(error) = crate::render::run().await {
            web_sys::console::error_1(&error);
            crate::render::set_status_error(&format!("Error: {error:?}"));
        }
    });
}
