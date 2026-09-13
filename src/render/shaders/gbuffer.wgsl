const MAX_JOINTS: u32 = 64u;

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

struct SkeletonUniform {
    joints: array<mat4x4<f32>, MAX_JOINTS>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var t_base: texture_2d<f32>;
@group(1) @binding(1) var t_mr: texture_2d<f32>;
@group(1) @binding(2) var t_normal: texture_2d<f32>;
@group(1) @binding(3) var t_emissive: texture_2d<f32>;
@group(1) @binding(4) var s_mat: sampler;
@group(1) @binding(5) var<uniform> material: MaterialUniform;
@group(1) @binding(6) var<uniform> skeleton: SkeletonUniform;

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

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) view_depth: f32,
};

struct FragmentOutput {
    @location(0) g_albedo: vec4<f32>,     // rgb=albedo, a=metallic
    @location(1) g_normal: vec4<f32>,     // rgb=world_normal, a=depth_norm
    @location(2) g_emissive: vec4<f32>,   // rgb=emissive, a=roughness
};

fn skeleton_matrix(idx: u32) -> mat4x4<f32> {
    switch (idx) {
        case 0u:  { return skeleton.joints[0]; }
        case 1u:  { return skeleton.joints[1]; }
        case 2u:  { return skeleton.joints[2]; }
        case 3u:  { return skeleton.joints[3]; }
        case 4u:  { return skeleton.joints[4]; }
        case 5u:  { return skeleton.joints[5]; }
        case 6u:  { return skeleton.joints[6]; }
        case 7u:  { return skeleton.joints[7]; }
        case 8u:  { return skeleton.joints[8]; }
        case 9u:  { return skeleton.joints[9]; }
        case 10u: { return skeleton.joints[10]; }
        case 11u: { return skeleton.joints[11]; }
        case 12u: { return skeleton.joints[12]; }
        case 13u: { return skeleton.joints[13]; }
        case 14u: { return skeleton.joints[14]; }
        case 15u: { return skeleton.joints[15]; }
        case 16u: { return skeleton.joints[16]; }
        case 17u: { return skeleton.joints[17]; }
        case 18u: { return skeleton.joints[18]; }
        case 19u: { return skeleton.joints[19]; }
        case 20u: { return skeleton.joints[20]; }
        case 21u: { return skeleton.joints[21]; }
        case 22u: { return skeleton.joints[22]; }
        case 23u: { return skeleton.joints[23]; }
        case 24u: { return skeleton.joints[24]; }
        case 25u: { return skeleton.joints[25]; }
        case 26u: { return skeleton.joints[26]; }
        case 27u: { return skeleton.joints[27]; }
        case 28u: { return skeleton.joints[28]; }
        case 29u: { return skeleton.joints[29]; }
        case 30u: { return skeleton.joints[30]; }
        case 31u: { return skeleton.joints[31]; }
        case 32u: { return skeleton.joints[32]; }
        case 33u: { return skeleton.joints[33]; }
        case 34u: { return skeleton.joints[34]; }
        case 35u: { return skeleton.joints[35]; }
        case 36u: { return skeleton.joints[36]; }
        case 37u: { return skeleton.joints[37]; }
        case 38u: { return skeleton.joints[38]; }
        case 39u: { return skeleton.joints[39]; }
        case 40u: { return skeleton.joints[40]; }
        case 41u: { return skeleton.joints[41]; }
        case 42u: { return skeleton.joints[42]; }
        case 43u: { return skeleton.joints[43]; }
        case 44u: { return skeleton.joints[44]; }
        case 45u: { return skeleton.joints[45]; }
        case 46u: { return skeleton.joints[46]; }
        case 47u: { return skeleton.joints[47]; }
        case 48u: { return skeleton.joints[48]; }
        case 49u: { return skeleton.joints[49]; }
        case 50u: { return skeleton.joints[50]; }
        case 51u: { return skeleton.joints[51]; }
        case 52u: { return skeleton.joints[52]; }
        case 53u: { return skeleton.joints[53]; }
        case 54u: { return skeleton.joints[54]; }
        case 55u: { return skeleton.joints[55]; }
        case 56u: { return skeleton.joints[56]; }
        case 57u: { return skeleton.joints[57]; }
        case 58u: { return skeleton.joints[58]; }
        case 59u: { return skeleton.joints[59]; }
        case 60u: { return skeleton.joints[60]; }
        case 61u: { return skeleton.joints[61]; }
        case 62u: { return skeleton.joints[62]; }
        case 63u: { return skeleton.joints[63]; }
        default: { return mat4x4<f32>(
            vec4<f32>(1.0, 0.0, 0.0, 0.0),
            vec4<f32>(0.0, 1.0, 0.0, 0.0),
            vec4<f32>(0.0, 0.0, 1.0, 0.0),
            vec4<f32>(0.0, 0.0, 0.0, 1.0),
        ); }
    }
}

fn skin_position(pos: vec3<f32>, joints: vec4<u32>, weights: vec4<f32>) -> vec3<f32> {
    let w_sum = weights.x + weights.y + weights.z + weights.w;
    if (w_sum < 0.001) { return pos; }
    let p0 = (skeleton_matrix(joints.x) * vec4<f32>(pos, 1.0)).xyz;
    let p1 = (skeleton_matrix(joints.y) * vec4<f32>(pos, 1.0)).xyz;
    let p2 = (skeleton_matrix(joints.z) * vec4<f32>(pos, 1.0)).xyz;
    let p3 = (skeleton_matrix(joints.w) * vec4<f32>(pos, 1.0)).xyz;
    return p0 * weights.x + p1 * weights.y + p2 * weights.z + p3 * weights.w;
}

fn skin_normal(n: vec3<f32>, joints: vec4<u32>, weights: vec4<f32>) -> vec3<f32> {
    let w_sum = weights.x + weights.y + weights.z + weights.w;
    if (w_sum < 0.001) { return n; }
    let n0 = (skeleton_matrix(joints.x) * vec4<f32>(n, 0.0)).xyz;
    let n1 = (skeleton_matrix(joints.y) * vec4<f32>(n, 0.0)).xyz;
    let n2 = (skeleton_matrix(joints.z) * vec4<f32>(n, 0.0)).xyz;
    let n3 = (skeleton_matrix(joints.w) * vec4<f32>(n, 0.0)).xyz;
    return normalize(n0 * weights.x + n1 * weights.y + n2 * weights.z + n3 * weights.w);
}

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let normal_matrix = mat4x4<f32>(inst.n0, inst.n1, inst.n2, inst.n3);

    let skinned_pos = skin_position(in.position, in.joints, in.weights);
    let skinned_normal = skin_normal(in.normal, in.joints, in.weights);

    let world = model * vec4<f32>(skinned_pos, 1.0);
    let view = camera.view * world;

    out.clip = camera.view_proj * world;
    out.world_pos = world.xyz;
    out.world_normal = normalize((normal_matrix * vec4<f32>(skinned_normal, 0.0)).xyz);
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
    // Упаковка: normal.rgb=world_normal, normal.a=view_depth_norm
    out.g_normal = vec4<f32>(n, in.view_depth / camera.near_far.y);
    // Упаковка: emissive.rgb=emissive, emissive.a=roughness
    out.g_emissive = vec4<f32>(emissive, roughness);
    return out;
}