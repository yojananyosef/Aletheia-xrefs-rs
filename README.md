# Aletheia · Referencias cruzadas de la Biblia

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
- [x] Búsqueda de referencias, panel de referencias, filtro de votos y URL.
- [x] Vista de Hilos con columna vertebral, contadores por versículo y
      dirección (remite a / citado por / ambos).
- [x] Texto bíblico en español (Platense) e inglés (ASV), con selector de
      idioma y de versión; el idioma elige la traducción por defecto.
- [x] Filtros por testamento, género y libro (aplicados al versículo de
      origen), combinables con el umbral de votos y persistidos en la URL.
- [x] Compartir el enlace actual con Web Share API y copia al portapapeles.
- [x] PWA instalable: manifest, iconos, *service worker* y caché offline.
- [x] SEO técnico: metadatos, Open Graph/Twitter, JSON-LD, `robots.txt`,
      `sitemap.xml` e imagen social.
- [ ] Palabras originales (hebreo/griego con léxico y Strong): requiere un
      corpus textual distinto del actual; queda como siguiente fase.

## Compilar

```bash
# Dependencias del sistema: rustup, target wasm32, trunk
rustup target add wasm32-unknown-unknown
cargo install trunk   # o descargar el binario de trunk-rs/trunk

# Datos (requiere los archivos crudos en data/raw)
cargo run -p bible-data -- all data/raw crates/app/data
cargo run -p bible-data -- verify crates/app/data

# Servir en http://127.0.0.1:8080
cd crates/app
trunk serve
```

`data/raw/cross_references.txt` se obtiene descomprimiendo
`openbible-cross-references.zip`; `data/raw/spaplatense.osis.xml` es el OSIS de
la Biblia Platense y `data/import_platense.py` lo convierte a
`data/raw/spaplatense_vpl.txt` (versificación KJV); `data/raw/eng-asv_vpl.txt`
es la ASV de eBible.org en formato versículo por línea.

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
Sitio: <https://yojananyosef.github.io/Aletheia-xrefs-rs/>.

## Diseño

```
crates/core   dominio canónico, versificación, formatos binarios
crates/data   pipeline de datos (OpenBible, Platense, ASV) + verificación
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

Las licencias anteriores corresponden a los datos; el código es original.
