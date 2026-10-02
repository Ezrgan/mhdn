struct Screen {
    size: vec2<f32>,
    _pad: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> screen: Screen;

@group(0) @binding(1)
var atlas: texture_2d<f32>;

@group(0) @binding(2)
var atlas_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) textured: f32,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex: u32,
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec4<f32>,
) -> VsOut {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vertex];
    let px = rect.xy + corner * rect.zw;
    let ndc = vec2<f32>(
        px.x / screen.size.x * 2.0 - 1.0,
        1.0 - px.y / screen.size.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.color = color;
    out.uv = mix(uv.xy, uv.zw, corner);
    out.textured = select(0.0, 1.0, (uv.z - uv.x) > 0.0);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let coverage = textureSample(atlas, atlas_sampler, in.uv).r;
    let factor = mix(1.0, coverage, in.textured);
    return vec4<f32>(in.color.rgb * factor, in.color.a * factor);
}
