
@group(0) @binding(0) var src: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> VsOut {
    var xy = vec2<f32>(-1.0, -1.0);
    if (id == 1u) { xy = vec2<f32>(3.0, -1.0); }
    if (id == 2u) { xy = vec2<f32>(-1.0, 3.0); }
    var out: VsOut;
    out.pos = vec4<f32>(xy, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    return textureLoad(src, p, 0);
}
