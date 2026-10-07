// Screen-space UI quads. Один инстанс = один прямоугольник.
// 6 вершин генерируются в шейдере через vertex_index.

struct UiGlobal {
    // x = width_px, y = height_px, z = 2/width, w = 2/height
    screen: vec4<f32>,
    // x = time
    time: vec4<f32>,
};

@group(0) @binding(0) var<uniform> global: UiGlobal;
@group(0) @binding(1) var t_atlas: texture_2d<f32>;
@group(0) @binding(2) var s_atlas: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv_rect: vec4<f32>,
) -> VOut {
    // 2 треугольника: (0,0),(1,0),(1,1) и (0,0),(1,1),(0,1)
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    let c = corners[vi];

    let px = rect.xy + c * rect.zw;
    let ndc_x = px.x * global.screen.z - 1.0;
    let ndc_y = 1.0 - px.y * global.screen.w;

    var out: VOut;
    out.clip = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = color;
    out.uv = mix(uv_rect.xy, uv_rect.zw, c);
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(t_atlas, s_atlas, in.uv);
    let c = in.color * t;
    if (c.a < 0.001) { discard; }
    return c;
}