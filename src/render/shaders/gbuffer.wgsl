struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    inv_view: mat4x4<f32>,
    camera_pos: vec4<f32>,
    near_far: vec4<f32>,
};

struct MaterialUniform {
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var t_base: texture_2d<f32>;
@group(1) @binding(1) var t_mr: texture_2d<f32>;
@group(1) @binding(2) var t_normal: texture_2d<f32>;
@group(1) @binding(3) var t_emissive: texture_2d<f32>;
@group(1) @binding(4) var s_mat: sampler;
@group(1) @binding(5) var<uniform> material: MaterialUniform;

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
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) view_depth: f32,
};

struct FragmentOutput {
    @location(0) g_albedo: vec4<f32>,
    @location(1) g_normal: vec4<f32>,
    @location(2) g_emissive: vec4<f32>,
    @location(3) g_depth: f32,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let normal_matrix = mat4x4<f32>(inst.n0, inst.n1, inst.n2, inst.n3);

    let world = model * vec4<f32>(in.position, 1.0);
    let view = camera.view * world;

    out.clip = camera.view_proj * world;
    out.world_pos = world.xyz;
    out.world_normal = normalize((normal_matrix * vec4<f32>(in.normal, 0.0)).xyz);
    out.uv = in.uv;
    out.color = in.color * inst.inst_color;
    out.view_depth = -view.z;
    return out;
}

fn apply_normal_map(
    geo_normal: vec3<f32>,
    world_pos: vec3<f32>,
    uv: vec2<f32>,
    normal_sample: vec3<f32>,
    scale: f32,
) -> vec3<f32> {
    let dp1 = dpdx(world_pos);
    let dp2 = dpdy(world_pos);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);

    let det = duv1.x * duv2.y - duv2.x * duv1.y;
    if (abs(det) < 1e-8) { return geo_normal; }

    let t_raw = dp1 * duv2.y - dp2 * duv1.y;
    let b_raw = -dp1 * duv2.x + dp2 * duv1.x;

    let n = normalize(geo_normal);
    let t = normalize(t_raw - n * dot(n, t_raw));
    let b = normalize(b_raw - n * dot(n, b_raw) - t * dot(t, b_raw));

    var ts = normal_sample * 2.0 - 1.0;
    ts.x = ts.x * scale;
    ts.y = ts.y * scale;
    ts = normalize(ts);

    let world_n = t * ts.x + b * ts.y + n * ts.z;
    if (dot(world_n, n) < 0.0) { return n; }
    return normalize(world_n);
}

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    let geo_n = normalize(in.world_normal);

    let base_sample = textureSample(t_base, s_mat, in.uv);
    let albedo = (base_sample * material.base_color * in.color).rgb;

    let mr_sample = textureSample(t_mr, s_mat, in.uv);
    let roughness = clamp(mr_sample.g * material.params.y, 0.04, 1.0);
    let metallic  = clamp(mr_sample.b * material.params.x, 0.0, 1.0);

    var n = geo_n;
    if (material.params.z != 0.0) {
        let nm = textureSample(t_normal, s_mat, in.uv).rgb;
        n = apply_normal_map(geo_n, in.world_pos, in.uv, nm, material.params.z);
    }

    let emissive_tex = textureSample(t_emissive, s_mat, in.uv).rgb;
    let emissive = emissive_tex * material.emissive.rgb * material.emissive.a;

    var out: FragmentOutput;
    out.g_albedo = vec4<f32>(albedo, metallic);
    out.g_normal = vec4<f32>(n, roughness);
    out.g_emissive = vec4<f32>(emissive, 1.0);
    out.g_depth = in.view_depth / camera.near_far.y;
    return out;
}