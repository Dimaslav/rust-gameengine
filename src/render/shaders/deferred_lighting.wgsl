const PI: f32 = 3.14159265359;

struct Camera {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    inv_view: mat4x4<f32>,
    camera_pos: vec4<f32>,
    near_far: vec4<f32>,
};

struct Lights {
    cascade_vp:      array<mat4x4<f32>, 3>,
    cascade_splits:  vec4<f32>,
    ambient_color:   vec4<f32>,
    counts:          vec4<u32>,
    light_view_proj: mat4x4<f32>,
    misc:            vec4<f32>,   // x = ibl_strength
    fog_params:      vec4<f32>,   // x=density, y=height_base, z=height_falloff
    fog_color:       vec4<f32>,   // rgb = color
    _pad1:           vec4<f32>,
    dir_lights:      array<vec4<f32>, 8>,
    point_lights:    array<vec4<f32>, 32>,
    cube_shadow_pos: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var t_albedo: texture_2d<f32>;
@group(0) @binding(1) var t_normal: texture_2d<f32>;
@group(0) @binding(2) var t_emissive: texture_2d<f32>;
@group(0) @binding(3) var t_depth: texture_depth_2d;
@group(0) @binding(4) var s_lin: sampler;
@group(0) @binding(5) var<uniform> camera: Camera;

@group(1) @binding(0) var<uniform> lights: Lights;

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

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var out: VOut;
    out.clip = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

fn reconstruct_world_pos(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, depth, 1.0);
    let world_h = camera.inv_view_proj * ndc;
    return world_h.xyz / max(world_h.w, 1e-6);
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
    n: vec3<f32>, v: vec3<f32>, l: vec3<f32>,
    albedo: vec3<f32>, metallic: f32, roughness: f32, radiance: vec3<f32>,
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

fn ibl_diffuse(n: vec3<f32>, albedo: vec3<f32>, metallic: f32) -> vec3<f32> {
    let irr = textureSampleLevel(t_irradiance, s_env, n, 0.0).rgb;
    return albedo * irr * (1.0 - metallic);
}

fn ibl_specular(n: vec3<f32>, v: vec3<f32>, albedo: vec3<f32>, metallic: f32, roughness: f32) -> vec3<f32> {
    let r = reflect(-v, n);
    let mip = roughness * 4.0;
    let prefiltered = textureSampleLevel(t_prefiltered, s_env, r, mip).rgb;
    let n_dot_v = max(dot(n, v), 0.0);
    let brdf = textureSampleLevel(t_brdf_lut, s_env, vec2<f32>(n_dot_v, roughness), 0.0).rg;
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    return prefiltered * (f0 * brdf.x + brdf.y);
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

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let depth = textureSample(t_depth, s_lin, in.uv);
    if (depth >= 0.9999) {
        // Небо рисует отдельный skybox pass. Здесь — ничего.
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    let g_albedo = textureSample(t_albedo, s_lin, in.uv);
    let g_normal = textureSample(t_normal, s_lin, in.uv);
    let g_emissive = textureSample(t_emissive, s_lin, in.uv);

    let albedo = g_albedo.rgb;
    let metallic = g_albedo.a;
    let n = normalize(g_normal.rgb);
    let roughness = g_emissive.a;
    let emissive = g_emissive.rgb;

    let world_pos = reconstruct_world_pos(in.uv, depth);
    let view_depth = length(camera.camera_pos.xyz - world_pos);
    let v = normalize(camera.camera_pos.xyz - world_pos);

    let ao = textureSample(t_ssao, s_ssao, in.uv).r;
    let ibl_strength = lights.misc.x;

    var color = emissive;
    color += ibl_diffuse(n, albedo, metallic) * ao * ibl_strength;
    color += ibl_specular(n, v, albedo, metallic, roughness) * ao * ibl_strength;

    let dir_count = lights.counts.x;
    let csm_shadow = compute_csm_shadow(world_pos, view_depth);
    for (var i: u32 = 0u; i < dir_count; i = i + 1u) {
        let idx = i * 2u;
        let dir_w = lights.dir_lights[idx];
        let col_w = lights.dir_lights[idx + 1u];
        let l = normalize(dir_w.xyz);
        let radiance = col_w.rgb * dir_w.w;
        let s = select(1.0, csm_shadow, i == 0u);
        color += pbr_light(n, v, l, albedo, metallic, roughness, radiance * s);
    }

    let pt_count = lights.counts.y;
    let cube_count = lights.counts.z;
    for (var i: u32 = 0u; i < pt_count; i = i + 1u) {
        let idx = i * 2u;
        let pos_r = lights.point_lights[idx];
        let col_i = lights.point_lights[idx + 1u];
        let to_light = pos_r.xyz - world_pos;
        let dist = length(to_light);
        if (dist < pos_r.w) {
            let l = to_light / max(dist, 0.0001);
            var atten = clamp(1.0 - dist / pos_r.w, 0.0, 1.0);
            atten = atten * atten;
            var s = 1.0;
            if (i == 0u && cube_count > 0u) {
                s = compute_point_shadow(world_pos, 0u);
            }
            color += pbr_light(n, v, l, albedo, metallic, roughness, col_i.rgb * col_i.a * atten * s);
        }
    }

    // === Height fog ===
    let fog_density = lights.fog_params.x;
    if (fog_density > 0.0) {
        let fog_h_base  = lights.fog_params.y;
        let fog_h_fall  = lights.fog_params.z;
        let cam_pos     = camera.camera_pos.xyz;
        let to_frag     = world_pos - cam_pos;
        let dist        = length(to_frag);
        let h           = max(0.0, world_pos.y - fog_h_base);
        let h_factor    = exp(-h * fog_h_fall);
        let fog_amount  = clamp(1.0 - exp(-dist * fog_density * h_factor), 0.0, 1.0);
        color           = mix(color, lights.fog_color.rgb, fog_amount);
    }

    return vec4<f32>(color, 1.0);
}