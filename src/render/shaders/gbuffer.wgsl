// G-buffer pass. MRT: albedo, view-normal + linear-depth, emissive.
// Bind groups: 0=camera, 1=material(+skeleton).
//
// Normal map применяется через derivative-based TBN (cotangent frame).
//
// UV масштабируется на `uv_scale.xy` (per-instance). Это позволяет
// текстуре тайлиться при масштабировании объекта, а не растягиваться.

struct Camera {
    view_proj:     mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    view:          mat4x4<f32>,
    inv_view:      mat4x4<f32>,
    camera_pos:    vec4<f32>,
    near_far:      vec4<f32>,
};

struct Material {
    base_color: vec4<f32>,
    emissive:   vec4<f32>,
    params:     vec4<f32>,   // metallic, roughness, normal_scale, alpha_cutoff
    flags:      vec4<u32>,   // alpha_mode (0=Opaque, 1=Mask, 2=Blend), _, _, _
};

struct Skeleton {
    joints: array<mat4x4<f32>, 64>,
};

struct Instance {
    model:         mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
    color:         vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var base_tex:     texture_2d<f32>;
@group(1) @binding(1) var mr_tex:       texture_2d<f32>;
@group(1) @binding(2) var normal_tex:   texture_2d<f32>;
@group(1) @binding(3) var emissive_tex: texture_2d<f32>;
@group(1) @binding(4) var mat_samp:     sampler;
@group(1) @binding(5) var<uniform> mat: Material;
@group(1) @binding(6) var<uniform> skeleton: Skeleton;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
    @location(3) color:    vec4<f32>,
    @location(4) joints:   vec4<u32>,
    @location(5) weights:  vec4<f32>,
    // instance
    @location(6)  m0: vec4<f32>,
    @location(7)  m1: vec4<f32>,
    @location(8)  m2: vec4<f32>,
    @location(9)  m3: vec4<f32>,
    @location(10) n0: vec4<f32>,
    @location(11) n1: vec4<f32>,
    @location(12) n2: vec4<f32>,
    @location(13) n3: vec4<f32>,
    @location(14) inst_color: vec4<f32>,
    @location(15) uv_scale:   vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,        // уже умноженный на uv_scale
    @location(3) color: vec4<f32>,
    @location(4) view_depth: f32,
};

fn skin_matrix(joints: vec4<u32>, weights: vec4<f32>) -> mat4x4<f32> {
    let w_sum = weights.x + weights.y + weights.z + weights.w;
    if (w_sum < 0.0001) {
        return mat4x4<f32>(
            vec4<f32>(1.0, 0.0, 0.0, 0.0),
            vec4<f32>(0.0, 1.0, 0.0, 0.0),
            vec4<f32>(0.0, 0.0, 1.0, 0.0),
            vec4<f32>(0.0, 0.0, 0.0, 1.0),
        );
    }
    return weights.x * skeleton.joints[joints.x]
         + weights.y * skeleton.joints[joints.y]
         + weights.z * skeleton.joints[joints.z]
         + weights.w * skeleton.joints[joints.w];
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    let nrm_mat = mat4x4<f32>(in.n0, in.n1, in.n2, in.n3);

    var local_pos = vec4<f32>(in.position, 1.0);
    var local_nrm = vec4<f32>(in.normal, 0.0);

    let w_sum = in.weights.x + in.weights.y + in.weights.z + in.weights.w;
    if (w_sum > 0.0001) {
        let sk = skin_matrix(in.joints, in.weights);
        local_pos = sk * local_pos;
        local_nrm = sk * local_nrm;
    }

    let world_pos = model * local_pos;
    let world_nrm = normalize((nrm_mat * local_nrm).xyz);

    let view_pos = camera.view * world_pos;

    var out: VsOut;
    out.clip_pos     = camera.view_proj * world_pos;
    out.world_pos    = world_pos.xyz;
    out.world_normal = world_nrm;
    // Масштабируем UV до fragment-сэмплинга: textureSample использует
    // уже умноженное значение → тайлинг без растяжения.
    out.uv           = in.uv * in.uv_scale.xy;
    out.color        = in.color * in.inst_color;
    out.view_depth   = -view_pos.z;
    return out;
}

fn cotangent_frame(N: vec3<f32>, p: vec3<f32>, uv: vec2<f32>) -> mat3x3<f32> {
    let dp1 = dpdx(p);
    let dp2 = dpdy(p);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);

    let dp2perp = cross(dp2, N);
    let dp1perp = cross(N, dp1);
    let T = dp2perp * duv1.x + dp1perp * duv2.x;
    let B = dp2perp * duv1.y + dp1perp * duv2.y;

    let invmax = inverseSqrt(max(max(dot(T, T), dot(B, B)), 0.0001));
    return mat3x3<f32>(T * invmax, B * invmax, N);
}

struct FsOut {
    @location(0) albedo:   vec4<f32>,
    @location(1) normal_d: vec4<f32>,
    @location(2) emissive: vec4<f32>,
};

@fragment
fn fs_main(
    in: VsOut,
    @builtin(front_facing) front_facing: bool,
) -> FsOut {
    let base_sample     = textureSample(base_tex,     mat_samp, in.uv);
    let mr_sample       = textureSample(mr_tex,       mat_samp, in.uv);
    let normal_sample   = textureSample(normal_tex,   mat_samp, in.uv);
    let emissive_sample = textureSample(emissive_tex, mat_samp, in.uv);

    let base = base_sample * mat.base_color * in.color;

    let alpha_mode = mat.flags.x;
    let alpha_cutoff = mat.params.w;
    if (alpha_mode == 1u && base.a < alpha_cutoff) {
        discard;
    }

    var n_world = normalize(in.world_normal);
    if (!front_facing) {
        n_world = -n_world;
    }

    let n_sample = normal_sample.xyz * 2.0 - 1.0;
    let n_scaled = vec3<f32>(
        n_sample.xy * mat.params.z,
        n_sample.z,
    );
    let tbn = cotangent_frame(n_world, in.world_pos, in.uv);
    let n_mapped = normalize(tbn * n_scaled);

    let metallic  = clamp(mat.params.x * mr_sample.b, 0.0, 1.0);
    let roughness = clamp(mat.params.y * mr_sample.g, 0.04, 1.0);

    let emissive = emissive_sample.rgb * mat.emissive.rgb;

    let n_view = normalize((camera.view * vec4<f32>(n_mapped, 0.0)).xyz);

    let depth_norm = clamp(in.view_depth / camera.near_far.y, 0.0, 1.0);

    var out: FsOut;
    out.albedo   = vec4<f32>(base.rgb, metallic);
    out.normal_d = vec4<f32>(n_view, depth_norm);
    out.emissive = vec4<f32>(emissive, roughness);
    return out;
}