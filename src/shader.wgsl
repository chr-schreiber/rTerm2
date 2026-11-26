// rterm2: ein Shader für alles. Jede Instanz ist ein Rechteck – entweder einfarbig
// (uv.z == 0) oder ein Glyph aus dem Atlas (Abdeckung im Rot-Kanal).

struct Globals {
    screen: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) textured: f32,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,   // x, y, w, h in Pixeln
    @location(1) uv: vec4<f32>,     // u, v, w, h normiert (w == 0: einfarbig)
    @location(2) color: vec4<f32>,  // rgba, nicht vormultipliziert
) -> VsOut {
    // Triangle-Strip: (0,0) (1,0) (0,1) (1,1)
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let px = rect.xy + corner * rect.zw;
    let ndc = vec2<f32>(px.x / globals.screen.x * 2.0 - 1.0, 1.0 - px.y / globals.screen.y * 2.0);

    var out: VsOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = uv.xy + corner * uv.zw;
    out.color = color;
    out.textured = select(0.0, 1.0, uv.z > 0.0);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // textureSampleLevel statt textureSample: erlaubt auch außerhalb einheitlichen Kontrollflusses
    let coverage = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0).r;
    let alpha = in.color.a * mix(1.0, coverage, in.textured);
    return vec4<f32>(in.color.rgb, alpha);
}
