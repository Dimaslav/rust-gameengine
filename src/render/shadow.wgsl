// Shadow map pass: только глубина, без fragment shader.
// Используется и для CSM (ortho), и для cube shadow (6 faces).

struct Lights {
    cascade_vp: array<mat4x4<f32>, 3>,
    cascade_splits: vec4<f32>,
    ambient_color: vec4<f32>,
    counts: vec4<u32>,
    light_view_proj: mat4x4<f32>,
    _pad0: vec4<f32>,
    _pad1: vec4<f32>,
    dir_lights: array<vec4<f32>, 8>,
    point_lights: array<vec4<f32>, 32>,
    cube_shadow_pos: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> lights: Lights;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) joints: vec4<u32>,
    @location(5) weights: vec4<f32>,
};

struct InstanceInput {
    @location(6)  m0: vec4<f32>,
    @location(7)  m1: vec4<f32>,
    @location(8)  m2: vec4<f32>,
    @location(9)  m3: vec4<f32>,
    @location(10) n0: vec4<f32>,
    @location(11) n1: vec4<f32>,
    @location(12) n2: vec4<f32>,
    @location(13) n3: vec4<f32>,
    @location(14) inst_color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let world = model * vec4<f32>(in.position, 1.0);
    return lights.light_view_proj * world;
}