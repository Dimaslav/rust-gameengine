@group(0) @binding(0) var t_hdr: texture_2d<f32>;
@group(0) @binding(1) var t_bloom: texture_2d<f32>;
@group(0) @binding(2) var s_lin: sampler;
@group(0) @binding(3) var<uniform> params: Params;

struct Params {
    // x = bloom strength, y = exposure, z = time (для grain), w = unused
    values: vec4<f32>,
    // x = vignette_strength, y = grain_strength, z = chromatic_strength, w = unused
    effects: vec4<f32>,
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

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var uv = in.uv;

    let chromatic = params.effects.z;
    var hdr: vec3<f32>;
    if (chromatic > 0.0001) {
        let center = vec2<f32>(0.5, 0.5);
        let to_center = uv - center;
        let r2 = dot(to_center, to_center);
        let shift = chromatic * r2 * 0.02;
        let dir = to_center;
        let r = textureSample(t_hdr, s_lin, uv + dir * shift).r;
        let g = textureSample(t_hdr, s_lin, uv).g;
        let b = textureSample(t_hdr, s_lin, uv - dir * shift).b;
        hdr = vec3<f32>(r, g, b);
    } else {
        hdr = textureSample(t_hdr, s_lin, uv).rgb;
    }

    let bloom = textureSample(t_bloom, s_lin, uv).rgb;
    let strength = params.values.x;
    hdr = hdr + bloom * strength;

    hdr = hdr * params.values.y;

    // ACES tonemap → linear [0, 1].
    // sRGB-энкодинг делает сам GPU (LDR_FORMAT = Rgba8UnormSrgb).
    var out_rgb = aces(hdr);

    let vignette = params.effects.x;
    if (vignette > 0.0001) {
        let center = vec2<f32>(0.5, 0.5);
        let d = distance(uv, center) * 1.4142;
        let v = smoothstep(1.0, 0.3, d * (1.0 + vignette));
        out_rgb = out_rgb * mix(1.0, v, vignette);
    }

    let grain = params.effects.y;
    if (grain > 0.0001) {
        let n = hash12(uv * 1024.0 + params.values.z);
        let g = (n - 0.5) * grain * 0.15;
        out_rgb = clamp(out_rgb + g, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    return vec4<f32>(out_rgb, 1.0);
}