struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    inv_view: mat4x4<f32>,
    camera_pos: vec4<f32>,
    near_far: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(2) @binding(10) var t_env: texture_cube<f32>;
@group(2) @binding(9) var s_env: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: VOut;
    let p = positions[idx];
    out.clip = vec4<f32>(p, 1.0, 1.0);
    out.local = vec3<f32>(p.x, -p.y, 1.0);
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let dir_view = normalize(vec3<f32>(in.local.x, in.local.y, -1.0));
    let dir_world = normalize((camera.inv_view * vec4<f32>(dir_view, 0.0)).xyz);
    return vec4<f32>(textureSampleLevel(t_env, s_env, dir_world, 0.0).rgb, 1.0);
}