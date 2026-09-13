@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var s_src: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct Params {
    // Для bright-pass: x = threshold
    // Для blur: xy = direction, zw = texel_size
    values: vec4<f32>,
};

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

// === Bright-pass: оставить только то, что выше порога ===
@fragment
fn fs_bright(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(t_src, s_src, in.uv).rgb;
    let brightness = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    let threshold = params.values.x;
    let soft = max(brightness - threshold, 0.0);
    let weight = soft / max(brightness, 0.0001);
    return vec4<f32>(c * weight, 1.0);
}

// === Separable Gaussian blur (9 taps, sigma ~ 2.0), развёрнутый цикл ===
@fragment
fn fs_blur(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = params.values.xy;
    let texel = params.values.zw;

    var result = textureSample(t_src, s_src, in.uv).rgb * 0.2270270270;

    let o1 = dir * texel * 1.0;
    result = result + textureSample(t_src, s_src, in.uv + o1).rgb * 0.1945945946;
    result = result + textureSample(t_src, s_src, in.uv - o1).rgb * 0.1945945946;

    let o2 = dir * texel * 2.0;
    result = result + textureSample(t_src, s_src, in.uv + o2).rgb * 0.1216216216;
    result = result + textureSample(t_src, s_src, in.uv - o2).rgb * 0.1216216216;

    let o3 = dir * texel * 3.0;
    result = result + textureSample(t_src, s_src, in.uv + o3).rgb * 0.0540540541;
    result = result + textureSample(t_src, s_src, in.uv - o3).rgb * 0.0540540541;

    let o4 = dir * texel * 4.0;
    result = result + textureSample(t_src, s_src, in.uv + o4).rgb * 0.0162162162;
    result = result + textureSample(t_src, s_src, in.uv - o4).rgb * 0.0162162162;

    return vec4<f32>(result, 1.0);
}