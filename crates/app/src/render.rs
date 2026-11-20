//! Arc diagram, axis overlay and interactions.
//!
//! The GPU geometry is evaluated in the vertex shader from per-link
//! endpoints, so panning and zooming only update uniforms. Frames are drawn
//! on demand: an idle diagram costs no GPU time. Arcs use adaptive levels of
//! detail (4–32 segments) and the additive glow accumulates at half
//! resolution, which is what makes 153k links interactive on integrated GPUs.

use std::cell::RefCell;
use std::io::Read;
use std::mem::size_of;
use std::rc::Rc;

use bytemuck::{Pod, Zeroable};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement, PointerEvent, WheelEvent,
};

use bible_core::format::{TranslationText, XrefData};
use bible_core::words::WordsData;
use bible_core::{VERSE_COUNT, Versification, book_from_query, normalize_query, tables};

use crate::axis::{self, Layout};
use crate::i18n::{self, Lang};

const BUCKETS: [usize; 7] = [4, 6, 8, 12, 16, 24, 32];
const MIN_VOTES: i16 = 4;
const MIN_VIEW_SPAN: f32 = 24.0;
const INTRO_MS: f64 = 800.0;
const SURFACE: [f32; 3] = [13.0 / 255.0, 13.0 / 255.0, 13.0 / 255.0];
const TOTAL_VERSES: u32 = VERSE_COUNT;
/// Safety cap for the hover/focus highlight ribbons.
const MAX_HIGHLIGHT: usize = 8192;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    t: f32,
    cos_t: f32,
    sin_t: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    a: f32,
    b: f32,
    color_a: u32,
    color_b: u32,
    weight: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RibbonVertex {
    t: f32,
    side: f32,
    cos_t: f32,
    sin_t: f32,
}

/// One published cross-reference with everything the UI needs (GPU endpoint,
/// colours, and the human coordinates used by the panel).
#[derive(Clone, Copy)]
struct Link {
    source: u32,
    start: u32,
    end: u32,
    votes: i16,
    a: f32,
    b: f32,
    low: f32,
    high: f32,
    color_a: u32,
    color_b: u32,
    weight: f32,
}

impl Link {
    fn instance(self) -> Instance {
        Instance {
            a: self.a,
            b: self.b,
            color_a: self.color_a,
            color_b: self.color_b,
            weight: self.weight,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ArcUniforms {
    clip_scale: [f32; 2],
    clip_offset: [f32; 2],
    view: [f32; 2],
    axis: [f32; 2],
    along_scale: f32,
    baseline: f32,
    max_height: f32,
    alpha: f32,
    vertical: f32,
    steady: f32,
    reveal: f32,
    /// Ribbon width in CSS px and its alpha (hover/focus highlights).
    width: f32,
    ribbon_alpha: f32,
    pad1: f32,
    pad2: f32,
    pad3: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlowUniforms {
    surface: [f32; 3],
    ceiling: f32,
    dim: f32,
    inv_scale: f32,
    pad: [f32; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilterState {
    /// `Some(0)` = Old Testament, `Some(1)` = New Testament.
    pub testament: Option<u8>,
    pub genre: Option<u8>,
    pub book: Option<u8>,
}

impl FilterState {
    pub fn is_active(self) -> bool {
        self.testament.is_some() || self.genre.is_some() || self.book.is_some()
    }

    /// Filters apply to the source verse: "references from this book/genre".
    pub fn matches_source(self, verse: u32, versification: &Versification) -> bool {
        let Some(reference) = versification.ref_at(verse) else {
            return true;
        };
        if let Some(book) = self.book {
            if reference.book != usize::from(book) {
                return false;
            }
        }
        if let Some(genre) = self.genre {
            if usize::from(tables::BOOK_GENRES[reference.book]) != usize::from(genre) {
                return false;
            }
        }
        if let Some(testament) = self.testament {
            let is_old = reference.book < 39;
            if is_old != (testament == 0) {
                return false;
            }
        }
        true
    }
}

pub enum Command {
    Votes(i16),
    Filters(FilterState),
    Focus(Option<(u32, u32)>),
    Search(String),
    ShowOriginal,
    OriginalLoaded(Result<WordsData, String>),
}

thread_local! {
    static COMMANDS: RefCell<Vec<Command>> = const { RefCell::new(Vec::new()) };
}

pub fn send(command: Command) {
    COMMANDS.with(|commands| commands.borrow_mut().push(command));
}

struct Shared {
    width: f32,
    height: f32,
    dpr: f32,
    layout: Layout,
    view: (f32, f32),
    dragging: bool,
    last_x: f32,
    last_y: f32,
    down_x: f32,
    down_y: f32,
    down_time: f64,
    scene_dirty: bool,
    overlay_dirty: bool,
    accumulate_dirty: bool,
    hover: Option<(u32, u32)>,
    hover_verse: Option<u32>,
    focus: Option<(u32, u32)>,
    /// Hover/focus range whose links are currently drawn as ribbons.
    highlighted: Option<(u32, u32)>,
    votes: i16,
    filters: FilterState,
    last_input: f64,
    original: Option<Rc<WordsData>>,
    original_loading: bool,
    original_error: Option<String>,
    show_original: bool,
}

impl Shared {
    fn mark_scene(&mut self, now: f64) {
        self.scene_dirty = true;
        self.overlay_dirty = true;
        self.accumulate_dirty = true;
        self.last_input = now;
    }

    fn mark_overlay(&mut self) {
        self.overlay_dirty = true;
    }

    fn pixels_per_verse(&self) -> f32 {
        self.layout.pixels_per_verse(self.view)
    }

    fn zoom_at(&mut self, factor: f32, anchor_px: f32, now: f64) {
        let span = self.view.1 - self.view.0;
        let ppv = self.pixels_per_verse();
        if span <= 0.0 || !factor.is_finite() || ppv <= 0.0 {
            return;
        }
        let anchor = self.view.0 + (anchor_px - self.layout.axis_start) / ppv;
        let next_span =
            (span / factor).clamp(MIN_VIEW_SPAN.min(TOTAL_VERSES as f32), TOTAL_VERSES as f32);
        let share = ((anchor - self.view.0) / span).clamp(0.0, 1.0);
        let start = (anchor - share * next_span).clamp(0.0, TOTAL_VERSES as f32 - next_span);
        if (start - self.view.0).abs() > 1e-4 || (next_span - span).abs() > 1e-4 {
            self.view = (start, start + next_span);
            self.mark_scene(now);
        }
    }

    fn pan_px(&mut self, delta_px: f32, now: f64) {
        let span = self.view.1 - self.view.0;
        let ppv = self.pixels_per_verse();
        if span <= 0.0 || ppv <= 0.0 || delta_px == 0.0 {
            return;
        }
        let delta = -delta_px / ppv;
        let start = (self.view.0 + delta).clamp(0.0, TOTAL_VERSES as f32 - span);
        if (start - self.view.0).abs() > 1e-4 {
            self.view = (start, start + span);
            self.mark_scene(now);
        }
    }
}

struct Glow {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

#[derive(Clone, Copy, Default)]
struct BucketRange {
    instances: u32,
    byte_offset: u64,
}

struct Lod {
    ranges: [BucketRange; BUCKETS.len()],
    ppv: f32,
    built: bool,
}

/// Segment bucket for an arc whose endpoints are `span_px` pixels apart.
/// Chord error of a half-ellipse with n segments is a*pi^2/(8n^2), with
/// a = half the pixel width, so this keeps it under ~1.2 px.
fn bucket_of(span_px: f32) -> usize {
    let half_width = (span_px * 0.5).max(0.5);
    let needed = (half_width * std::f32::consts::PI * std::f32::consts::PI / 9.6)
        .sqrt()
        .ceil() as usize;
    BUCKETS
        .iter()
        .position(|segments| needed <= *segments)
        .unwrap_or(BUCKETS.len() - 1)
}

#[derive(Default)]
struct Stats {
    shown: usize,
    total: usize,
    connected: usize,
    cross: usize,
    out_counts: Vec<u32>,
    in_counts: Vec<u32>,
}

pub fn set_status(text: &str) {
    if let Some(element) = element("status") {
        element.set_text_content(Some(text));
        element.set_class_name("status");
    }
}

pub fn set_status_error(text: &str) {
    if let Some(element) = element("status") {
        element.set_text_content(Some(text));
        element.set_class_name("status error");
    }
}

fn element(id: &str) -> Option<web_sys::Element> {
    web_sys::window()?.document()?.get_element_by_id(id)
}

fn performance_now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or(0.0)
}

fn votes_from_url(window: &web_sys::Window) -> i16 {
    i18n::query_param(window, "votes")
        .and_then(|value| value.parse::<i16>().ok())
        .map(|votes| votes.clamp(0, 100))
        .unwrap_or(MIN_VOTES)
}

fn filters_from_url(window: &web_sys::Window) -> FilterState {
    FilterState {
        testament: match i18n::query_param(window, "testament").as_deref() {
            Some("ot") => Some(0),
            Some("nt") => Some(1),
            _ => None,
        },
        genre: i18n::query_param(window, "genre")
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|genre| usize::from(*genre) < tables::GENRE_NAMES.len()),
        book: i18n::query_param(window, "book")
            .and_then(|value| {
                tables::BOOK_IDS
                    .iter()
                    .position(|candidate| *candidate == value)
            })
            .map(|book| book as u8),
    }
}

fn update_filter_url(window: &web_sys::Window, filters: FilterState, votes: i16) {
    let mut owned: Vec<(&str, String)> = Vec::new();
    match filters.testament {
        Some(0) => owned.push(("testament", "ot".into())),
        Some(1) => owned.push(("testament", "nt".into())),
        _ => {}
    }
    if let Some(genre) = filters.genre {
        owned.push(("genre", genre.to_string()));
    }
    if let Some(book) = filters.book {
        owned.push(("book", tables::BOOK_IDS[usize::from(book)].to_string()));
    }
    if votes != MIN_VOTES {
        owned.push(("votes", votes.to_string()));
    }
    let set: Vec<(&str, &str)> = owned
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    let url = i18n::url_with(window, &set, &["testament", "genre", "book", "votes"]);
    if let Ok(history) = window.history() {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&url));
    }
}

fn show_toast(message: &str) {
    let Some(toast) = element("toast") else {
        return;
    };
    toast.set_text_content(Some(message));
    let _ = toast.remove_attribute("hidden");
    if let Some(window) = web_sys::window() {
        let handler = Closure::<dyn FnMut()>::new(move || {
            if let Some(toast) = element("toast") {
                let _ = toast.set_attribute("hidden", "");
            }
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            handler.as_ref().unchecked_ref(),
            2400,
        );
        handler.forget();
    }
}

fn share_current() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let url = window.location().href().unwrap_or_default();
    let title = window
        .document()
        .map(|document| document.title())
        .unwrap_or_else(|| "Aletheia · Referencias cruzadas".into());
    let navigator = window.navigator();
    let share = js_sys::Reflect::get(navigator.as_ref(), &JsValue::from_str("share"))
        .ok()
        .and_then(|value| value.dyn_into::<js_sys::Function>().ok());
    if let Some(share) = share {
        let data = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &data,
            &JsValue::from_str("title"),
            &JsValue::from_str(&title),
        );
        let _ = js_sys::Reflect::set(
            &data,
            &JsValue::from_str("text"),
            &JsValue::from_str(i18n::tr(
                i18n::lang(),
                "Referencias cruzadas de la Biblia",
                "Bible cross references",
            )),
        );
        let _ = js_sys::Reflect::set(&data, &JsValue::from_str("url"), &JsValue::from_str(&url));
        if let Ok(result) = share.call1(&navigator, &data) {
            if let Ok(promise) = result.dyn_into::<js_sys::Promise>() {
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = JsFuture::from(promise).await;
                });
            }
        }
        return;
    }

    let lang = i18n::lang();
    let clipboard = navigator.clipboard();
    let fallback = url.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match JsFuture::from(clipboard.write_text(&url)).await {
            Ok(_) => show_toast(i18n::tr(lang, "Enlace copiado", "Link copied")),
            Err(_) if legacy_copy(&fallback) => {
                show_toast(i18n::tr(lang, "Enlace copiado", "Link copied"))
            }
            Err(_) => show_toast(i18n::tr(
                lang,
                "No se pudo copiar el enlace",
                "Could not copy the link",
            )),
        }
    });
}

/// Clipboard fallback for browsers without the async clipboard API.
fn legacy_copy(text: &str) -> bool {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return false;
    };
    let Some(body) = document.body() else {
        return false;
    };
    let Ok(node) = document.create_element("textarea") else {
        return false;
    };
    let Ok(textarea) = node.dyn_into::<web_sys::HtmlTextAreaElement>() else {
        return false;
    };
    textarea.set_value(text);
    let _ = textarea.set_attribute(
        "style",
        "position:fixed;top:-1000px;left:-1000px;opacity:0;pointer-events:none",
    );
    if body.append_child(textarea.as_ref()).is_err() {
        return false;
    }
    textarea.select();
    let copied = js_sys::Reflect::get(document.as_ref(), &JsValue::from_str("execCommand"))
        .ok()
        .and_then(|function| function.dyn_into::<js_sys::Function>().ok())
        .and_then(|function| {
            function
                .call1(document.as_ref(), &JsValue::from_str("copy"))
                .ok()
        })
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let _ = textarea.remove();
    copied
}

pub async fn run() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let lang = i18n::lang_of(&window);
    document.set_title(i18n::tr(
        lang,
        "Referencias cruzadas de la Biblia · Aletheia",
        "Bible Cross References · Aletheia",
    ));
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("arcs")
        .ok_or_else(|| JsValue::from_str("missing #arcs canvas"))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("#arcs is not a canvas"))?;
    let overlay_canvas: HtmlCanvasElement = document
        .get_element_by_id("axis")
        .ok_or_else(|| JsValue::from_str("missing #axis canvas"))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("#axis is not a canvas"))?;
    let overlay: CanvasRenderingContext2d = overlay_canvas
        .get_context("2d")
        .map_err(|_| JsValue::from_str("2d context failed"))?
        .ok_or_else(|| JsValue::from_str("no 2d context"))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("bad 2d context"))?;

    let translation = translation_id(&window);
    let translation_url = format!("data/{translation}.bin");
    let (xrefs, verses) =
        futures_util::join!(fetch_bytes("data/xrefs.bin"), fetch_bytes(&translation_url));
    let xrefs = XrefData::parse(&xrefs?).map_err(JsValue::from)?;
    let text = TranslationText::parse(&verses?).map_err(JsValue::from)?;

    let threads_mode = query_param(&window, "view").as_deref() == Some("threads")
        || window
            .location()
            .pathname()
            .map(|path| path.ends_with("threads/"))
            .unwrap_or(false);
    if threads_mode {
        return crate::threads::run(xrefs, text).await;
    }

    let versification = Rc::new(Versification::kjv());
    let total_links = xrefs.link_count();
    let links_all = build_links(&xrefs, &versification);
    // `?perf=1` shows presented-frame pacing; the normal UI stays quiet.
    let perf = query_flag(&window, "perf");

    let dpr = window.device_pixel_ratio().min(2.0) as f32;
    let css_width = canvas.client_width().max(1) as f32;
    let css_height = canvas.client_height().max(1) as f32;
    let width = (css_width * dpr).round().max(1.0) as u32;
    let height = (css_height * dpr).round().max(1.0) as u32;
    canvas.set_width(width);
    canvas.set_height(height);
    overlay_canvas.set_width(width);
    overlay_canvas.set_height(height);

    let instance = wgpu::util::new_instance_with_webgpu_detection(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
    .await;
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|error| JsValue::from_str(&format!("surface: {error}")))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        })
        .await
        .map_err(|error| JsValue::from_str(&format!("adapter: {error}")))?;
    let info = adapter.get_info();
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("bible-xrefs"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::Performance,
            ..Default::default()
        })
        .await
        .map_err(|error| JsValue::from_str(&format!("device: {error}")))?;

    device.on_uncaptured_error(std::sync::Arc::new(|error| {
        web_sys::console::error_1(&JsValue::from_str(&format!("wgpu error: {error}")));
    }));
    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|format| !format.is_srgb())
        .unwrap_or(caps.formats[0]);
    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        color_space: wgpu::SurfaceColorSpace::Auto,
        width,
        height,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
    };
    surface.configure(&device, &config);

    let half = adapter
        .get_texture_format_features(wgpu::TextureFormat::Rgba16Float)
        .allowed_usages
        .contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
    let accum_format = if half {
        wgpu::TextureFormat::Rgba16Float
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let glow_scale = 0.5f32;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("arcs"),
        source: wgpu::ShaderSource::Wgsl(include_str!("arcs.wgsl").into()),
    });
    let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("glow composite"),
        source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
    });

    let arc_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("arcs uniforms"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let glow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("glow composite"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let arc_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("arcs layout"),
        bind_group_layouts: &[Some(&arc_layout)],
        immediate_size: 0,
    });
    let glow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("glow layout"),
        bind_group_layouts: &[Some(&glow_layout)],
        immediate_size: 0,
    });

    let arc_uniforms_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("arcs uniforms"),
        size: size_of::<ArcUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let arc_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("arcs uniforms"),
        layout: &arc_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: arc_uniforms_buffer.as_entire_binding(),
        }],
    });
    let glow_uniforms_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glow uniforms"),
        size: size_of::<GlowUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut glow_uniforms = GlowUniforms {
        surface: SURFACE,
        ceiling: 20.0,
        dim: 1.0,
        inv_scale: 1.0 / glow_scale,
        pad: [0.0; 2],
    };
    queue.write_buffer(&glow_uniforms_buffer, 0, bytemuck::bytes_of(&glow_uniforms));
    let mut glow = create_glow(
        &device,
        &glow_layout,
        &glow_uniforms_buffer,
        scaled_extent(width, height, glow_scale),
        accum_format,
    );

    let mut vertices = Vec::with_capacity(BUCKETS.iter().sum::<usize>() * 2);
    let mut template_offsets = [(0u32, 0u32); BUCKETS.len()];
    for (bucket, segments) in BUCKETS.iter().copied().enumerate() {
        let first = vertices.len() as u32;
        for segment in 0..segments {
            let start = segment as f32 / segments as f32;
            let end = (segment + 1) as f32 / segments as f32;
            for t in [start, end] {
                let angle = t * std::f32::consts::PI;
                vertices.push(Vertex {
                    t,
                    cos_t: angle.cos(),
                    sin_t: angle.sin(),
                });
            }
        }
        template_offsets[bucket] = (first, (segments * 2) as u32);
    }
    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("arcs templates"),
        size: (vertices.len() * size_of::<Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&vertices));

    let ribbon_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ribbons"),
        source: wgpu::ShaderSource::Wgsl(include_str!("ribbon.wgsl").into()),
    });
    let arcs_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("arcs"),
        layout: Some(&arc_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 4,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 8,
                            shader_location: 2,
                        },
                    ],
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 0,
                            shader_location: 3,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 4,
                            shader_location: 4,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Unorm8x4,
                            offset: 8,
                            shader_location: 5,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Unorm8x4,
                            offset: 12,
                            shader_location: 6,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 16,
                            shader_location: 7,
                        },
                    ],
                }),
            ],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: accum_format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    let glow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("glow composite"),
        layout: Some(&glow_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &composite_shader,
            entry_point: Some("vs_full"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &composite_shader,
            entry_point: Some("fs_full"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    // Ribbon templates: two triangles per segment, with the side (±1) as a
    // vertex attribute so the fragment shader can anti-alias the edges.
    let ribbon_segments = 32usize;
    let mut ribbon_vertices = Vec::with_capacity(ribbon_segments * 6);
    for segment in 0..ribbon_segments {
        let t0 = segment as f32 / ribbon_segments as f32;
        let t1 = (segment + 1) as f32 / ribbon_segments as f32;
        let c0 = (t0 * std::f32::consts::PI).cos();
        let s0 = (t0 * std::f32::consts::PI).sin();
        let c1 = (t1 * std::f32::consts::PI).cos();
        let s1 = (t1 * std::f32::consts::PI).sin();
        for (t, cos_t, sin_t, side) in [
            (t0, c0, s0, -1.0),
            (t0, c0, s0, 1.0),
            (t1, c1, s1, -1.0),
            (t0, c0, s0, 1.0),
            (t1, c1, s1, 1.0),
            (t1, c1, s1, -1.0),
        ] {
            ribbon_vertices.push(RibbonVertex {
                t,
                side,
                cos_t,
                sin_t,
            });
        }
    }
    let ribbon_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ribbon template"),
        size: (ribbon_vertices.len() * size_of::<RibbonVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(
        &ribbon_vertex_buffer,
        0,
        bytemuck::cast_slice(&ribbon_vertices),
    );
    let ribbon_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ribbon instances"),
        size: (MAX_HIGHLIGHT * size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let ribbon_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ribbons"),
        layout: Some(&arc_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &ribbon_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<RibbonVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 4,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 8,
                            shader_location: 2,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 12,
                            shader_location: 3,
                        },
                    ],
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 0,
                            shader_location: 4,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 4,
                            shader_location: 5,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Unorm8x4,
                            offset: 8,
                            shader_location: 6,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Unorm8x4,
                            offset: 12,
                            shader_location: 7,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 16,
                            shader_location: 8,
                        },
                    ],
                }),
            ],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &ribbon_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    let layout = Layout::compute(css_width, css_height);
    let now = performance_now();
    let initial_votes = votes_from_url(&window);
    let initial_filters = filters_from_url(&window);
    let shared = Rc::new(RefCell::new(Shared {
        width: css_width,
        height: css_height,
        dpr,
        layout,
        view: (0.0, TOTAL_VERSES as f32),
        dragging: false,
        last_x: 0.0,
        last_y: 0.0,
        down_x: 0.0,
        down_y: 0.0,
        down_time: 0.0,
        scene_dirty: true,
        overlay_dirty: true,
        accumulate_dirty: true,
        hover: None,
        hover_verse: None,
        focus: None,
        highlighted: None,
        votes: initial_votes,
        filters: initial_filters,
        last_input: now,
        original: None,
        original_loading: false,
        original_error: None,
        show_original: false,
    }));

    let initial_active: Vec<Link> = links_all
        .iter()
        .filter(|link| {
            link.votes >= initial_votes
                && initial_filters.matches_source(link.source, &versification)
        })
        .copied()
        .collect();
    let stats = compute_stats(&initial_active, initial_votes, total_links, &versification);
    update_stats(&stats);
    attach_input(&canvas, &window, shared.clone(), versification.clone());
    attach_chrome(&shared);

    // URL contract: ?ref=Juan.3.16 focuses a passage on load.
    let mut invalid_reference = false;
    if let Some(reference) = query_param(&window, "ref") {
        if let Some((start, end)) = parse_reference(&reference, &versification) {
            apply_focus(&shared, start, end, &versification);
            open_panel(&shared, &links_all, &versification, &text, initial_votes);
        } else {
            invalid_reference = true;
            set_status(i18n::tr(
                lang,
                "No se reconoce esa referencia. Pruebe «Jn 3:16» o «Sal 23».",
                "Reference not recognized. Try “John 3:16” or “Ps 23”.",
            ));
        }
    }

    let mut lod = Lod {
        ranges: [BucketRange::default(); BUCKETS.len()],
        ppv: 0.0,
        built: false,
    };
    let instance_buffers = [
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("arcs instances 0"),
            size: (links_all.len().max(1) * size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("arcs instances 1"),
            size: (links_all.len().max(1) * size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
    ];
    let mut buffer_index = 0usize;
    let mut ordered: Vec<Instance> = Vec::with_capacity(links_all.len());
    let mut active_votes = i16::MIN;
    let mut active_filters = FilterState::default();
    let mut active: Vec<Link> = Vec::with_capacity(links_all.len());
    let mut force_rebuild = true;
    let mut accumulate_dirty = true;
    let mut highlight_count = 0usize;
    let mut highlight_votes = i16::MIN;
    let mut last_present = now;
    let mut frame_ms = 0.0f64;
    let mut last_report = 0.0f64;
    let start = now;
    let backend = backend_name(info.backend);

    web_sys::console::log_1(
        &format!(
            "wgpu: {} · {} · {} links · canvas {width}x{height} dpr {dpr}",
            info.name,
            backend,
            links_all.len()
        )
        .into(),
    );
    if !invalid_reference {
        set_status("");
    }

    loop {
        let frame_now = next_frame(&window).await?;
        let linear = ((frame_now - start) / INTRO_MS).clamp(0.0, 1.0);
        let reveal = 1.0 - (1.0 - linear).powi(3);
        let intro = linear < 1.0;

        // Commands from the UI.
        let commands: Vec<Command> =
            COMMANDS.with(|commands| commands.borrow_mut().drain(..).collect());
        for command in commands {
            match command {
                Command::Votes(votes) => {
                    shared.borrow_mut().votes = votes;
                    if let Some(value) = element("votes-value") {
                        value.set_text_content(Some(&votes.to_string()));
                    }
                    let filters = shared.borrow().filters;
                    update_filter_url(&window, filters, votes);
                }
                Command::Filters(filters) => {
                    shared.borrow_mut().filters = filters;
                    shared.borrow_mut().mark_scene(frame_now);
                    force_rebuild = true;
                    let votes = shared.borrow().votes;
                    update_filter_url(&window, filters, votes);
                }
                Command::Focus(passage) => {
                    if let Some((start, end)) = passage {
                        apply_focus(&shared, start, end, &versification);
                    } else {
                        shared.borrow_mut().focus = None;
                    }
                    open_panel(
                        &shared,
                        &links_all,
                        &versification,
                        &text,
                        shared.borrow().votes,
                    );
                }
                Command::Search(query) => {
                    if let Some((start, end)) = parse_reference(&query, &versification) {
                        apply_focus(&shared, start, end, &versification);
                        open_panel(
                            &shared,
                            &links_all,
                            &versification,
                            &text,
                            shared.borrow().votes,
                        );
                        force_rebuild = true;
                        set_status("");
                    } else {
                        let lang = i18n::lang();
                        set_status(i18n::tr(
                            lang,
                            "No se reconoce esa referencia. Pruebe «Jn 3:16» o «Sal 23».",
                            "Reference not recognized. Try “John 3:16” or “Ps 23”.",
                        ));
                    }
                }
                Command::ShowOriginal => {
                    let (loaded, loading) = {
                        let state = shared.borrow();
                        (state.original.is_some(), state.original_loading)
                    };
                    if loaded {
                        shared.borrow_mut().show_original = true;
                        if shared.borrow().focus.is_some() {
                            open_panel(
                                &shared,
                                &links_all,
                                &versification,
                                &text,
                                shared.borrow().votes,
                            );
                        }
                    } else if !loading {
                        {
                            let mut state = shared.borrow_mut();
                            state.show_original = true;
                            state.original_loading = true;
                            state.original_error = None;
                        }
                        if shared.borrow().focus.is_some() {
                            open_panel(
                                &shared,
                                &links_all,
                                &versification,
                                &text,
                                shared.borrow().votes,
                            );
                        }
                        wasm_bindgen_futures::spawn_local(async move {
                            let result = load_original().await;
                            send(Command::OriginalLoaded(result));
                        });
                    }
                }
                Command::OriginalLoaded(result) => {
                    let (focus, votes) = {
                        let mut state = shared.borrow_mut();
                        state.original_loading = false;
                        match result {
                            Ok(words) => {
                                state.original = Some(Rc::new(words));
                                state.original_error = None;
                                state.show_original = true;
                            }
                            Err(error) => {
                                state.original_error = Some(error);
                            }
                        }
                        (state.focus, state.votes)
                    };
                    if focus.is_some() {
                        open_panel(&shared, &links_all, &versification, &text, votes);
                    }
                }
            }
        }

        let votes = shared.borrow().votes;
        let filters = shared.borrow().filters;
        if votes != active_votes || filters != active_filters {
            active_votes = votes;
            active_filters = filters;
            active.clear();
            active.extend(
                links_all
                    .iter()
                    .filter(|link| {
                        link.votes >= votes && filters.matches_source(link.source, &versification)
                    })
                    .copied(),
            );
            let stats = compute_stats(&active, votes, total_links, &versification);
            update_stats(&stats);
            force_rebuild = true;
            shared.borrow_mut().mark_scene(frame_now);
            if shared.borrow().focus.is_some() {
                open_panel(&shared, &links_all, &versification, &text, votes);
            }
        }

        let css_width = canvas.client_width().max(1) as f32;
        let css_height = canvas.client_height().max(1) as f32;
        let mut state = shared.borrow_mut();
        let resized =
            (css_width - state.width).abs() > 0.5 || (css_height - state.height).abs() > 0.5;
        if resized {
            state.width = css_width;
            state.height = css_height;
            state.layout = Layout::compute(css_width, css_height);
            let width = (css_width * state.dpr).round().max(1.0) as u32;
            let height = (css_height * state.dpr).round().max(1.0) as u32;
            canvas.set_width(width);
            canvas.set_height(height);
            overlay_canvas.set_width(width);
            overlay_canvas.set_height(height);
            config.width = width;
            config.height = height;
            surface.configure(&device, &config);
            glow = create_glow(
                &device,
                &glow_layout,
                &glow_uniforms_buffer,
                scaled_extent(width, height, glow_scale),
                accum_format,
            );
            state.mark_scene(frame_now);
            accumulate_dirty = true;
        }
        let ppv = state.pixels_per_verse();
        let view = state.view;
        // Every filtered link is drawn at all times; zooming never rebuilds
        // the instance set. Only after a gesture settles does a large zoom
        // refine the tessellation once.
        let settled = frame_now - state.last_input > 160.0;
        let needs_lod =
            force_rebuild || (settled && lod.built && (ppv > lod.ppv * 2.0 || ppv < lod.ppv * 0.5));
        let scene_dirty = state.scene_dirty || intro || needs_lod;
        if state.accumulate_dirty || intro || needs_lod {
            accumulate_dirty = true;
            state.accumulate_dirty = false;
        }
        let overlay_dirty = state.overlay_dirty || intro || needs_lod;
        let focus = state.focus;
        let hover = state.hover;
        state.scene_dirty = false;
        state.overlay_dirty = false;
        drop(state);

        if needs_lod {
            force_rebuild = false;
            let mut counts = [0u32; BUCKETS.len()];
            let visible = active.len();
            for link in &active {
                counts[bucket_of((link.high - link.low) * ppv)] += 1;
            }
            let mut ranges = [BucketRange::default(); BUCKETS.len()];
            let mut cursors = [0u32; BUCKETS.len()];
            let mut offset = 0usize;
            for (bucket, range) in ranges.iter_mut().enumerate() {
                *range = BucketRange {
                    instances: counts[bucket],
                    byte_offset: (offset * size_of::<Instance>()) as u64,
                };
                cursors[bucket] = offset as u32;
                offset += counts[bucket] as usize;
            }
            ordered.clear();
            ordered.resize(visible, Instance::zeroed());
            for link in &active {
                let bucket = bucket_of((link.high - link.low) * ppv);
                let slot = &mut cursors[bucket];
                ordered[*slot as usize] = link.instance();
                *slot += 1;
            }
            buffer_index = (buffer_index + 1) % instance_buffers.len();
            if visible > 0 {
                queue.write_buffer(
                    &instance_buffers[buffer_index],
                    0,
                    bytemuck::cast_slice(&ordered),
                );
            }
            glow_uniforms.ceiling = (visible as f32 / 250.0).max(20.0);
            queue.write_buffer(&glow_uniforms_buffer, 0, bytemuck::bytes_of(&glow_uniforms));
            lod = Lod {
                ranges,
                ppv,
                built: true,
            };
        }
        if needs_lod {
            accumulate_dirty = true;
        }
        let highlight = focus.or(hover);
        let highlight_changed =
            highlight != shared.borrow().highlighted || highlight_votes != votes;
        if highlight_changed {
            highlight_votes = votes;
            let instances = collect_highlight(&active, highlight);
            highlight_count = instances.len();
            if highlight_count > 0 {
                queue.write_buffer(&ribbon_instance_buffer, 0, bytemuck::cast_slice(&instances));
            }
            shared.borrow_mut().highlighted = highlight;
            shared.borrow_mut().scene_dirty = true;
        }
        let scene_dirty = scene_dirty || highlight_changed;

        if scene_dirty {
            let state = shared.borrow();
            let axis = [state.layout.axis_start, state.layout.axis_end];
            let along_scale = (axis[1] - axis[0]) / (view.1 - view.0).max(f32::EPSILON);
            let mut arc_uniforms = ArcUniforms {
                clip_scale: [2.0 / state.width, -2.0 / state.height],
                clip_offset: [-1.0, 1.0],
                view: [view.0, view.1],
                axis,
                along_scale,
                baseline: state.layout.baseline,
                max_height: state.layout.max_height,
                alpha: 1.0,
                vertical: if state.layout.vertical { 1.0 } else { 0.0 },
                steady: if reveal >= 1.0 { 1.0 } else { 0.0 },
                reveal: reveal as f32,
                width: if focus.is_some() { 1.9 } else { 1.6 },
                ribbon_alpha: if focus.is_some() { 0.95 } else { 0.85 },
                pad1: 0.0,
                pad2: 0.0,
                pad3: 0.0,
            };
            drop(state);
            let fade: f32 = if reveal >= 1.0 {
                1.0
            } else {
                let f = (reveal as f32 / 0.5).min(1.0);
                f * f * (3.0 - 2.0 * f)
            };
            arc_uniforms.alpha = fade / glow_scale;
            queue.write_buffer(&arc_uniforms_buffer, 0, bytemuck::bytes_of(&arc_uniforms));
            glow_uniforms.dim = if focus.is_some() { 0.28 } else { 1.0 };
            queue.write_buffer(&glow_uniforms_buffer, 0, bytemuck::bytes_of(&glow_uniforms));

            let surface_texture = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(texture)
                | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
                wgpu::CurrentSurfaceTexture::Outdated => {
                    surface.configure(&device, &config);
                    continue;
                }
                _ => continue,
            };
            let surface_view = surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("arcs"),
            });
            if accumulate_dirty {
                accumulate_dirty = false;
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("accumulate"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &glow.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&arcs_pipeline);
                pass.set_bind_group(0, &arc_bind_group, &[]);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                for (bucket, range) in lod.ranges.iter().enumerate() {
                    if range.instances == 0 {
                        continue;
                    }
                    let (first, count) = template_offsets[bucket];
                    pass.set_vertex_buffer(
                        1,
                        instance_buffers[buffer_index].slice(range.byte_offset..),
                    );
                    pass.draw(first..first + count, 0..range.instances);
                }
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("composite"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &surface_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&glow_pipeline);
                pass.set_bind_group(0, &glow.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if highlight_count > 0 {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("highlight ribbons"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &surface_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&ribbon_pipeline);
                pass.set_bind_group(0, &arc_bind_group, &[]);
                pass.set_vertex_buffer(0, ribbon_vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, ribbon_instance_buffer.slice(..));
                pass.draw(0..(ribbon_segments * 6) as u32, 0..highlight_count as u32);
            }
            queue.submit(Some(encoder.finish()));
            queue.present(surface_texture);
            if perf {
                let present = performance_now();
                let interval = (present - last_present).max(0.1);
                last_present = present;
                frame_ms = if frame_ms == 0.0 {
                    interval
                } else {
                    frame_ms * 0.75 + interval * 0.25
                };
                if present - last_report > 500.0 {
                    last_report = present;
                    set_status(&format!(
                        "{:.0} fps · {:.1} ms · {} {}",
                        if frame_ms > 0.0 {
                            1000.0 / frame_ms
                        } else {
                            0.0
                        },
                        frame_ms,
                        lod.ranges.iter().map(|range| range.instances).sum::<u32>(),
                        i18n::tr(lang, "visibles", "visible")
                    ));
                }
            }
        }

        if overlay_dirty {
            let state = shared.borrow();
            let painter = state.layout;
            axis::paint(
                &overlay,
                &painter,
                view,
                &versification,
                hover,
                focus,
                state.dpr as f64,
            );
            update_tooltip(&state, &versification, &stats);
            drop(state);
        }
    }
}

fn apply_focus(shared: &Rc<RefCell<Shared>>, start: u32, end: u32, versification: &Versification) {
    let mut state = shared.borrow_mut();
    state.focus = Some((start, end));
    // Keep the current span and pan only when the passage is outside it.
    // Selecting a verse never destroys the full-Bible view.
    let (view_start, view_end) = state.view;
    let span = view_end - view_start;
    if (start as f32) < view_start - 0.5 || (end as f32 + 1.0) > view_end + 0.5 {
        let center = (start as f32 + end as f32 + 1.0) / 2.0;
        let next_start = (center - span / 2.0).clamp(0.0, TOTAL_VERSES as f32 - span);
        state.view = (next_start, next_start + span);
    }
    state.mark_scene(performance_now());
    if let Some(reference) = versification.ref_at(start) {
        if let Some(window) = web_sys::window() {
            let reference = format!(
                "{}.{}.{}",
                tables::BOOK_IDS[reference.book],
                reference.chapter,
                reference.verse
            );
            let url = i18n::url_with(&window, &[("ref", &reference)], &[]);
            if let Ok(history) = window.history() {
                let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&url));
            }
        }
    }
}

fn open_panel(
    shared: &Rc<RefCell<Shared>>,
    links: &[Link],
    versification: &Versification,
    text: &TranslationText,
    votes: i16,
) {
    let (focus, filters, original, show_original, loading, error) = {
        let state = shared.borrow();
        (
            state.focus,
            state.filters,
            state.original.clone(),
            state.show_original,
            state.original_loading,
            state.original_error.clone(),
        )
    };
    let Some((start, end)) = focus else {
        if let Some(panel) = element("panel") {
            let _ = panel.set_attribute("hidden", "");
        }
        return;
    };
    let html = panel_html(
        start,
        end,
        links,
        versification,
        text,
        votes,
        filters,
        original.as_deref(),
        show_original,
        loading,
        error.as_deref(),
    );
    if let Some(panel) = element("panel") {
        panel.set_inner_html(&html);
        let _ = panel.remove_attribute("hidden");
    }
}

#[allow(clippy::too_many_arguments)]
fn panel_html(
    start: u32,
    end: u32,
    links: &[Link],
    versification: &Versification,
    text: &TranslationText,
    votes: i16,
    filters: FilterState,
    original: Option<&WordsData>,
    show_original: bool,
    original_loading: bool,
    original_error: Option<&str>,
) -> String {
    let lang = i18n::lang();
    let (Some(first), Some(last)) = (versification.ref_at(start), versification.ref_at(end)) else {
        return String::new();
    };
    let title = if start == end {
        i18n::format_verse(lang, versification, start)
    } else if first.book == last.book && first.chapter == last.chapter {
        i18n::format_reference(
            lang,
            first.book,
            first.chapter,
            first.verse,
            Some(last.verse),
        )
    } else {
        format!(
            "{} — {}",
            i18n::format_verse(lang, versification, start),
            i18n::format_verse(lang, versification, end)
        )
    };
    let genre = usize::from(versification.genre_of_verse(start));
    let testament = i18n::testament(lang, first.book);

    let mut body = String::new();
    for verse in start..=end.min(start + 5) {
        if let Some(content) = text.text(verse) {
            let content = if content.trim().is_empty() {
                format!(
                    "<span class=\"verse-empty\">{}</span>",
                    i18n::empty_verse(lang)
                )
            } else {
                escape(content)
            };
            body.push_str(&format!(
                "<p class=\"panel-verse\"><span class=\"panel-verse-ref\">{}</span>{}</p>",
                escape(&i18n::format_verse(lang, versification, verse)),
                content
            ));
        }
    }

    let mut outgoing: Vec<&Link> = links
        .iter()
        .filter(|link| {
            link.votes >= votes
                && filters.matches_source(link.source, versification)
                && link.source >= start
                && link.source <= end
        })
        .collect();
    outgoing.sort_by(|a, b| b.votes.cmp(&a.votes));
    let outgoing_total = outgoing.len();
    outgoing.truncate(12);

    let mut incoming: Vec<&Link> = links
        .iter()
        .filter(|link| {
            link.votes >= votes
                && filters.matches_source(link.source, versification)
                && link.start <= end
                && link.end >= start
        })
        .collect();
    incoming.sort_by(|a, b| b.votes.cmp(&a.votes));
    let incoming_total = incoming.len();
    incoming.truncate(12);

    let mut html = format!(
        "<header class=\"panel-head\"><div><span class=\"panel-kicker\"><i style=\"background:{}\"></i>{} · {}</span><h2>{}</h2></div><button id=\"panel-close\" class=\"panel-close\" type=\"button\" aria-label=\"{}\">×</button></header>",
        axis::GENRE_HEX[genre],
        escape(i18n::genre_name(lang, genre)),
        testament,
        escape(&title),
        i18n::tr(lang, "Cerrar", "Close")
    );
    html.push_str(&body);
    html.push_str(&original_section(
        lang,
        start,
        end,
        versification,
        original,
        show_original,
        original_loading,
        original_error,
    ));
    html.push_str(&section(
        lang,
        i18n::tr(lang, "Remite a", "Refers to"),
        i18n::tr(
            lang,
            "adónde apunta este pasaje",
            "where this passage points",
        ),
        outgoing_total,
        &outgoing,
        versification,
        text,
    ));
    html.push_str(&section(
        lang,
        i18n::tr(lang, "Citado por", "Cited by"),
        i18n::tr(lang, "pasajes que apuntan aquí", "passages that point here"),
        incoming_total,
        &incoming,
        versification,
        text,
    ));
    html
}

#[allow(clippy::too_many_arguments)]
fn original_section(
    lang: Lang,
    start: u32,
    end: u32,
    versification: &Versification,
    original: Option<&WordsData>,
    show: bool,
    loading: bool,
    error: Option<&str>,
) -> String {
    let mut html = String::from("<section class=\"panel-section original-section\">");
    let book = versification
        .ref_at(start)
        .map(|reference| reference.book)
        .unwrap_or(0);
    let language = original
        .and_then(|words| words.verse_language(start))
        .unwrap_or(if book < 39 { 0 } else { 2 });
    let (language_name, language_attr) = match language {
        1 => (i18n::tr(lang, "arameo", "Aramaic"), "arc"),
        2 => (i18n::tr(lang, "griego", "Greek"), "grc"),
        _ => (i18n::tr(lang, "hebreo", "Hebrew"), "he"),
    };
    html.push_str(&format!(
        "<div class=\"original-head\"><h3>{} · <span>{}</span></h3>",
        i18n::tr(lang, "Texto original", "Original text"),
        language_name
    ));
    if !show {
        html.push_str(&format!(
            "<button class=\"original-load\" type=\"button\" data-original>{}</button>",
            i18n::tr(lang, "Mostrar texto original", "Show original text")
        ));
        html.push_str("</div>");
        html.push_str(&format!(
            "<p class=\"panel-note\">{}</p>",
            i18n::tr(
                lang,
                "Hebreo, arameo o griego con léxico y morfología. Se descarga una vez (5,4 MB).",
                "Hebrew, Aramaic or Greek with lexicon and morphology. Downloaded once (5.4 MB)."
            )
        ));
    } else if loading {
        html.push_str("</div>");
        html.push_str(&format!(
            "<p class=\"panel-note original-loading\">{}</p>",
            i18n::tr(lang, "Cargando texto original…", "Loading original text…")
        ));
    } else if let Some(error) = error {
        html.push_str("</div>");
        html.push_str(&format!("<p class=\"panel-error\">{}</p>", escape(error)));
        html.push_str(&format!(
            "<button class=\"original-load\" type=\"button\" data-original>{}</button>",
            i18n::tr(lang, "Reintentar", "Retry")
        ));
    } else if let Some(words) = original {
        html.push_str("</div>");
        for verse in start..=end.min(start + 5) {
            let reference = i18n::format_verse(lang, versification, verse);
            html.push_str("<div class=\"original-verse\">");
            html.push_str(&format!(
                "<span class=\"original-ref\">{}</span>",
                escape(&reference)
            ));
            if words.verse_is_fallback(verse) {
                html.push_str(&format!(
                    "<p class=\"original-fallback\">{}</p>",
                    i18n::tr(
                        lang,
                        "Texto bizantino/TR (ausente en NA28).",
                        "Byzantine/TR text (absent from NA28)."
                    )
                ));
            }
            let verse_words = words.words_in_verse(verse);
            if verse_words.is_empty() {
                html.push_str(&format!(
                    "<p class=\"panel-empty\">{}</p>",
                    i18n::tr(
                        lang,
                        "No incluido en el texto crítico.",
                        "Not in the critical text."
                    )
                ));
            } else {
                html.push_str(&format!(
                    "<div class=\"original-words\" dir=\"{}\" lang=\"{}\">",
                    if language == 2 { "ltr" } else { "rtl" },
                    language_attr
                ));
                for word in verse_words {
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
                html.push_str("</div>");
            }
            html.push_str("</div>");
        }
        html.push_str(
            "<div id=\"lex-card\" class=\"lex-card\" role=\"region\" aria-live=\"polite\" hidden></div>",
        );
        let source = match language {
            0 | 1 => i18n::tr(lang, "Texto: TAHOT (WLC)", "Text: TAHOT (WLC)"),
            _ => i18n::tr(
                lang,
                "Texto: TAGNT (SBLGNT–NA28)",
                "Text: TAGNT (SBLGNT–NA28)",
            ),
        };
        html.push_str(&format!(
            "<p class=\"original-source\">{} · {}</p>",
            source,
            i18n::tr(
                lang,
                "Léxico: STEPBible, CC BY 4.0.",
                "Lexicon: STEPBible, CC BY 4.0."
            )
        ));
    } else {
        html.push_str("</div>");
        html.push_str(&format!(
            "<button class=\"original-load\" type=\"button\" data-original>{}</button>",
            i18n::tr(lang, "Mostrar texto original", "Show original text")
        ));
    }
    html.push_str("</section>");
    html
}

fn show_lexicon(shared: &Rc<RefCell<Shared>>, strong_id: u32, surface: &str, morph: &str) {
    let Some(original) = shared.borrow().original.clone() else {
        return;
    };
    let Some(entry) = original.lex(strong_id) else {
        return;
    };
    let lang = i18n::lang();
    let count = original.occurrence_count(strong_id);
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

    html.push_str(&format!(
        "<p class=\"lex-gloss\"><span class=\"lex-label\">{}</span> {}</p>",
        i18n::tr(lang, "Glosa", "Gloss"),
        if entry.gloss.is_empty() {
            "—".to_string()
        } else {
            escape(entry.gloss)
        }
    ));
    let category = lex_category(entry.morph, lang);
    if !category.is_empty() && description.is_empty() {
        html.push_str(&format!(
            "<p class=\"lex-morph\"><span class=\"lex-label\">{}</span> {}</p>",
            i18n::tr(lang, "Categoría", "Category"),
            category
        ));
    }
    if !entry.meaning.is_empty() {
        html.push_str(&format!(
            "<p class=\"lex-meaning\">{}</p>",
            escape(entry.meaning)
        ));
    } else {
        html.push_str(&format!(
            "<p class=\"panel-note\">{}</p>",
            i18n::tr(
                lang,
                "Sin definición en el léxico.",
                "No lexicon definition."
            )
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
    html.push_str("<div id=\"lex-occurrences\"></div>");

    if let Some(card) = element("lex-card") {
        card.set_inner_html(&html);
        let _ = card.remove_attribute("hidden");
    }
    if let Some(panel) = element("panel") {
        if let Ok(nodes) = panel.query_selector_all(".word") {
            for index in 0..nodes.length() {
                if let Some(node) = nodes
                    .item(index)
                    .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
                {
                    let selected = node
                        .get_attribute("data-strong")
                        .and_then(|value| value.parse::<u32>().ok())
                        == Some(strong_id);
                    if selected {
                        let _ = node.set_attribute("data-selected", "true");
                    } else {
                        let _ = node.remove_attribute("data-selected");
                    }
                }
            }
        }
    }
}

fn show_occurrences(shared: &Rc<RefCell<Shared>>, strong_id: u32) {
    let Some(original) = shared.borrow().original.clone() else {
        return;
    };
    let lang = i18n::lang();
    let versification = Versification::kjv();
    let all = original.occurrences(strong_id);
    let total = all.len();
    let mut html = String::new();
    if total == 0 {
        html.push_str(&format!(
            "<p class=\"panel-note\">{}</p>",
            i18n::tr(lang, "Sin ocurrencias.", "No occurrences.")
        ));
    } else {
        html.push_str(&format!(
            "<p class=\"lex-occurrences-title\">{} {}</p>",
            total,
            i18n::tr(lang, "ocurrencias", "occurrences")
        ));
        html.push_str("<div class=\"lex-occurrences-list\">");
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
    }
    if let Some(node) = element("lex-occurrences") {
        node.set_inner_html(&html);
    }
}

fn morph_description(code: &str, hebrew: bool) -> String {
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
            out.push_str(greek_number(chars[1], lang));
        } else if chars.len() >= 3 {
            out.push(' ');
            out.push_str(greek_case(chars[0], lang));
            out.push(' ');
            out.push_str(greek_number(chars[1], lang));
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
        out.push_str(greek_number(tail_chars[1], lang));
        out.push(' ');
        out.push_str(greek_gender(tail_chars[2], lang));
    } else if tail_chars.len() >= 2 && tail_chars[0].is_ascii_digit() {
        out.push(' ');
        out.push_str(greek_person(tail_chars[0], lang));
        out.push(' ');
        out.push_str(greek_number(tail_chars[1], lang));
    }
    out.trim().to_string()
}

fn lex_category(code: &str, lang: Lang) -> &'static str {
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

fn greek_number(code: char, _lang: Lang) -> &'static str {
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

async fn load_original() -> Result<WordsData, String> {
    let bytes = fetch_bytes("data/words.bin.gz")
        .await
        .map_err(|error| format!("{error:?}"))?;
    // Servers normally hand over the gzip bytes, but a proxy or a future
    // uncompressed file may already be decoded.
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
    WordsData::parse(&raw)
}

fn section(
    lang: Lang,
    title: &str,
    note: &str,
    total: usize,
    items: &[&Link],
    versification: &Versification,
    text: &TranslationText,
) -> String {
    let mut html = format!(
        "<section class=\"panel-section\"><h3>{} <span>{}</span></h3><p class=\"panel-note\">{}</p>",
        title, total, note
    );
    if items.is_empty() {
        html.push_str(&format!(
            "<p class=\"panel-empty\">{}</p></section>",
            i18n::tr(lang, "Ninguno", "None")
        ));
        return html;
    }
    for link in items {
        let (book, chapter) = versification
            .ref_at(link.start)
            .map(|r| (r.book, r.chapter))
            .unwrap_or((0, 1));
        let last = versification.ref_at(link.end).map(|r| r.verse).unwrap_or(1);
        let first = versification
            .ref_at(link.start)
            .map(|r| r.verse)
            .unwrap_or(1);
        let reference = i18n::format_reference(lang, book, chapter, first, Some(last));
        let genre = usize::from(versification.genre_of_verse(link.start));
        let body = text
            .text(link.start)
            .map(|content| truncate(content, 150))
            .unwrap_or_default();
        html.push_str(&format!(
            "<div class=\"panel-item\"><div class=\"panel-item-head\"><span class=\"panel-item-ref\"><i style=\"background:{}\"></i>{}</span><span class=\"panel-item-votes\">{} {}</span></div><p>{}</p></div>",
            axis::GENRE_HEX[genre],
            escape(&reference),
            link.votes,
            i18n::tr(lang, "votos", "votes"),
            escape(&body)
        ));
    }
    html.push_str("</section>");
    html
}

fn update_stats(stats: &Stats) {
    let lang = i18n::lang();
    if let Some(node) = element("stats") {
        let share = if TOTAL_VERSES == 0 {
            0.0
        } else {
            stats.connected as f64 * 100.0 / f64::from(TOTAL_VERSES)
        };
        let share = format!("{:.1}", (share * 10.0).round() / 10.0);
        let share = match lang {
            Lang::Es => share.replace('.', ","),
            Lang::En => share,
        };
        node.set_inner_html(&match lang {
            Lang::Es => format!(
                "<div><strong>{}</strong> versículos · <strong>{}</strong> de {} referencias mostradas</div>\
                 <div><strong>{}</strong> % de los versículos conectados · <strong>{}</strong> referencias entre los Testamentos</div>",
                thousands(TOTAL_VERSES as usize),
                thousands(stats.shown),
                thousands(stats.total),
                share,
                thousands(stats.cross)
            ),
            Lang::En => format!(
                "<div><strong>{}</strong> verses · <strong>{}</strong> of {} references shown</div>\
                 <div><strong>{}</strong>% of verses connected · <strong>{}</strong> cross-testament references</div>",
                thousands(TOTAL_VERSES as usize),
                thousands(stats.shown),
                thousands(stats.total),
                share,
                thousands(stats.cross)
            ),
        });
    }
    if let Some(node) = element("filter-count") {
        node.set_text_content(Some(&format!(
            "{} {} {} {}",
            thousands(stats.shown),
            i18n::tr(lang, "de", "of"),
            thousands(stats.total),
            i18n::tr(lang, "referencias", "references")
        )));
    }
}

fn compute_stats(links: &[Link], votes: i16, total: usize, versification: &Versification) -> Stats {
    let nt_start = versification
        .book_range(39)
        .map(|(start, _)| start)
        .unwrap_or(0);
    let mut mark = vec![false; TOTAL_VERSES as usize];
    let mut out_counts = vec![0u32; TOTAL_VERSES as usize];
    let mut diff = vec![0i32; TOTAL_VERSES as usize + 1];
    let mut cross = 0usize;
    for link in links.iter().filter(|link| link.votes >= votes) {
        mark[link.source as usize] = true;
        out_counts[link.source as usize] += 1;
        diff[link.start as usize] += 1;
        diff[link.end as usize + 1] -= 1;
        let source_nt = link.source >= nt_start;
        let target_nt = link.start >= nt_start;
        if source_nt != target_nt {
            cross += 1;
        }
    }
    let mut running = 0i32;
    let mut in_counts = vec![0u32; TOTAL_VERSES as usize];
    for verse in 0..TOTAL_VERSES as usize {
        running += diff[verse];
        in_counts[verse] = running.max(0) as u32;
        if running > 0 {
            mark[verse] = true;
        }
    }
    Stats {
        shown: links.iter().filter(|link| link.votes >= votes).count(),
        total,
        connected: mark.iter().filter(|value| **value).count(),
        cross,
        out_counts,
        in_counts,
    }
}

fn update_tooltip(state: &Shared, versification: &Versification, stats: &Stats) {
    let Some(tooltip) = element("tooltip") else {
        return;
    };
    let Some(verse) = state.hover_verse else {
        let _ = tooltip.set_attribute("hidden", "");
        return;
    };
    let lang = i18n::lang();
    let label = i18n::format_verse(lang, versification, verse);
    let count = stats.out_counts.get(verse as usize).copied().unwrap_or(0)
        + stats.in_counts.get(verse as usize).copied().unwrap_or(0);
    tooltip.set_inner_html(&format!(
        "<strong>{}</strong><span>{} {}</span>",
        escape(&label),
        count,
        i18n::tr(lang, "referencias", "references")
    ));
    let x = state.last_x + 14.0;
    let y = state.last_y + 14.0;
    let _ = tooltip.set_attribute("style", &format!("left:{x}px; top:{y}px;"));
    let _ = tooltip.remove_attribute("hidden");
}

fn attach_input(
    canvas: &HtmlCanvasElement,
    window: &web_sys::Window,
    shared: Rc<RefCell<Shared>>,
    versification: Rc<Versification>,
) {
    let down = {
        let shared = shared.clone();
        let canvas = canvas.clone();
        Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            if event.pointer_type() == "mouse" && event.button() != 0 {
                return;
            }
            let rect = canvas.get_bounding_client_rect();
            let x = event.client_x() as f32 - rect.left() as f32;
            let y = event.client_y() as f32 - rect.top() as f32;
            {
                let mut state = shared.borrow_mut();
                state.dragging = true;
                state.last_x = x;
                state.last_y = y;
                state.down_x = x;
                state.down_y = y;
                state.down_time = performance_now();
                state.hover = None;
                state.hover_verse = None;
                state.mark_overlay();
            }
            if let Some(tooltip) = element("tooltip") {
                let _ = tooltip.set_attribute("hidden", "");
            }
            canvas.set_pointer_capture(event.pointer_id()).ok();
            event.prevent_default();
        })
    };
    let up = {
        let shared = shared.clone();
        let canvas = canvas.clone();
        let versification = versification.clone();
        Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            let rect = canvas.get_bounding_client_rect();
            let x = event.client_x() as f32 - rect.left() as f32;
            let y = event.client_y() as f32 - rect.top() as f32;
            let click = {
                let state = shared.borrow();
                let moved = ((x - state.down_x).powi(2) + (y - state.down_y).powi(2)).sqrt();
                state.dragging && moved < 6.0 && performance_now() - state.down_time < 450.0
            };
            if click {
                let state = shared.borrow();
                let passage = state.layout.along_of(x, y);
                let ppv = state.pixels_per_verse();
                let verse = state.layout.along_to_verse(state.view, passage);
                drop(state);
                if let Some((start, end)) = passage_at(verse, ppv, &versification) {
                    shared.borrow_mut().focus = Some((start, end));
                    send(Command::Focus(Some((start, end))));
                }
            }
            shared.borrow_mut().dragging = false;
            event.prevent_default();
        })
    };
    let moved = {
        let shared = shared.clone();
        let canvas = canvas.clone();
        let versification = versification.clone();
        Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            let rect = canvas.get_bounding_client_rect();
            let x = event.client_x() as f32 - rect.left() as f32;
            let y = event.client_y() as f32 - rect.top() as f32;
            let mut state = shared.borrow_mut();
            if state.dragging {
                let delta = if state.layout.vertical {
                    y - state.last_y
                } else {
                    x - state.last_x
                };
                state.last_x = x;
                state.last_y = y;
                state.pan_px(delta, performance_now());
                return;
            }
            let along = state.layout.along_of(x, y);
            let inside = along >= state.layout.axis_start && along <= state.layout.axis_end;
            let verse = state.layout.along_to_verse(state.view, along);
            let ppv = state.pixels_per_verse();
            let passage = if inside {
                passage_at(verse, ppv, &versification)
            } else {
                None
            };
            let changed = passage != state.hover;
            state.hover = passage;
            state.hover_verse = if inside {
                Some((verse.floor() as u32).min(TOTAL_VERSES - 1))
            } else {
                None
            };
            state.mark_overlay();
            state.last_x = x;
            state.last_y = y;
            let _ = changed;
        })
    };
    let leave = {
        let shared = shared.clone();
        Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let mut state = shared.borrow_mut();
            state.hover = None;
            state.hover_verse = None;
            state.mark_overlay();
        })
    };
    let wheel = {
        let shared = shared.clone();
        let canvas = canvas.clone();
        Closure::<dyn FnMut(WheelEvent)>::new(move |event: WheelEvent| {
            let rect = canvas.get_bounding_client_rect();
            let x = event.client_x() as f32 - rect.left() as f32;
            let y = event.client_y() as f32 - rect.top() as f32;
            let mut state = shared.borrow_mut();
            let along = state.layout.along_of(x, y);
            let pixels = match event.delta_mode() {
                1 => event.delta_y() as f32 * 16.0,
                2 => event.delta_y() as f32 * 400.0,
                _ => event.delta_y() as f32,
            };
            let factor = (-pixels * if event.ctrl_key() { 0.01 } else { 0.002 }).exp();
            state.zoom_at(factor, along, performance_now());
            event.prevent_default();
        })
    };
    let resize = {
        let shared = shared.clone();
        Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            shared.borrow_mut().mark_scene(performance_now());
        })
    };

    let _ = canvas.add_event_listener_with_callback("pointerdown", down.as_ref().unchecked_ref());
    let _ = canvas.add_event_listener_with_callback("pointerup", up.as_ref().unchecked_ref());
    let _ = canvas.add_event_listener_with_callback("pointermove", moved.as_ref().unchecked_ref());
    let _ = window.add_event_listener_with_callback("pointerup", up.as_ref().unchecked_ref());
    let _ = canvas.add_event_listener_with_callback("pointerleave", leave.as_ref().unchecked_ref());
    let _ = canvas.add_event_listener_with_callback("wheel", wheel.as_ref().unchecked_ref());
    let _ = window.add_event_listener_with_callback("resize", resize.as_ref().unchecked_ref());
    down.forget();
    up.forget();
    moved.forget();
    leave.forget();
    wheel.forget();
    resize.forget();
}

pub(crate) fn attach_translation_select(window: &web_sys::Window) {
    let Some(select) = window
        .document()
        .and_then(|document| document.get_element_by_id("translation"))
        .and_then(|node| node.dyn_into::<web_sys::HtmlSelectElement>().ok())
    else {
        return;
    };
    select.set_value(translation_id(window));
    let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::HtmlSelectElement>().ok())
        else {
            return;
        };
        if let Some(window) = web_sys::window() {
            let url = i18n::url_with(&window, &[("t", &target.value())], &[]);
            let _ = window.location().set_href(&url);
        }
    });
    let _ = select.add_event_listener_with_callback("change", handler.as_ref().unchecked_ref());
    handler.forget();
}

fn set_range_progress(range: &HtmlInputElement, votes: i16) {
    let _ = range.set_attribute(
        "style",
        &format!("--range-progress:{}%", votes.clamp(0, 100)),
    );
}

fn set_filters_active(active: bool) {
    if let Some(toggle) = element("filters-toggle") {
        if active {
            let _ = toggle.set_attribute("class", "filters-toggle has-filters");
        } else {
            let _ = toggle.set_attribute("class", "filters-toggle");
        }
    }
}

fn read_filters(document: &web_sys::Document) -> FilterState {
    let value = |id: &str| {
        document
            .get_element_by_id(id)
            .and_then(|node| node.dyn_into::<web_sys::HtmlSelectElement>().ok())
            .map(|select| select.value())
            .unwrap_or_default()
    };
    FilterState {
        testament: match value("filter-testament").as_str() {
            "ot" => Some(0),
            "nt" => Some(1),
            _ => None,
        },
        genre: value("filter-genre")
            .parse::<u8>()
            .ok()
            .filter(|genre| usize::from(*genre) < tables::GENRE_NAMES.len()),
        book: tables::BOOK_IDS
            .iter()
            .position(|candidate| *candidate == value("filter-book"))
            .map(|book| book as u8),
    }
}

pub(crate) fn attach_share(window: &web_sys::Window) {
    let Some(share) = window
        .document()
        .and_then(|document| document.get_element_by_id("share"))
    else {
        return;
    };
    let handler = Closure::<dyn FnMut()>::new(share_current);
    let _ = share.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
    handler.forget();
}

fn attach_chrome(shared: &Rc<RefCell<Shared>>) {
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();

    if let Some(toggle) = document.get_element_by_id("filters-toggle") {
        let handler = Closure::<dyn FnMut()>::new(move || {
            if let Some(filters) = element("filters") {
                if filters.has_attribute("hidden") {
                    let _ = filters.remove_attribute("hidden");
                    if let Some(toggle) = element("filters-toggle") {
                        let _ = toggle.set_attribute("aria-expanded", "true");
                    }
                } else {
                    let _ = filters.set_attribute("hidden", "");
                    if let Some(toggle) = element("filters-toggle") {
                        let _ = toggle.set_attribute("aria-expanded", "false");
                    }
                }
            }
        });
        let _ = toggle.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
        handler.forget();
    }
    if let Some(close) = document.get_element_by_id("filters-close") {
        let handler = Closure::<dyn FnMut()>::new(move || {
            if let Some(filters) = element("filters") {
                let _ = filters.set_attribute("hidden", "");
            }
            if let Some(toggle) = element("filters-toggle") {
                let _ = toggle.set_attribute("aria-expanded", "false");
            }
        });
        let _ = close.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
        handler.forget();
    }

    let filters = shared.borrow().filters;
    set_filters_active(filters.is_active());
    if let Some(select) = document
        .get_element_by_id("filter-testament")
        .and_then(|node| node.dyn_into::<web_sys::HtmlSelectElement>().ok())
    {
        select.set_value(match filters.testament {
            Some(0) => "ot",
            Some(1) => "nt",
            _ => "",
        });
    }
    if let Some(select) = document
        .get_element_by_id("filter-genre")
        .and_then(|node| node.dyn_into::<web_sys::HtmlSelectElement>().ok())
    {
        select.set_value(
            &filters
                .genre
                .map(|genre| genre.to_string())
                .unwrap_or_default(),
        );
    }
    if let Some(select) = document
        .get_element_by_id("filter-book")
        .and_then(|node| node.dyn_into::<web_sys::HtmlSelectElement>().ok())
    {
        select.set_value(
            &filters
                .book
                .map(|book| tables::BOOK_IDS[usize::from(book)].to_string())
                .unwrap_or_default(),
        );
    }
    for id in ["filter-testament", "filter-genre", "filter-book"] {
        let Some(select) = document
            .get_element_by_id(id)
            .and_then(|node| node.dyn_into::<web_sys::HtmlSelectElement>().ok())
        else {
            continue;
        };
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                let filters = read_filters(&document);
                set_filters_active(filters.is_active());
                send(Command::Filters(filters));
            }
        });
        let _ = select.add_event_listener_with_callback("change", handler.as_ref().unchecked_ref());
        handler.forget();
    }

    let votes = shared.borrow().votes;
    let range = document
        .get_element_by_id("votes")
        .and_then(|node| node.dyn_into::<HtmlInputElement>().ok());
    if let Some(range) = &range {
        range.set_value(&votes.to_string());
        set_range_progress(range, votes);
    }
    if let Some(label) = element("votes-value") {
        label.set_text_content(Some(&votes.to_string()));
    }
    if let Some(range) = range {
        let shared = shared.clone();
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Some(target) = event
                .target()
                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
            {
                if let Ok(value) = target.value().parse::<i16>() {
                    if let Some(label) = element("votes-value") {
                        label.set_text_content(Some(&value.to_string()));
                    }
                    set_range_progress(&target, value);
                    send(Command::Votes(value));
                    shared.borrow_mut().mark_scene(performance_now());
                }
            }
        });
        let _ = range.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref());
        handler.forget();
    }

    if let Some(reset) = document.get_element_by_id("filters-reset") {
        let handler = Closure::<dyn FnMut()>::new(move || {
            if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                for id in ["filter-testament", "filter-genre", "filter-book"] {
                    if let Some(select) = document
                        .get_element_by_id(id)
                        .and_then(|node| node.dyn_into::<web_sys::HtmlSelectElement>().ok())
                    {
                        select.set_value("");
                    }
                }
                if let Some(range) = document
                    .get_element_by_id("votes")
                    .and_then(|node| node.dyn_into::<HtmlInputElement>().ok())
                {
                    range.set_value(&MIN_VOTES.to_string());
                    set_range_progress(&range, MIN_VOTES);
                }
                if let Some(label) = element("votes-value") {
                    label.set_text_content(Some(&MIN_VOTES.to_string()));
                }
            }
            set_filters_active(false);
            send(Command::Filters(FilterState::default()));
            send(Command::Votes(MIN_VOTES));
        });
        let _ = reset.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
        handler.forget();
    }

    attach_share(&window);
    attach_translation_select(&window);
    i18n::attach_language_select(&window);
    if let Some(search) = document
        .get_element_by_id("search")
        .and_then(|node| node.dyn_into::<HtmlInputElement>().ok())
    {
        let handler = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if event.key() == "Enter" {
                    event.prevent_default();
                    if let Some(target) = event
                        .target()
                        .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
                    {
                        send(Command::Search(target.value()));
                    }
                }
            },
        );
        let _ =
            search.add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
        handler.forget();
    }

    if let Some(panel) = document.get_element_by_id("panel") {
        let shared = shared.clone();
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
            if let Ok(Some(_)) = target.closest("#panel-close") {
                shared.borrow_mut().focus = None;
                shared.borrow_mut().mark_scene(performance_now());
                if let Some(panel) = element("panel") {
                    let _ = panel.set_attribute("hidden", "");
                }
                return;
            }
            if let Ok(Some(_)) = target.closest("[data-original]") {
                send(Command::ShowOriginal);
                return;
            }
            if let Ok(Some(button)) = target.closest("[data-strong]") {
                let strong = button
                    .get_attribute("data-strong")
                    .and_then(|value| value.parse::<u32>().ok());
                let surface = button.get_attribute("data-surface").unwrap_or_default();
                let morph = button.get_attribute("data-morph").unwrap_or_default();
                if let Some(strong) = strong {
                    show_lexicon(&shared, strong, &surface, &morph);
                }
                return;
            }
            if let Ok(Some(button)) = target.closest("[data-occurrences]") {
                if let Some(id) = button
                    .get_attribute("data-occurrences")
                    .and_then(|value| value.parse::<u32>().ok())
                {
                    show_occurrences(&shared, id);
                }
                return;
            }
            if let Ok(Some(button)) = target.closest("[data-occurrence]") {
                if let Some(verse) = button
                    .get_attribute("data-occurrence")
                    .and_then(|value| value.parse::<u32>().ok())
                {
                    send(Command::Focus(Some((verse, verse))));
                }
            }
        });
        let _ = panel.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
        handler.forget();
    }
}

/// Links of the hovered/focused passage, in both directions, deduplicated by
/// construction (a link is added once even when both ends are inside).
fn collect_highlight(active: &[Link], range: Option<(u32, u32)>) -> Vec<Instance> {
    let Some((start, end)) = range else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(64);
    for link in active {
        let outgoing = link.source >= start && link.source <= end;
        let incoming = link.start <= end && link.end >= start;
        if outgoing || incoming {
            out.push(link.instance());
            if out.len() >= MAX_HIGHLIGHT {
                break;
            }
        }
    }
    out
}

fn passage_at(verse: f32, ppv: f32, versification: &Versification) -> Option<(u32, u32)> {
    let index = (verse.floor() as i64).clamp(0, i64::from(TOTAL_VERSES) - 1) as u32;
    let reference = versification.ref_at(index)?;
    if ppv < 4.0 {
        versification.chapter_range(reference.book, reference.chapter)
    } else {
        Some((index, index))
    }
}

pub(crate) fn parse_reference(input: &str, versification: &Versification) -> Option<(u32, u32)> {
    // OSIS references in URLs use dots (`John.3.16`); parse them first so the
    // separators are not lost when the query is normalized. Localized names
    // (`Juan.3.16`, `1 Cor.13.4`) are accepted too.
    if input.contains('.') {
        let parts: Vec<&str> = input.trim().split('.').collect();
        let book = parts.first().and_then(|id| {
            bible_core::book_index(id).or_else(|| {
                let key = normalize_query(id);
                book_from_query(&key).and_then(|(book, rest)| rest.is_empty().then_some(book))
            })
        });
        if let (Some(book), Some(chapter)) = (
            book,
            parts.get(1).and_then(|chapter| chapter.parse::<u16>().ok()),
        ) {
            let chapter = chapter.max(1).min(versification.chapters_of(book) as u16);
            let (chapter_start, chapter_end) = versification.chapter_range(book, chapter)?;
            return match parts.get(2).and_then(|verse| verse.parse::<u16>().ok()) {
                Some(verse) if verse > 0 => {
                    let index = versification.index_of(bible_core::VerseRef {
                        book,
                        chapter,
                        verse,
                    });
                    index.map(|index| (index, index))
                }
                _ => Some((chapter_start, chapter_end)),
            };
        }
        // A single dot may also separate chapter and verse (`Jn 3.16`).
        return parse_reference(&input.replace('.', ":"), versification);
    }
    let query = normalize_query(input);
    if query.is_empty() {
        return None;
    }
    let (book, rest) = book_from_query(&query)?;
    let rest = rest.trim_start_matches([':', '.', '-']);
    let (chapter_part, range_part) = match rest.split_once(':') {
        Some((chapter, range)) => (chapter, Some(range)),
        None => (rest, None),
    };
    let chapter: u16 = chapter_part.parse().unwrap_or(1);
    let chapter = chapter.max(1);
    let count = versification.chapters_of(book);
    if count == 0 {
        return None;
    }
    let chapter = chapter.min(count as u16);
    let (chapter_start, chapter_end) = versification.chapter_range(book, chapter)?;
    let Some(range) = range_part else {
        return Some((chapter_start, chapter_end));
    };
    let (start_part, end_part) = match range.split_once('-') {
        Some((start, end)) => (start, Some(end)),
        None => (range, None),
    };
    let start: u16 = start_part.parse().ok()?;
    if start == 0 {
        return None;
    }
    let end: u16 = end_part.and_then(|end| end.parse().ok()).unwrap_or(start);
    let index = |verse: u16| {
        versification.index_of(bible_core::VerseRef {
            book,
            chapter,
            verse,
        })
    };
    let from = index(start)?;
    let to = index(end.max(start)).unwrap_or(from);
    Some((
        from.min(chapter_end).max(chapter_start),
        to.min(chapter_end),
    ))
}

pub(crate) fn query_flag(window: &web_sys::Window, name: &str) -> bool {
    window
        .location()
        .search()
        .unwrap_or_default()
        .trim_start_matches('?')
        .split('&')
        .any(|pair| {
            pair == name
                || pair
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with('='))
        })
}

/// Translation file id from `?t=` (Platense by default in Spanish).
pub(crate) fn translation_id(window: &web_sys::Window) -> &'static str {
    i18n::translation_id(window)
}

pub(crate) fn query_param(window: &web_sys::Window, name: &str) -> Option<String> {
    i18n::query_param(window, name)
}

fn build_links(xrefs: &XrefData, versification: &Versification) -> Vec<Link> {
    let mut links = Vec::with_capacity(xrefs.link_count());
    for source in 0..xrefs.verse_count as usize {
        let start = xrefs.offsets[source] as usize;
        let end = xrefs.offsets[source + 1] as usize;
        let source_color = genre_color(versification.genre_of_verse(source as u32));
        for link in start..end {
            let target_start = xrefs.target_start[link];
            let target_end = xrefs.target_end[link];
            links.push(Link {
                source: source as u32,
                start: target_start,
                end: target_end,
                votes: xrefs.votes[link],
                a: source as f32 + 0.5,
                b: (target_start + target_end + 1) as f32 / 2.0,
                low: (source as f32 + 0.5).min((target_start + target_end + 1) as f32 / 2.0),
                high: (source as f32 + 0.5).max((target_start + target_end + 1) as f32 / 2.0),
                color_a: source_color,
                color_b: genre_color(versification.genre_of_verse(target_start)),
                weight: vote_weight(xrefs.votes[link]),
            });
        }
    }
    links
}

fn vote_weight(votes: i16) -> f32 {
    let votes = f32::from(votes.max(0));
    (0.2 + 0.8 * (1.0 + votes).log10() / 2.0).min(1.0)
}

fn genre_color(genre: u8) -> u32 {
    const HEX: [u32; 10] = [
        0xeb568f, 0xcd4805, 0xb48f05, 0xcd489d, 0x88a00d, 0x17a7b4, 0x119245, 0x0593e6, 0x03ac8f,
        0xa06aec,
    ];
    let rgb = HEX[usize::from(genre).min(HEX.len() - 1)];
    ((rgb >> 16) & 0xff) | (((rgb >> 8) & 0xff) << 8) | ((rgb & 0xff) << 16) | (0xff << 24)
}

fn scaled_extent(width: u32, height: u32, scale: f32) -> (u32, u32) {
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

fn create_glow(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
) -> Glow {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("glow accumulation"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glow composite"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: uniform_buffer.as_entire_binding(),
            },
        ],
    });
    Glow {
        texture,
        view,
        bind_group,
    }
}

fn backend_name(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::BrowserWebGpu => "WebGPU",
        wgpu::Backend::Gl => "WebGL2",
        wgpu::Backend::Metal => "Metal",
        wgpu::Backend::Vulkan => "Vulkan",
        wgpu::Backend::Dx12 => "DX12",
        wgpu::Backend::Noop => "noop",
    }
}

fn thousands(value: usize) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len() + text.len() / 3);
    for (index, ch) in text.chars().enumerate() {
        if index > 0 && (text.len() - index) % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out
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
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|error| JsValue::from_str(&format!("fetch {url}: {error:?}")))?;
    let response: web_sys::Response = response
        .dyn_into()
        .map_err(|_| JsValue::from_str("fetch did not return a Response"))?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!(
            "fetch {url}: HTTP {}",
            response.status()
        )));
    }
    let buffer = JsFuture::from(
        response
            .array_buffer()
            .map_err(|error| JsValue::from_str(&format!("fetch {url}: {error:?}")))?,
    )
    .await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

async fn next_frame(window: &web_sys::Window) -> Result<f64, JsValue> {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let callback = Closure::once(move |timestamp: JsValue| {
            let _ = resolve.call1(&JsValue::UNDEFINED, &timestamp);
        });
        let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
        callback.forget();
    });
    let value = JsFuture::from(promise).await?;
    Ok(value.as_f64().unwrap_or(0.0))
}
