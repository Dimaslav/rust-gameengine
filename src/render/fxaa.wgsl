struct Params {
    values: vec4<f32>,
};

@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var s_src: sampler;
@group(0) @binding(2) var<uniform> params: Params;

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

fn luma(c: vec3<f32>) -> f32 {
    // Rec.709 — точнее для HDR-контента, чем Rec.601.
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let inv = params.values.xy;
    let strength = params.values.z;

    let rgbM = textureSample(t_src, s_src, in.uv).rgb;

    if (strength < 0.001) {
        return vec4<f32>(rgbM, 1.0);
    }

    // Соседи (N/S/E/W).
    let rgbN = textureSample(t_src, s_src, in.uv + vec2<f32>(0.0, -inv.y)).rgb;
    let rgbS = textureSample(t_src, s_src, in.uv + vec2<f32>(0.0,  inv.y)).rgb;
    let rgbE = textureSample(t_src, s_src, in.uv + vec2<f32>( inv.x, 0.0)).rgb;
    let rgbW = textureSample(t_src, s_src, in.uv + vec2<f32>(-inv.x, 0.0)).rgb;

    // Диагонали.
    let rgbNW = textureSample(t_src, s_src, in.uv + vec2<f32>(-inv.x, -inv.y)).rgb;
    let rgbNE = textureSample(t_src, s_src, in.uv + vec2<f32>( inv.x, -inv.y)).rgb;
    let rgbSW = textureSample(t_src, s_src, in.uv + vec2<f32>(-inv.x,  inv.y)).rgb;
    let rgbSE = textureSample(t_src, s_src, in.uv + vec2<f32>( inv.x,  inv.y)).rgb;

    let lM  = luma(rgbM);
    let lN  = luma(rgbN);
    let lS  = luma(rgbS);
    let lE  = luma(rgbE);
    let lW  = luma(rgbW);
    let lNW = luma(rgbNW);
    let lNE = luma(rgbNE);
    let lSW = luma(rgbSW);
    let lSE = luma(rgbSE);

    let lMin = min(lM, min(min(lNW, lNE), min(lSW, lSE)));
    let lMax = max(lM, max(max(lNW, lNE), max(lSW, lSE)));

    // Low-contrast bypass: на очень плавных переходах не трогаем.
    // Порог относительный — 1/24 от lMax. Это компенсирует
    // HDR-разброс яркости (небо vs земля).
    let threshold = lMax * 0.0417; // 1/24
    if (lMax - lMin < max(threshold, 0.005)) {
        return vec4<f32>(rgbM, 1.0);
    }

    // Направление edge по производным лума.
    let dirX = -((lNW + lNE) - (lSW + lSE));
    let dirY =  ((lNW + lSW) - (lNE + lSE));
    let dirReduce = max((lNW + lNE + lSW + lSE) * 0.25 * 0.5, 0.0001);
    let rcpDirMin = 1.0 / (min(abs(dirX), abs(dirY)) + dirReduce);

    var dir = vec2<f32>(
        clamp(dirX * rcpDirMin, -8.0, 8.0) * inv.x,
        clamp(dirY * rcpDirMin, -8.0, 8.0) * inv.y,
    );

    // Сэмплы вдоль edge (симметрично ±0.5, ±1.0, ±1.5, ±2.0 пикселя).
    let uv1 = in.uv + dir * (1.0 / 3.0 - 0.5);
    let uv2 = in.uv + dir * (2.0 / 3.0 - 0.5);
    let rgbA = 0.5 * (
        textureSample(t_src, s_src, uv1).rgb +
        textureSample(t_src, s_src, uv2).rgb
    );

    let uv3 = in.uv + dir * -0.5;
    let uv4 = in.uv + dir *  0.5;
    let rgbB = rgbA * 0.5 + 0.25 * (
        textureSample(t_src, s_src, uv3).rgb +
        textureSample(t_src, s_src, uv4).rgb
    );

    let lB = luma(rgbB);
    let fxaa_color = select(rgbB, rgbA, lB < lMin || lB > lMax);

    return vec4<f32>(mix(rgbM, fxaa_color, strength), 1.0);
}