use leptos::prelude::*;

use crate::i18n;

#[component]
fn App() -> impl IntoView {
    let lang = i18n::lang();
    view! {
        <div class="app">
            <header class="topbar">
                <h1>{i18n::tr(lang, "Referencias cruzadas de la Biblia", "Bible Cross References")}</h1>
                <nav class="views">
                    <a class="view active" id="view-arcs" href="./">{i18n::tr(lang, "Arcos", "Arcs")}</a>
                    <a class="view" id="view-threads" href="?view=threads">{i18n::tr(lang, "Hilos", "Threads")}</a>
                </nav>
                <form class="search" id="search-form" onsubmit="return false">
                    <input
                        id="search"
                        type="search"
                        autocomplete="off"
                        spellcheck="false"
                        placeholder=i18n::tr(lang, "Ir a…  Jn 3:16, Sal 23", "Go to…  John 3:16, Ps 23")
                    />
                </form>
                <select class="translation" id="translation" aria-label=i18n::tr(lang, "Versión de la Biblia", "Bible version")>
                    <option value=i18n::TRANSLATIONS[0].id>{i18n::TRANSLATIONS[0].label}</option>
                    <option value=i18n::TRANSLATIONS[1].id>{i18n::TRANSLATIONS[1].label}</option>
                </select>
                <select class="translation" id="language" aria-label=i18n::tr(lang, "Idioma de la aplicación", "Application language")>
                    <option value="es">"ES"</option>
                    <option value="en">"EN"</option>
                </select>
                <button class="filters-toggle" id="filters-toggle" type="button">{i18n::tr(lang, "Filtros", "Filters")}</button>
            </header>
            <div id="arcs-root" class="arcs-root">
            <div class="stage" id="stage">
                <canvas id="arcs"></canvas>
                <canvas id="axis"></canvas>
                <div class="legend" id="legend">
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
                </div>
                <div class="stats" id="stats"></div>
                <div class="hints">
                    <span>{i18n::tr(lang, "rueda para acercar", "scroll to zoom")}</span>
                    <span>{i18n::tr(lang, "arrastrar para desplazar", "drag to pan")}</span>
                    <span>{i18n::tr(lang, "clic para elegir", "click to select")}</span>
                </div>
                <div class="tooltip" id="tooltip" hidden></div>
                <div class="filters" id="filters" hidden>
                    <label class="filters-row">
                        <span>{i18n::tr(lang, "Referencias con al menos", "References with at least")}</span>
                        <output id="votes-value">"4"</output>
                        <span>{i18n::tr(lang, "votos", "votes")}</span>
                    </label>
                    <input id="votes" type="range" min="0" max="100" step="1" value="4" />
                    <p class="filters-note" id="filter-count"></p>
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
            <footer>
                "© 2026 · "
                {i18n::tr(lang, "Datos: ", "Data: ")}
                <a href="https://www.openbible.info/labs/cross-references/" target="_blank">"OpenBible.info"</a>
                {i18n::tr(
                    lang,
                    " (CC BY 4.0), reindexados · Texto: Platense (Straubinger, dominio público) y ASV (dominio público).",
                    " (CC BY 4.0), reindexed · Text: Platense (Straubinger, public domain) and ASV (public domain).",
                )}
            </footer>
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
