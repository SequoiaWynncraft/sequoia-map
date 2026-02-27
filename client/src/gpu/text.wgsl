// Dual-atlas text shader. Each pass samples one atlas (halo or fill).

struct Viewport {
    offset: vec2<f32>,
    scale: f32,
    _time: f32,
    resolution: vec2<f32>,
    _pad1: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> vp: Viewport;

@group(1) @binding(0)
var glyph_texture: texture_2d<f32>;

@group(1) @binding(1)
var glyph_sampler: sampler;

struct VertexInput {
    @location(0) quad_pos: vec2<f32>,
    @location(1) rect: vec4<f32>,
    @location(2) uv_rect: vec4<f32>,
    @location(3) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = in.rect.xy + in.quad_pos * in.rect.zw;
    let screen = world_pos * vp.scale + vp.offset;
    let ndc = screen / vp.resolution * 2.0 - 1.0;
    out.clip_position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = mix(in.uv_rect.xy, in.uv_rect.zw, in.quad_pos);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Soft-threshold sampled alpha — tight range for bold, readable glyphs at distance.
    let sampled_alpha = textureSample(glyph_texture, glyph_sampler, in.uv).a;
    let alpha = smoothstep(0.18, 0.52, sampled_alpha) * in.color.a;
    return vec4<f32>(in.color.rgb, alpha);
}
