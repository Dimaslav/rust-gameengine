// Volumetric fog composite.
//
// Читает HDR + froxel-texture + G-buffer depth. Для каждого пикселя:
//   1. Линеаризовать depth → view-space distance
//   2. Перевести в froxel-z coordinate (экспоненциальная инверсия)
//   3. Sample froxel-texture с bilinear filter (смешивает соседние слои)
//   4. result = hdr * exp(-extinction) + scattering
//
// Результат идёт в отдельный hdr_fog_view, откуда его читает TAA.

struct Volumetric {
    grid: vec4<f32>,
    cam: vec4<f32>,
    params: vec4<f32>,
    fog_color: vec4<f32>,
};

@group(0) @binding(0) var t_hdr:   texture_2d<f32>;
@group(0) @binding(1) var t_fog:   texture_3d<f32>;
@group(0) @binding(2) var s_lin:   sampler;
@group(0) @binding(3) var t_depth: texture_depth_2d;
@group(0) @binding(4) var<uniform> vol: Volumetric;

const FROXEL_Z: f32 = 64.0;

struct VOut {
    @builtin(position) frag_coord: vec4<f32>,
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
    out.frag_coord = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let hdr = textureSample(t_hdr, s_lin, in.uv).rgb;

    if (vol.params.x <= 0.0) {
        return vec4<f32>(hdr, 1.0);
    }

    let depth_raw = textureSample(t_depth, s_lin, in.uv);

    let near = vol.cam.x;
    let far = vol.cam.y;

    // Perspective depth linearization (wgpu NDC z ∈ [0, 1]):
    //   ndc_z = far * (d - near) / (d * (far - near))
    //   d = near * far / (far - ndc_z * (far - near))
    let view_z = (near * far) / max(far - depth_raw * (far - near), 1e-4);

    // Froxel z coordinate (inverse of the compute-side mapping).
    let ratio = far / near;
    let slice_f = clamp(log(view_z / near) / log(ratio), 0.0, 1.0);

    // Sample froxel texture — bilinear через слои.
    let z_uv = slice_f * (FROXEL_Z - 1.0) / FROXEL_Z + 0.5 / FROXEL_Z;
    let fog = textureSampleLevel(
        t_fog,
        s_lin,
        vec3<f32>(in.uv.x, in.uv.y, z_uv),
        0.0,
    );

    let scattering = fog.rgb;
    let extinction = fog.a;
    let transmittance = exp(-extinction);

    let result = hdr * transmittance + scattering;
    return vec4<f32>(result, 1.0);
}