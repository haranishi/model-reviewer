mod broken;
mod glb_writer;
pub mod metadata;
mod pattern;
pub mod rich;
mod sample;
mod shapes;
pub mod stl;
mod stress;
mod textured;

pub use pattern::{Pattern, jpeg_8x8, pattern_png};
pub use rich::rich_glb;
pub use sample::{
    EXPECTED_WORLD_POSITIONS, MESH_COUNT, NODE_COUNT, NODE_NAMES, PLACED_TRIANGLES,
    PRIMITIVE_COUNT, UNIQUE_TRIANGLES, bytes as sample_bytes,
};
pub use stl::{
    STL_SAMPLE_SIZE, STL_SAMPLE_TRIANGLES, STL_SAMPLE_VERTICES, stl_inputs, stl_sample, stl_stress,
};
pub use stress::{nested_descendants, placed_triangles, stress_glb, stress_nested_glb};
pub use textured::{
    QuadImage, SamplerOptions, TextureTransform, TexturedOptions, UvFormat, textured_quad_glb,
    textured_scene_glb,
};

pub fn sample_glb() -> Vec<u8> {
    sample::bytes()
}

pub fn broken_glb() -> Vec<u8> {
    broken::bytes()
}

pub fn stress_note() -> &'static str {
    stress::note()
}

pub mod inputs {
    pub use crate::broken::{
        allowlisted_emissive, allowlisted_unlit, alpha_blend, animation_skin_morph, bad_json,
        cyclic_nodes, data_uri_buffer, deep_chain, external_buffer, huge_vertex_count, index_oob,
        inf_positions, lines_and_points, lines_only, missing_bin, nan_positions, no_mesh,
        optional_draco_with_fallback, optional_draco_without_position,
        optional_meshopt_with_fallback, required_extension, scene_root_is_child, sparse_accessor,
        textures, triangle, triangle_fan, triangle_strip, truncated, two_parents, unknown_optional,
        vertex_colors, view_past_bin, wrong_magic, zero_count,
    };
}
