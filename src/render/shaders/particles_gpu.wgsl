// GPU particle rendering.
//
// Без vertex buffer: 6 вершин генерируются в шейдере,
// N инстансов = N частиц. Данные читаются из storage buffer.
// Мёртвые частицы clip-out'ятся (z=-2 — за far-plane).

struct Camera {
    view_proj:      mat4x4<f32>,
    inv_view_proj:  mat4x4<f32>,
    view:           mat4x4<f32>,
    inv_view:       mat4x4<f32>,
    camera_pos:     vec4<f32>,
    near_far:       vec4<f32>,
    prev_view_proj: mat4x4<f32>,
    screen_size:    vec4<f32>,
};

struct Particle {
    pos: vec4<f32>,
    vel: vec4<f32>,
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    attrs: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<storage, read> particles: array<Particle>;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @builtin(instance_index) ii: u32,
) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5),
        vec2<f32>( 0.5, -0.5),
        vec2<f32>( 0.5,  0.5),
        vec2<f32>(-0.5, -0.5),
        vec2<f32>( 0.5,  0.5),
        vec2<f32>(-0.5,  0.5),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 0.0),
    );

    var out: VOut;
    out.clip = vec4<f32>(0.0, 0.0, -2.0, 1.0);
    out.uv = vec2<f32>(0.0);
    out.color = vec4<f32>(0.0);

    if (ii >= arrayLength(&particles)) { return out; }

    let p = particles[ii];
    if (p.vel.w <= 0.0 || p.pos.w >= p.vel.w) {
        return out; // мёртвая — clip-out
    }

    let t = p.pos.w / p.vel.w;
    let size = mix(p.attrs.x, p.attrs.y, t);
    let color = mix(p.color_a, p.color_b, t);

    let c = corners[vi];
    let right = camera.view[0].xyz;
    let up = camera.view[1].xyz;
    let world = p.pos.xyz + right * c.x * size + up * c.y * size;

    out.clip = camera.view_proj * vec4<f32>(world, 1.0);
    out.uv = uvs[vi];
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let d = length(in.uv - vec2<f32>(0.5, 0.5)) * 2.0;
    let falloff = clamp(1.0 - d, 0.0, 1.0);
    let soft = falloff * falloff;
    let alpha = in.color.a * soft;
    if (alpha < 0.003) { discard; }
    return vec4<f32>(in.color.rgb, alpha);
}