struct Uniforms {
  /// 2 / viewport
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
  pad1: f32,
  pad2: f32,
  pad3: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsIn {
  @location(0) t: f32,
  @location(1) cos_t: f32,
  @location(2) sin_t: f32,
  @location(3) a: f32,
  @location(4) b: f32,
  @location(5) color_a: vec4<f32>,
  @location(6) color_b: vec4<f32>,
  @location(7) weight: f32,
};

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
};

fn along(verse: f32) -> f32 {
  return u.axis.x + (verse - u.view.x) * u.along_scale;
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
  // Once the intro is over (`steady = 1`) the template's baked cosine/sine
  // are used, so the hot vertex shader has no transcendentals and no
  // divisions: panning and zooming only touch uniforms.
  let t = select(min(in.t, u.reveal), in.t, u.steady > 0.5);
  var cos_t = in.cos_t;
  var sin_t = in.sin_t;
  if (u.steady < 0.5) {
    let theta = t * 3.141592653589793;
    cos_t = cos(theta);
    sin_t = sin(theta);
  }
  let a = along(in.a);
  let b = along(in.b);
  let radius = 0.5 * (b - a);
  let height = min(abs(radius), u.max_height);
  let local = vec2<f32>(0.5 * (a + b) - radius * cos_t, height * sin_t);

  // toScreen: horizontal axes lift the arc up, vertical axes push it right.
  var px: vec2<f32>;
  if (u.vertical < 0.5) {
    px = vec2<f32>(local.x, u.baseline - local.y);
  } else {
    px = vec2<f32>(u.baseline + local.y, local.x);
  }

  var out: VsOut;
  // clip = px * (2 / viewport) - 1, with y flipped.
  out.position = vec4<f32>(
    px.x * u.clip_scale.x + u.clip_offset.x,
    px.y * u.clip_scale.y + u.clip_offset.y,
    0.0,
    1.0,
  );
  out.color = vec4<f32>(mix(in.color_a.rgb, in.color_b.rgb, t) * in.weight, u.alpha);
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  return in.color;
}
