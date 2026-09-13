//! Загрузчик glTF 2.0 с поддержкой PBR metallic-roughness.

use anyhow::{Context, Result};
use glam::Mat4;
use std::collections::HashMap;
use std::path::Path;

use super::material::Material;
use super::mesh::{Mesh, Vertex3D};
use super::renderer::Renderer;

pub struct GltfInstance {
    pub mesh_name: String,
    pub material_name: String,
    pub model: Mat4,
}

pub fn load_gltf_into(
    renderer: &mut Renderer,
    path: impl AsRef<Path>,
    prefix: &str,
) -> Result<Vec<GltfInstance>> {
    let path = path.as_ref();
    let (doc, buffers, images) = gltf::import(path)
        .with_context(|| format!("failed to import glTF: {}", path.display()))?;

    // === Текстуры ===
    let mut texture_name_map: HashMap<usize, String> = HashMap::new();
    for (i, img) in images.iter().enumerate() {
        let name = format!("{}_tex_{}", prefix, i);
        let data = &img.pixels;
        let w = img.width;
        let h = img.height;

        match img.format {
            gltf::image::Format::R8G8B8A8 => {
                renderer.load_texture_rgba(&name, data, w, h)?;
            }
            gltf::image::Format::R8G8B8 => {
                let mut rgba = Vec::with_capacity((w * h * 4) as usize);
                for px in data.chunks_exact(3) {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
                renderer.load_texture_rgba(&name, &rgba, w, h)?;
            }
            _ => continue,
        }
        texture_name_map.insert(i, name);
    }

    // === Материалы ===
    let mut material_name_map: HashMap<usize, String> = HashMap::new();
    for (i, mat) in doc.materials().enumerate() {
        let pbr = mat.pbr_metallic_roughness();
        let base = pbr.base_color_factor();

        let mut material = Material::new([base[0], base[1], base[2], base[3]]);
        material.metallic = pbr.metallic_factor();
        material.roughness = pbr.roughness_factor();

        // Base color texture
        if let Some(tex_info) = pbr.base_color_texture() {
            let img_idx = tex_info.texture().source().index();
            if let Some(name) = texture_name_map.get(&img_idx) {
                material.base_color_texture = Some(name.clone());
            }
        }

        // Metallic-roughness texture
        if let Some(tex_info) = pbr.metallic_roughness_texture() {
            let img_idx = tex_info.texture().source().index();
            if let Some(name) = texture_name_map.get(&img_idx) {
                material.metallic_roughness_texture = Some(name.clone());
            }
        }

        // Normal texture
        if let Some(normal_info) = mat.normal_texture() {
            let img_idx = normal_info.texture().source().index();
            if let Some(name) = texture_name_map.get(&img_idx) {
                material.normal_texture = Some(name.clone());
                material.normal_scale = normal_info.scale();
            }
        }

        // Emissive
        let em = mat.emissive_factor();
        material.emissive = [em[0], em[1], em[2]];
        if let Some(tex_info) = mat.emissive_texture() {
            let img_idx = tex_info.texture().source().index();
            if let Some(name) = texture_name_map.get(&img_idx) {
                material.emissive_texture = Some(name.clone());
            }
        }

        let name = format!("{}_mat_{}", prefix, i);
        renderer.add_material(&name, material);
        material_name_map.insert(i, name);
    }

    if material_name_map.is_empty() {
        let name = format!("{}_mat_default", prefix);
        renderer.add_material(&name, Material::default());
        material_name_map.insert(usize::MAX, name);
    }

    // === Меши + инстансы ===
    let mut instances = Vec::new();

    for node in doc.nodes() {
        let Some(mesh) = node.mesh() else { continue };
        let node_transform = Mat4::from_cols_array_2d(&node.transform().matrix());

        for (p_idx, primitive) in mesh.primitives().enumerate() {
            let reader = primitive.reader(|b| Some(&buffers[b.index()]));

            let positions: Vec<[f32; 3]> = match reader.read_positions() {
                Some(iter) => iter.collect(),
                None => continue,
            };
            if positions.is_empty() {
                continue;
            }

            let normals: Vec<[f32; 3]> = reader
                .read_normals()
                .map(|iter| iter.collect())
                .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);

            let uvs: Vec<[f32; 2]> = reader
                .read_tex_coords(0)
                .map(|tc| tc.into_f32().collect())
                .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);

            let indices: Vec<u32> = reader
                .read_indices()
                .map(|i| i.into_u32().collect())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());

            let mut vertices = Vec::with_capacity(positions.len());
            for i in 0..positions.len() {
                vertices.push(Vertex3D {
                    position: positions[i],
                    normal: normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
                    uv: uvs.get(i).copied().unwrap_or([0.0, 0.0]),
                    color: [1.0, 1.0, 1.0, 1.0],
                });
            }

            let mesh_name = format!("{}_mesh_{}_{}", prefix, mesh.index(), p_idx);
            let mesh_obj = Mesh::new(&renderer.device, &vertices, &indices, &mesh_name);
            renderer.add_mesh(&mesh_name, mesh_obj);

            let material_name = primitive
                .material()
                .index()
                .and_then(|idx| material_name_map.get(&idx).cloned())
                .or_else(|| material_name_map.get(&usize::MAX).cloned())
                .unwrap_or_else(|| format!("{}_mat_default", prefix));

            instances.push(GltfInstance {
                mesh_name,
                material_name,
                model: node_transform,
            });
        }
    }

    Ok(instances)
}