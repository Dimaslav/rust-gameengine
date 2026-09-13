struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    camera_pos: vec4<f32>,
    near_far: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

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
    @location(0) view_normal: vec3<f32>,
    @location(1) view_z: f32,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let normal_matrix = mat4x4<f32>(inst.n0, inst.n1, inst.n2, inst.n3);

    let world = model * vec4<f32>(in.position, 1.0);
    let view = camera.view * world;
    out.clip = camera.view_proj * world;
    out.view_normal = normalize((camera.view * normal_matrix * vec4<f32>(in.normal, 0.0)).xyz);
    out.view_z = -view.z;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let far = camera.near_far.y;
    // RGB = view-space normal, A = linear depth normalized by far
    return vec4<f32>(in.view_normal, in.view_z / far);
}