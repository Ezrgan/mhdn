struct Quad {
    rect: vec4<f32>,
    color: vec4<f32>,
}

struct Frame {
    screen: vec2<f32>,
    _pad: vec2<f32>,
    quads: array<Quad, 256>,
}

@group(0) @binding(0)
var<uniform> frame: Frame;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VsOut {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let q = frame.quads[instance];
    let corner = corners[vertex];
    let px = q.rect.xy + corner * q.rect.zw;
    let ndc = vec2<f32>(
        px.x / frame.screen.x * 2.0 - 1.0,
        1.0 - px.y / frame.screen.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.color = q.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
