// SSAO: читает normal из .rgb и depth_norm из .a текстуры gbuffer_normal.

struct SsaoUniform {
    proj_scale: vec4<f32>,
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

fn reconstruct_view_pos(uv: vec2<f32>, depth_norm: f32) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0);
    let ld = depth_norm * params.proj_scale.z;
    let vx = ndc.x * ld / params.proj_scale.x;
    let vy = ndc.y * ld / params.proj_scale.y;
    return vec3<f32>(vx, vy, -ld);
}

fn sample_occlusion(
    view_pos: vec3<f32>,
    tbn: mat3x3<f32>,
    radius: f32,
    bias: f32,
    sample_dir: vec3<f32>,
) -> f32 {
    let s = tbn * normalize(sample_dir) * radius;
    let sp = view_pos + s;
    let clip = vec4<f32>(sp.x * params.proj_scale.x, sp.y * params.proj_scale.y, -sp.z - bias, 1.0);
    let ndc = clip.xy / clip.w;
    let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (suv.x < 0.0 || suv.x > 1.0 || suv.y < 0.0 || suv.y > 1.0) { return 0.0; }
    let sd = textureSample(t_gbuffer, s_lin, suv).a * params.proj_scale.z;
    let sample_z = -sd;
    if (sample_z >= sp.z + bias) { return 1.0; }
    return 0.0;
}

fn compute_occlusion(view_pos: vec3<f32>, normal: vec3<f32>, uv: vec2<f32>) -> f32 {
    let radius = params.proj_scale.w;
    let bias = params.params.x;
    let noise_uv = uv * params.params.zw;
    let noise = textureSample(t_noise, s_lin, noise_uv).xyz;
    var tangent = normalize(noise - normal * dot(noise, normal));
    if (length(tangent) < 0.001) {
        tangent = normalize(cross(normal, vec3<f32>(0.0, 1.0, 0.0)));
    }
    let bitangent = cross(normal, tangent);
    let tbn = mat3x3<f32>(tangent, bitangent, normal);

    var occlusion = 0.0;
    occlusion += sample_occlusion(view_pos, tbn, radius, bias, vec3<f32>( 0.5381,  0.1856,  0.4319));
    occlusion += sample_occlusion(view_pos, tbn, radius, bias, vec3<f32>( 0.1379,  0.2486,  0.4430));
    occlusion += sample_occlusion(view_pos, tbn, radius, bias, vec3<f32>( 0.3371,  0.5679,  0.0057));
    occlusion += sample_occlusion(view_pos, tbn, radius, bias, vec3<f32>(-0.6999, -0.0451,  0.0019));
    occlusion += sample_occlusion(view_pos, tbn, radius, bias, vec3<f32>( 0.0689, -0.1598,  0.8547));
    occlusion += sample_occlusion(view_pos, tbn, radius, bias, vec3<f32>( 0.0560,  0.0069,  0.1843));
    occlusion += sample_occlusion(view_pos, tbn, radius, bias, vec3<f32>(-0.0146,  0.1402,  0.0762));
    occlusion += sample_occlusion(view_pos, tbn, radius, bias, vec3<f32>( 0.0100, -0.1924,  0.0344));
    return occlusion / 8.0;
}

@fragment
fn fs_ssao(in: VertexOutput) -> @location(0) vec4<f32> {
    let g = textureSample(t_gbuffer, s_lin, in.uv);
    let depth_norm = g.a;    // depth в .a от gbuffer_normal
    if (depth_norm >= 0.9999) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }
    let normal = normalize(g.rgb);
    let view_pos = reconstruct_view_pos(in.uv, depth_norm);
    let ao = compute_occlusion(view_pos, normal, in.uv);
    return vec4<f32>(1.0 - ao, 0.0, 0.0, 1.0);
}

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