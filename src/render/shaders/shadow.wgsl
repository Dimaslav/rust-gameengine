// Cube shadow map для point light. Используется `texture_depth_cube`
// в основном шейдере.
struct CubeShadow {
    light_pos: vec4<f32>,       // xyz = позиция, w = far
    face_vp: array<mat4x4<f32>, 6>,
};

@group(1) @binding(0) var<uniform> cube: CubeShadow;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct InstanceInput {
    @location(4)  m0: vec4<f32>,
    @location(5)  m1: vec4<f32>,
    @location(6)  m2: vec4<f32>,
    @location(7)  m3: vec4<f32>,
    @location(8)  n0: vec4<f32>,
    @location(9)  n1: vec4<f32>,
    @location(10) n2: vec4<f32>,
    @location(11) n3: vec4<f32>,
    @location(12) inst_color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let world = model * vec4<f32>(in.position, 1.0);
    // face_vp выбирается снаружи через push constant? Нет — рисуем все 6
    // граней одним проходом, каждая в свою array layer.
    // Здесь мы не знаем, какая грань. Поэтому используем multi-view
    // (не доступен), либо 6 draw call с разными bind groups.
    // Простейший путь: рисуем face 0, а render pass с 6 вложениями
    // вызывается 6 раз с разными bind group.
    out.clip = cube.face_vp[0] * world;
    return out;
}