struct GlowUniforms {
  surface: vec3<f32>,
  ceiling: f32,
  dim: f32,
  inv_scale: f32,
  pad0: f32,
  pad1: f32,
};

@group(0) @binding(0) var accumulated: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u: GlowUniforms;

@vertex
fn vs_full(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
  let xy = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
  return vec4<f32>(xy * 2.0 - 1.0, 0.0, 1.0);
}

fn texel_at(coord: vec2<i32>) -> vec3<f32> {
  let limit = vec2<i32>(textureDimensions(accumulated)) - vec2<i32>(1, 1);
  return textureLoad(accumulated, clamp(coord, vec2<i32>(0, 0), limit), 0).rgb;
}

// Bilinear fetch from the (possibly smaller) accumulation buffer. Sampling
// with four texelFetch calls keeps this independent of float-texture
// filtering support, which WebGL2 exposes only as an extension.
fn sample_glow(position: vec2<f32>) -> vec3<f32> {
  let p = position / u.inv_scale - vec2<f32>(0.5, 0.5);
  let base = floor(p);
  let f = p - base;
  let i = vec2<i32>(base);
  let c00 = texel_at(i);
  let c10 = texel_at(i + vec2<i32>(1, 0));
  let c01 = texel_at(i + vec2<i32>(0, 1));
  let c11 = texel_at(i + vec2<i32>(1, 1));
  return mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);
}

// Hue-preserving logarithmic tone mapping, ported from the reference glow
// compositor: each mark adds light; the brightest channel is compressed as
// log(1 + light) / log(1 + ceiling) and the other channels scale with it, so
// dense regions keep their texture and color instead of burning out to white.
@fragment
fn fs_full(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let light = sample_glow(position.xy);
  let peak = max(max(light.r, light.g), light.b);
  let intensity = clamp(log(1.0 + peak) / log(1.0 + u.ceiling), 0.0, 1.0) * u.dim;
  let glow = select(vec3<f32>(0.0), light / peak * intensity, peak > 0.0);
  return vec4<f32>(u.surface + glow * (1.0 - u.surface), 1.0);
}
