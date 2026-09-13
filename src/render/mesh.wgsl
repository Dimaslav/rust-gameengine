const MAX_DIR_LIGHTS: u32 = 4u;
const MAX_POINT_LIGHTS: u32 = 16u;
const CASCADE_COUNT: u32 = 3u;
const PI: f32 = 3.14159265359;

struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    camera_pos: vec4<f32>,
    near_far: vec4<f32>,
};

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

struct MaterialUniform {
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    params: vec4<f32>,   // x=metallic, y=roughness, z=normal_scale, w=ibl_strength
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> lights: Lights;

// Group 2
@group(2) @binding(0) var t_csm: texture_depth_2d_array;
@group(2) @binding(1) var s_csm: sampler_comparison;
@group(2) @binding(2) var t_point_shadow: texture_depth_cube;
@group(2) @binding(3) var s_point_shadow: sampler_comparison;
@group(2) @binding(4) var t_ssao: texture_2d<f32>;
@group(2) @binding(5) var s_ssao: sampler;
@group(2) @binding(6) var t_irradiance: texture_cube<f32>;
@group(2) @binding(7) var t_prefiltered: texture_cube<f32>;
@group(2) @binding(8) var t_brdf_lut: texture_2d<f32>;
@group(2) @binding(9) var s_env: sampler;
@group(2) @binding(10) var t_env: texture_cube<f32>;

// Group 3: material
@group(3) @binding(0) var t_base: texture_2d<f32>;
@group(3) @binding(1) var t_mr: texture_2d<f32>;
@group(3) @binding(2) var t_normal: texture_2d<f32>;
@group(3) @binding(3) var t_emissive: texture_2d<f32>;
@group(3) @binding(4) var s_mat: sampler;
@group(3) @binding(5) var<uniform> material: MaterialUniform;

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
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let model = mat4x4<f32>(inst.m0, inst.m1, inst.m2, inst.m3);
    let normal_matrix = mat4x4<f32>(inst.n0, inst.n1, inst.n2, inst.n3);

    let world = model * vec4<f32>(in.position, 1.0);
    out.clip = camera.view_proj * world;
    out.world_pos = world.xyz;
    out.normal = normalize((normal_matrix * vec4<f32>(in.normal, 0.0)).xyz);
    out.uv = in.uv;
    out.color = in.color * inst.inst_color;
    return out;
}

fn compute_csm_shadow(world_pos: vec3<f32>, view_depth: f32) -> f32 {
    var cascade = 0i;
    if (view_depth > lights.cascade_splits.x) { cascade = 1i; }
    if (view_depth > lights.cascade_splits.y) { cascade = 2i; }

    let light_clip = lights.cascade_vp[cascade] * vec4<f32>(world_pos, 1.0);
    let ndc = light_clip.xyz / light_clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);

    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) { return 1.0; }
    if (ndc.z < 0.0 || ndc.z > 1.0) { return 1.0; }

    let bias = 0.002;
    let depth = ndc.z - bias;

    var shadow = 0.0;
    let texel = 1.0 / 2048.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let off = vec2<f32>(f32(x), f32(y)) * texel;
            shadow += textureSampleCompare(t_csm, s_csm, uv + off, cascade, depth);
        }
    }
    return shadow / 9.0;
}

fn compute_point_shadow(world_pos: vec3<f32>, light_idx: u32) -> f32 {
    let pos = lights.cube_shadow_pos[light_idx].xyz;
    let far = lights.cube_shadow_pos[light_idx].w;
    let to_frag = world_pos - pos;
    let dist = length(to_frag);
    if (dist > far) { return 1.0; }
    let dir = to_frag / max(dist, 0.0001);
    let bias = 0.01;
    let depth = dist / far - bias;
    return textureSampleCompare(t_point_shadow, s_point_shadow, dir, depth);
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h2 = n_dot_h * n_dot_h;
    let denom = n_dot_h2 * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom + 1e-6);
}

fn geometry_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    let gv = n_dot_v / (n_dot_v * (1.0 - k) + k + 1e-6);
    let gl = n_dot_l / (n_dot_l * (1.0 - k) + k + 1e-6);
    return gv * gl;
}

fn pbr_light(
    n: vec3<f32>,
    v: vec3<f32>,
    l: vec3<f32>,
    albedo: vec3<f32>,
    metallic: f32,
    roughness: f32,
    radiance: vec3<f32>,
) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) { return vec3<f32>(0.0); }
    let n_dot_v = max(dot(n, v), 1e-4);
    let h = normalize(l + v);
    let n_dot_h = max(dot(n, h), 0.0);
    let h_dot_v = max(dot(h, v), 0.0);

    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let f = fresnel_schlick(h_dot_v, f0);
    let d = distribution_ggx(n_dot_h, roughness);
    let g = geometry_smith(n_dot_v, n_dot_l, roughness);

    let specular = (d * g) * f / (4.0 * n_dot_v * n_dot_l + 1e-4);
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    let diffuse = kd * albedo / PI;

    return (diffuse + specular) * radiance * n_dot_l;
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

// === IBL ===
fn ibl_diffuse(n: vec3<f32>, albedo: vec3<f32>, metallic: f32) -> vec3<f32> {
    let irr = textureSampleLevel(t_irradiance, s_env, n, 0.0).rgb;
    let kd = (1.0 - metallic);
    return albedo * irr * kd;
}

fn ibl_specular(n: vec3<f32>, v: vec3<f32>, albedo: vec3<f32>, metallic: f32, roughness: f32, ibl_strength: f32) -> vec3<f32> {
    let r = reflect(-v, n);
    // max mip = PREFILTER_MIPS - 1 = 4
    let mip = roughness * 4.0;
    let prefiltered = textureSampleLevel(t_prefiltered, s_env, r, mip).rgb;

    let n_dot_v = max(dot(n, v), 0.0);
    let brdf = textureSampleLevel(t_brdf_lut, s_env, vec2<f32>(n_dot_v, roughness), 0.0).rg;

    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let specular = prefiltered * (f0 * brdf.x + brdf.y);

    return specular * ibl_strength;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let geo_n = normalize(in.normal);
    let v = normalize(camera.camera_pos.xyz - in.world_pos);

    let base_sample = textureSample(t_base, s_mat, in.uv);
    let albedo = (base_sample * material.base_color * in.color).rgb;

    let mr_sample = textureSample(t_mr, s_mat, in.uv);
    let roughness = clamp(mr_sample.g * material.params.y, 0.04, 1.0);
    let metallic  = clamp(mr_sample.b * material.params.x, 0.0, 1.0);
    let ibl_strength = select(1.0, material.params.w, material.params.w > 0.0);

    var n = geo_n;
    if (material.params.z != 0.0) {
        let nm = textureSample(t_normal, s_mat, in.uv).rgb;
        n = apply_normal_map(geo_n, in.world_pos, in.uv, nm, material.params.z);
    }

    let emissive_tex = textureSample(t_emissive, s_mat, in.uv).rgb;
    let emissive = emissive_tex * material.emissive.rgb * material.emissive.a;

    // SSAO
    let ssao_size = vec2<f32>(textureDimensions(t_ssao));
    let ssao_uv = in.clip.xy / ssao_size;
    let ao = textureSample(t_ssao, s_ssao, ssao_uv).r;

    // Ambient через IBL (вместо фиксированного ambient)
    var color = emissive;
    color += ibl_diffuse(n, albedo, metallic) * ao;
    color += ibl_specular(n, v, albedo, metallic, roughness, ibl_strength) * ao;

    // Directional lights
    let view_depth = length(camera.camera_pos.xyz - in.world_pos);
    let csm_shadow = compute_csm_shadow(in.world_pos, view_depth);

    let dir_count = lights.counts.x;
    for (var i: u32 = 0u; i < dir_count; i = i + 1u) {
        let idx = i * 2u;
        let dir_w = lights.dir_lights[idx];
        let col_w = lights.dir_lights[idx + 1u];
        let l = normalize(dir_w.xyz);
        let radiance = col_w.rgb * col_w.a;
        let s = select(1.0, csm_shadow, i == 0u);
        color += pbr_light(n, v, l, albedo, metallic, roughness, radiance * s);
    }

    // Point lights
    let pt_count = lights.counts.y;
    let cube_count = lights.counts.z;

    for (var i: u32 = 0u; i < pt_count; i = i + 1u) {
        let idx = i * 2u;
        let pos_r = lights.point_lights[idx];
        let col_i = lights.point_lights[idx + 1u];
        let to_light = pos_r.xyz - in.world_pos;
        let dist = length(to_light);
        if (dist < pos_r.w) {
            let l = to_light / max(dist, 0.0001);
            var atten = clamp(1.0 - dist / pos_r.w, 0.0, 1.0);
            atten = atten * atten;
            var s = 1.0;
            if (i == 0u && cube_count > 0u) {
                s = compute_point_shadow(in.world_pos, 0u);
            }
            color += pbr_light(n, v, l, albedo, metallic, roughness, col_i.rgb * col_i.a * atten * s);
        }
    }

    return vec4<f32>(color, base_sample.a * material.base_color.a * in.color.a);
}