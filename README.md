# Aletheia Arcs · Referencias cruzadas de la Biblia

Visualizador web de referencias cruzadas de la Biblia escrito en **Rust +
WebAssembly**, con **Leptos** para la interfaz y **wgpu** (WebGPU, con retroceso
automático a WebGL2) para el diagrama de arcos.

## Estado

- [x] Toolchain Rust + `wasm32-unknown-unknown` + Trunk.
- [x] Modelo canónico propio: 66 libros, 1.189 capítulos, 31.102 versículos
      (versificación KJV).
- [x] Formatos binarios propios y pipeline de datos desde OpenBible, la
      Platense (Straubinger) y la ASV.
- [x] Paridad de datos: 344.799 enlaces totales, 153.419 con votos ≥ 4,
      31.102 versículos de texto.
- [x] Diagrama de arcos en wgpu con geometría evaluada en el vertex shader,
      acumulación HDR y *tone mapping* logarítmico (glow).
- [x] Pan y zoom con rueda/arrastre, orientación vertical en pantallas
      verticales.
- [x] LOD por tramo, *culling* por ventana visible y render bajo demanda.
- [x] Eje con libros, capítulos, géneros y vista general; tooltips.
- [x] Escala cronológica por defecto: color por fecha aproximada de composición
      (no por la fecha de los hechos narrados), regla de años bajo el eje,
      leyenda plegable y tabla completa en «Acerca de». El modo por género sigue
      disponible en los filtros o con `?measure=genre`.
- [x] Página «Acerca de» con metodología, fuentes, licencias y las fechas de los
      66 libros; la atribución legal ya no ocupa el pie del diagrama.
- [x] Búsqueda de referencias, panel de referencias, filtro de votos y URL.
- [x] Vista de Hilos con columna vertebral, contadores por versículo,
      dirección (remite a / citado por / ambos), cabecera fija opaca y selector
      de capítulo con el mismo menú temático del resto del sitio.
- [x] Texto bíblico en español (Platense) e inglés (ASV), con selector de
      idioma y de versión; el idioma elige la traducción por defecto.
- [x] Filtros por testamento, género y libro (aplicados al versículo de
      origen), combinables con el umbral de votos y persistidos en la URL.
- [x] Selectores de versión, idioma, capítulo y filtros con menú temático propio
      y hoja inferior en móvil; salto directo a cualquier libro desde la barra
      superior (abre su primer capítulo en la vista actual) y, en el diagrama,
      pulsación larga para previsualizar un versículo y sus referencias.
- [x] Compartir el enlace actual con Web Share API y copia al portapapeles.
- [x] PWA instalable: manifest, iconos, *service worker* y caché offline.
- [x] SEO técnico: metadatos, Open Graph/Twitter, JSON-LD, `robots.txt`,
      `sitemap.xml` e imagen social. La página «Acerca de» también está indexada.
- [x] Texto original palabra por palabra: hebreo/arameo (TAHOT, base WLC) y
      griego (TAGNT, base SBLGNT–NA28), con número Strong, morfología y léxico
      breve (TBESH/TBESG). Se carga bajo demanda (5,6 MB gzip) y está disponible
      tanto en el panel de Arcos como en el lector de Hilos.
- [x] Glosas y acepciones en español alineadas de la Reina-Valera 1909
      (dominio público), con la definición completa de STEPBible en inglés como
      segunda capa. Cobertura: 13.768 de 16.316 lemas; el resto cae al inglés
      con un aviso explícito.
- [x] Filtro `?lemma=H2617A`: muestra y resalta en oro los arcos entre
      versículos que comparten un lema, con un aviso que permite quitarlo. Se
      combina con votos, testamento, género y libro. Con un pasaje enfocado, el
      aviso incluye una capa «Capítulo» para alternar entre solo el lema y lema
      más los arcos del capítulo. El hover se limita al pasaje enfocado; al
      recorrer un arco del lema identifica su ocurrencia más cercana.

### Siguiente fase

- Traducir las definiciones completas de STEPBible al español (hoy las glosas y
  acepciones ya son españolas, pero el artículo largo sigue en inglés).
- Ampliar la cobertura de glosas españolas para los lemas que la RV1909 no
  alinea (unos 2.500, en su mayoría nombres propios y formas raras).

## Compilar

```bash
# Dependencias del sistema: rustup, target wasm32, trunk
rustup target add wasm32-unknown-unknown
cargo install trunk   # o descargar el binario de trunk-rs/trunk

# Datos (requiere los archivos crudos en data/raw)
cargo run -p bible-data -- all data/raw crates/app/data

# Texto original (opcional): TAHOT/TAGNT/TBESH/TBESG en data/raw/stepbible
# y RV1909 en data/raw/spanish; simplemma mejora los lemas verbales españoles.
python3 -m pip install simplemma   # opcional, en un entorno virtual si hace falta
python3 data/import_stepbible.py

cargo run -p bible-data -- verify crates/app/data

# Servir en http://127.0.0.1:8080
cd crates/app
trunk serve
```

La página «Acerca de» vive en `crates/app/about/index.html`; el hook
`post_build` de `Trunk.toml` la copia a `dist/about/`.

`data/raw/cross_references.txt` se obtiene descomprimiendo
`openbible-cross-references.zip`; `data/raw/spaplatense.osis.xml` es el OSIS de
la Biblia Platense y `data/import_platense.py` lo convierte a
`data/raw/spaplatense_vpl.txt` (versificación KJV); `data/raw/eng-asv_vpl.txt`
es la ASV de eBible.org en formato versículo por línea.

Para el texto original, descargue de
<https://github.com/STEPBible/STEPBible-Data> los cuatro archivos TAHOT, los dos
TAGNT y los léxicos TBESH/TBESG en `data/raw/stepbible/`. El importador
`data/import_stepbible.py` los convierte en `crates/app/data/words.bin.gz`
(formato `WRD1`), alineado a la versificación KJV; el mapeo resuelve los títulos
de los salmos, Joel, Malaquías, 2 Corintios 13 y 3 Juan 14/15. El texto griego
sigue la base NA27/28 (SBLGNT/THGNT); si un versículo de la KJV no tiene
testimonio en NA (Hechos 8:37, Marcos 9:44/46), se usa el texto bizantino/TR.

Para las glosas y acepciones españolas, descargue el USFX de la Reina-Valera
1909 de <https://ebible.org/Scriptures/spaRV1909_usfx.zip> y descomprímalo en
`data/raw/spanish/spaRV1909_usfx.xml`. El importador alinea sus etiquetas
`<w s="H7225">` con las palabras TAHOT/TAGNT y guarda una glosa y hasta tres
acepciones por lema. Es una alineación de uso, no una traducción del artículo
completo de STEPBible.

La edición Platense no incluye Números 1:54, Marcos 9:44 y 9:46 ni Hechos 8:37
(este último también falta en la ASV); la interfaz los marca como «No incluido
en esta versión» en lugar de inventar texto.

## Publicar en GitHub Pages

```bash
scripts/deploy-pages.sh
```

El script compila `crates/app` con Trunk y publica `dist/` en la rama
`gh-pages` (incluidos los binarios de datos). En GitHub, la fuente de Pages debe
ser la rama `gh-pages`, carpeta `/`. `Trunk.toml` usa `public_url = "./"` para
que los recursos funcionen bajo la subruta del proyecto.
Sitio: <https://arcs.johan.cl/> (el repositorio técnico sigue siendo
`Aletheia-xrefs-rs`; GitHub Pages puede seguir sirviendo la misma compilación).

## Diseño

```
crates/core   dominio canónico, versificación, formatos binarios
crates/data   pipeline de datos (OpenBible, Platense, ASV, STEPBible) + verificación
crates/app    Leptos + wgpu + shaders WGSL
```

La interfaz usa un sistema de diseño propio en CSS: variables de color,
tipografía y espaciado, controles nativos estilizados y una serif editorial para
el texto bíblico. Tailwind puede integrarse con Trunk mediante su CLI, pero
`shadcn` es específico de React; para Leptos existen bibliotecas como Thaw o
leptonic. Aquí se evitó añadir otra capa para no duplicar estilos ni peso.

- **Coordenadas**: índice denso de versículo en orden KJV (`0..31102`).
- **Enlaces**: CSR por versículo fuente; cada enlace guarda rango destino y
  votos (`i16`). Orden: fuente, votos descendentes, destino.
- **Texto**: offsets UTF-8 por versículo en un blob compacto; el binario
  completo se carga una vez y se indexa sin copiar cadenas.
- **Original**: `WRD1` (versión 2) indexa 421.690 palabras por versículo; cada
  registro guarda superficie, Strong, morfología y lengua, y el léxico viaja en
  el mismo archivo comprimido, con glosa y acepciones españolas. Se descarga
  solo al pulsar «Mostrar texto original» o al abrir un enlace `?lemma=`.
- **GPU**: una instancia por enlace con `a`, `b`, géneros y peso. El vertex
  shader calcula el arco; pan/zoom solo actualizan uniformes.
- **Glow**: pasada aditiva a `RGBA16Float` + pasada de composición que comprime
  la luz con `log(1 + peak) / log(1 + ceiling)` preservando el tono.

## Rendimiento

Medido en Brave con WebGPU a 1440 px: unos 47–51 fps con las 153.419
referencias visibles, frente a los ~8 fps del visor original en la misma
máquina. El render es bajo demanda (sin trabajo de GPU cuando la vista no
cambia); `?perf=1` muestra fps, milisegundos por cuadro e instancias visibles.

## Fuentes de datos

- OpenBible.info: CC BY 4.0.
- Biblia Platense (Straubinger): dominio público.
- American Standard Version (ASV): dominio público.
- STEPBible Data (TAHOT, TAGNT, TBESH, TBESG), Tyndale House Cambridge /
  STEPBible.org: CC BY 4.0. TAHOT se basa en WLC; TAGNT incluye SBLGNT
  (CC BY 4.0) y NA27/28; TBESH/TBESG se basan en BDB y Abbott-Smith.
- Reina-Valera 1909 (USFX), eBible.org: dominio público. Se usa solo para
  alinear glosas y acepciones españolas.
- *Comentario Bíblico Adventista del Séptimo Día*, ed. 1980: referencia
  bibliográfica para las fechas aproximadas de composición. No se redistribuye
  su texto; solo se citan rangos de años.

Las licencias anteriores corresponden a los datos; el código es original.
