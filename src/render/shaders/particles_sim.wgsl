struct SimParams {
    dt: f32,
    time: f32,
    _pad0: f32,
    _pad1: f32,
};

struct Particle {
    pos: vec4<f32>,     // xyz, w = age
    vel: vec4<f32>,     // xyz, w = lifetime (0 = dead)
    color_a: vec4<f32>, // start rgba
    color_b: vec4<f32>, // end rgba
    attrs: vec4<f32>,   // x = size_start, y = size_end, z = gravity, w = drag
};

@group(0) @binding(0) var<uniform> params: SimParams;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;

@compute @workgroup_size(256, 1, 1)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&particles)) { return; }

    var p = particles[i];
    if (p.vel.w <= 0.0) { return; } // пустой слот
    if (p.pos.w >= p.vel.w) { return; } // уже мёртв

    p.pos.w += params.dt;
    if (p.pos.w >= p.vel.w) {
        // Только что умер — фиксируем и выходим.
        particles[i] = p;
        return;
    }

    // Гравитация.
    p.vel.y -= p.attrs.z * params.dt;

    // Drag (линейное затухание).
    let drag = max(1.0 - p.attrs.w * params.dt, 0.0);
    p.vel = vec4<f32>(p.vel.xyz * drag, p.vel.w);

    // Интеграция позиции.
    p.pos = vec4<f32>(p.pos.xyz + p.vel.xyz * params.dt, p.pos.w);

    particles[i] = p;
}