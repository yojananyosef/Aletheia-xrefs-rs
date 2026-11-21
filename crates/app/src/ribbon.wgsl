// Anti-aliased ribbons for the hovered/focused passage's links, drawn on top
// of the glow (ported from the reference `RIBBON_*` shaders).

struct Uniforms {
  clip_scale: vec2<f32>,
  clip_offset: vec2<f32>,
  view: vec2<f32>,
  axis: vec2<f32>,
  along_scale: f32,
  baseline: f32,
  max_height: f32,
  alpha: f32,
  vertical: f32,
  steady: f32,
  reveal: f32,
  width: f32,
  ribbon_alpha: f32,
  height_scale: f32,
  pad2: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsIn {
  @location(0) t: f32,
  @location(1) side: f32,
  @location(2) cos_t: f32,
  @location(3) sin_t: f32,
  @location(4) a: f32,
  @location(5) b: f32,
  @location(6) color_a: vec4<f32>,
  @location(7) color_b: vec4<f32>,
  @location(8) weight: f32,
};

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) side: f32,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
  let a = u.axis.x + (in.a - u.view.x) * u.along_scale;
  let b = u.axis.x + (in.b - u.view.x) * u.along_scale;
  let radius = 0.5 * (b - a);
  let height = abs(radius) * u.height_scale;
  let point_local = vec2<f32>(0.5 * (a + b) - radius * in.cos_t, height * in.sin_t);
  let tangent_local = vec2<f32>(radius * in.sin_t, height * in.cos_t);

  var point: vec2<f32>;
  var tangent: vec2<f32>;
  if (u.vertical < 0.5) {
    point = vec2<f32>(point_local.x, u.baseline - point_local.y);
    tangent = vec2<f32>(tangent_local.x, -tangent_local.y);
  } else {
    point = vec2<f32>(u.baseline + point_local.y, point_local.x);
    tangent = vec2<f32>(tangent_local.y, tangent_local.x);
  }

  var normal = vec2<f32>(0.0, 1.0);
  if (length(tangent) > 1e-4) {
    normal = normalize(vec2<f32>(-tangent.y, tangent.x));
  }
  let px = point + normal * in.side * u.width * 0.5;

  var out: VsOut;
  out.position = vec4<f32>(
    px.x * u.clip_scale.x + u.clip_offset.x,
    px.y * u.clip_scale.y + u.clip_offset.y,
    0.0,
    1.0,
  );
  out.color = vec4<f32>(
    mix(in.color_a.rgb, in.color_b.rgb, in.t) * mix(0.55, 1.0, in.weight),
    u.ribbon_alpha,
  );
  out.side = in.side;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let edge = 1.0 - smoothstep(0.5, 1.0, abs(in.side));
  return vec4<f32>(in.color.rgb, in.color.a * edge);
}
