struct SsaoUniform {
    // x = proj[0][0], y = proj[1][1], z = far, w = radius
    proj_scale: vec4<f32>,
    // x = bias, y = strength, zw = noise_uv_scale
    params: vec4<f32>,
};

@group(0) @binding(0) var t_gbuffer: texture_2d<f32>;
@group(0) @binding(1) var t_noise: texture_2d<f32>;
@group(0) @binding(2) var s_lin: sampler;
@group(0) @binding(3) var<uniform> params: SsaoUniform;

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
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
    var out: VertexOutput;
    out.clip = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

// Восстанавливает view-space позицию из UV и нормализованной глубины.
fn reconstruct_view_pos(uv: vec2<f32>, depth_norm: f32) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0);
    let ld = depth_norm * params.proj_scale.z;
    let vx = ndc.x * ld / params.proj_scale.x;
    let vy = ndc.y * ld / params.proj_scale.y;
    return vec3<f32>(vx, vy, -ld);
}

// 8 сэмплов в hemisphere (развёрнуто, naga не позволяет dynamic array index).
fn compute_occlusion(view_pos: vec3<f32>, normal: vec3<f32>, uv: vec2<f32>) -> f32 {
    let radius = params.proj_scale.w;
    let bias = params.params.x;

    // TBN: нормаль как Z, случайный вектор из noise текстуры как T.
    let noise_uv = uv * params.params.zw;
    let noise = textureSample(t_noise, s_lin, noise_uv).xyz;
    // Gram-Schmidt: T ортогонален N.
    var tangent = normalize(noise - normal * dot(noise, normal));
    if (length(tangent) < 0.001) {
        tangent = normalize(cross(normal, vec3<f32>(0.0, 1.0, 0.0)));
    }
    let bitangent = cross(normal, tangent);
    let tbn = mat3x3<f32>(tangent, bitangent, normal);

    var occlusion = 0.0;
    let screen_size = vec2<f32>(textureDimensions(t_gbuffer));

    // 8 сэмплов. Hardcoded.
    // Сэмпл i в tangent-space → view-space.
    // Проекция в screen → чтение depth → сравнение.

    // s0
    {
        let s = tbn * normalize(vec3<f32>( 0.5381, 0.1856, 0.4319)) * radius;
        let sp = view_pos + s;
        let clip = vec4<f32>(sp.x * params.proj_scale.x, sp.y * params.proj_scale.y, -sp.z - bias, 1.0);
        let ndc = clip.xy / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0) {
            let sd = textureSample(t_gbuffer, s_lin, suv).a * params.proj_scale.z;
            let sample_z = -sd;
            if (sample_z >= sp.z + bias) {
                occlusion += 1.0;
            }
        }
    }

    // s1
    {
        let s = tbn * normalize(vec3<f32>( 0.1379, 0.2486, 0.4430)) * radius;
        let sp = view_pos + s;
        let clip = vec4<f32>(sp.x * params.proj_scale.x, sp.y * params.proj_scale.y, -sp.z - bias, 1.0);
        let ndc = clip.xy / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0) {
            let sd = textureSample(t_gbuffer, s_lin, suv).a * params.proj_scale.z;
            let sample_z = -sd;
            if (sample_z >= sp.z + bias) { occlusion += 1.0; }
        }
    }

    // s2
    {
        let s = tbn * normalize(vec3<f32>( 0.3371, 0.5679, 0.0057)) * radius;
        let sp = view_pos + s;
        let clip = vec4<f32>(sp.x * params.proj_scale.x, sp.y * params.proj_scale.y, -sp.z - bias, 1.0);
        let ndc = clip.xy / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0) {
            let sd = textureSample(t_gbuffer, s_lin, suv).a * params.proj_scale.z;
            let sample_z = -sd;
            if (sample_z >= sp.z + bias) { occlusion += 1.0; }
        }
    }

    // s3
    {
        let s = tbn * normalize(vec3<f32>(-0.6999, -0.0451, 0.0019)) * radius;
        let sp = view_pos + s;
        let clip = vec4<f32>(sp.x * params.proj_scale.x, sp.y * params.proj_scale.y, -sp.z - bias, 1.0);
        let ndc = clip.xy / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0) {
            let sd = textureSample(t_gbuffer, s_lin, suv).a * params.proj_scale.z;
            let sample_z = -sd;
            if (sample_z >= sp.z + bias) { occlusion += 1.0; }
        }
    }

    // s4
    {
        let s = tbn * normalize(vec3<f32>( 0.0689, -0.1598, 0.8547)) * radius;
        let sp = view_pos + s;
        let clip = vec4<f32>(sp.x * params.proj_scale.x, sp.y * params.proj_scale.y, -sp.z - bias, 1.0);
        let ndc = clip.xy / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0) {
            let sd = textureSample(t_gbuffer, s_lin, suv).a * params.proj_scale.z;
            let sample_z = -sd;
            if (sample_z >= sp.z + bias) { occlusion += 1.0; }
        }
    }

    // s5
    {
        let s = tbn * normalize(vec3<f32>( 0.0560, 0.0069, 0.1843)) * radius;
        let sp = view_pos + s;
        let clip = vec4<f32>(sp.x * params.proj_scale.x, sp.y * params.proj_scale.y, -sp.z - bias, 1.0);
        let ndc = clip.xy / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0) {
            let sd = textureSample(t_gbuffer, s_lin, suv).a * params.proj_scale.z;
            let sample_z = -sd;
            if (sample_z >= sp.z + bias) { occlusion += 1.0; }
        }
    }

    // s6
    {
        let s = tbn * normalize(vec3<f32>(-0.0146, 0.1402, 0.0762)) * radius;
        let sp = view_pos + s;
        let clip = vec4<f32>(sp.x * params.proj_scale.x, sp.y * params.proj_scale.y, -sp.z - bias, 1.0);
        let ndc = clip.xy / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0) {
            let sd = textureSample(t_gbuffer, s_lin, suv).a * params.proj_scale.z;
            let sample_z = -sd;
            if (sample_z >= sp.z + bias) { occlusion += 1.0; }
        }
    }

    // s7
    {
        let s = tbn * normalize(vec3<f32>( 0.0100, -0.1924, 0.0344)) * radius;
        let sp = view_pos + s;
        let clip = vec4<f32>(sp.x * params.proj_scale.x, sp.y * params.proj_scale.y, -sp.z - bias, 1.0);
        let ndc = clip.xy / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0) {
            let sd = textureSample(t_gbuffer, s_lin, suv).a * params.proj_scale.z;
            let sample_z = -sd;
            if (sample_z >= sp.z + bias) { occlusion += 1.0; }
        }
    }

    return occlusion / 8.0;
}

@fragment
fn fs_ssao(in: VertexOutput) -> @location(0) vec4<f32> {
    let g = textureSample(t_gbuffer, s_lin, in.uv);
    let depth_norm = g.a;
    // Если это skybox / фон, depth_norm == 1.0 — не затеняем.
    if (depth_norm >= 0.9999) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }
    let normal = normalize(g.xyz);
    let view_pos = reconstruct_view_pos(in.uv, depth_norm);
    let ao = compute_occlusion(view_pos, normal, in.uv);
    // Возвращаем "1 - ao" чтобы bloom и tonemap не путались.
    return vec4<f32>(1.0 - ao, 0.0, 0.0, 1.0);
}

// === Blur 4x4 ===
@fragment
fn fs_blur(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t_gbuffer));
    var sum = 0.0;
    for (var y = -2; y < 2; y = y + 1) {
        for (var x = -2; x < 2; x = x + 1) {
            let o = vec2<f32>(f32(x), f32(y)) * texel;
            sum += textureSample(t_gbuffer, s_lin, in.uv + o).r;
        }
    }
    return vec4<f32>(sum / 16.0, 0.0, 0.0, 1.0);
}