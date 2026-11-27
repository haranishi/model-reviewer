//! 正しい STL の読み込み。

use std::path::Path;

use glam::Vec3;
use model_reviewer_core::scene::{LoadWarning, Model};
use model_reviewer_core::units::{UnitLabel, UpAxis};
use model_reviewer_stl::{STL_COLOR, StlError, StlKind, StlOptions, detect, load_stl};
use model_reviewer_testdata::stl::{ascii_stl, binary_stl, box_triangles};
use model_reviewer_testdata::{
    STL_SAMPLE_SIZE, STL_SAMPLE_TRIANGLES, STL_SAMPLE_VERTICES, stl_inputs, stl_sample, stl_stress,
};

fn options(unit: UnitLabel) -> StlOptions<'static> {
    StlOptions {
        name: "箱",
        unit,
        up: UpAxis::default(),
        allow_trailing: false,
    }
}

fn load(bytes: &[u8]) -> Model {
    load_stl(bytes, &options(UnitLabel::Mm)).unwrap_or_else(|error| panic!("{error:?}"))
}

fn close(a: f32, b: f32) -> bool {
    let scale = a.abs().max(b.abs());
    if scale < 1.0 {
        (a - b).abs() <= 1.0e-6
    } else {
        (a - b).abs() <= 1.0e-5 * scale
    }
}

fn assert_close_vec(actual: Vec3, expected: Vec3) {
    for axis in 0..3 {
        assert!(
            close(actual[axis], expected[axis]),
            "{actual:?} != {expected:?}"
        );
    }
}

fn sample_ascii(newline: &str) -> Vec<u8> {
    ascii_stl(&[("box", &box_triangles(STL_SAMPLE_SIZE))], newline)
}

#[test]
fn binary_and_ascii_give_the_same_model() {
    let binary = load(&stl_sample());
    for newline in ["\n", "\r\n", "\r"] {
        let ascii_bytes = sample_ascii(newline);
        assert_eq!(detect(&ascii_bytes, false), Some(StlKind::Ascii));
        let ascii = load(&ascii_bytes);
        assert_eq!(ascii.primitives, binary.primitives, "{newline:?}");
        assert_eq!(ascii.nodes, binary.nodes);
        assert_eq!(ascii.draws, binary.draws);
        assert_eq!(ascii.stats, binary.stats);
        assert_eq!(ascii.bounds, binary.bounds);
        assert!(ascii.warnings.is_empty());
    }
}

#[test]
fn the_box_has_eight_vertices_and_twelve_triangles() {
    let model = load(&stl_sample());
    assert_eq!(detect(&stl_sample(), false), Some(StlKind::Binary));
    assert_eq!(model.nodes.len(), 1);
    assert_eq!(model.draws.len(), 1);
    assert_eq!(model.primitives.len(), 1);
    let prim = &model.primitives[0];
    assert_eq!(prim.vertices.len(), STL_SAMPLE_VERTICES as usize);
    assert_eq!(prim.indices.len(), 3 * STL_SAMPLE_TRIANGLES as usize);
    assert!(!prim.has_normals);
    assert!(prim.vertices.iter().all(|vertex| vertex.normal == [0.0; 3]));
    // 座標の値はファイルのまま（ビットで同じ）。
    assert_eq!(prim.local_aabb.min, Vec3::ZERO);
    assert_eq!(prim.local_aabb.max, Vec3::from(STL_SAMPLE_SIZE));
    assert_eq!(model.stats.nodes, 1);
    assert_eq!(model.stats.meshes, 1);
    assert_eq!(model.stats.primitives, 1);
    assert_eq!(
        model.stats.unique_triangles,
        u64::from(STL_SAMPLE_TRIANGLES)
    );
    assert_eq!(
        model.stats.placed_triangles,
        u64::from(STL_SAMPLE_TRIANGLES)
    );
    let node = &model.nodes[0];
    assert_eq!(node.name, "箱");
    assert_eq!(node.gltf_index, 0);
    assert_eq!(node.parent, None);
    assert_eq!(node.draws, 0..1);
    assert_eq!(model.draws[0].base_color, STL_COLOR);
    assert!(model.warnings.is_empty(), "{:?}", model.warnings);
}

#[test]
fn binary_starting_with_solid_is_binary() {
    let bytes = stl_inputs::binary_starting_with_solid();
    assert!(bytes.starts_with(b"solid"));
    assert_eq!(detect(&bytes, false), Some(StlKind::Binary));
    let model = load(&bytes);
    assert_eq!(model.stats.unique_triangles, 12);
    assert_eq!(model.primitives[0].vertices.len(), 8);
}

#[test]
fn several_solids_become_one_part_with_a_warning() {
    for newline in ["\n", "\r\n"] {
        let model = load(&stl_inputs::ascii_two_solids(newline));
        assert_eq!(model.nodes.len(), 1);
        assert_eq!(model.stats.unique_triangles, 24);
        assert_eq!(model.primitives[0].vertices.len(), 16);
        assert_eq!(
            model.warnings,
            vec![LoadWarning::StlSolidsMerged { count: 2 }]
        );
    }
}

#[test]
fn ascii_accepts_bom_case_and_extra_spaces() {
    let text = "\u{feff}  SOLID Name With Spaces\r\n\tFacet Normal nan 0 1\r\n Outer   Loop\r\n  VERTEX 0 0 0\r\n vertex 1e0 0 0\r\nvertex 0 1 0\r\n ENDLOOP\r\nendfacet\r\n\r\nEndSolid\r\n";
    let model = load(text.as_bytes());
    assert_eq!(model.stats.unique_triangles, 1);
    assert_eq!(model.primitives[0].vertices.len(), 3);
}

#[test]
fn unit_becomes_node_scale() {
    for (unit, meters) in [
        (UnitLabel::Mm, 0.001f32),
        (UnitLabel::Cm, 0.01),
        (UnitLabel::M, 1.0),
    ] {
        let model = load_stl(&stl_sample(), &options(unit)).unwrap();
        let node = &model.nodes[0];
        assert_close_vec(
            node.local.matrix3.x_axis.into(),
            Vec3::new(meters, 0.0, 0.0),
        );
        assert_close_vec(
            node.local.matrix3.y_axis.into(),
            Vec3::new(0.0, 0.0, -meters),
        );
        assert_close_vec(
            node.local.matrix3.z_axis.into(),
            Vec3::new(0.0, meters, 0.0),
        );
        assert_eq!(Vec3::from(node.local.translation), Vec3::ZERO);
        assert_eq!(node.world, node.local);
        // 形状の数値は単位によらず同じ。
        assert_eq!(
            model.primitives[0].local_aabb.max,
            Vec3::from(STL_SAMPLE_SIZE)
        );
        let size = Vec3::from(STL_SAMPLE_SIZE) * meters;
        assert_close_vec(model.bounds.max, Vec3::new(size.x, size.z, 0.0));
        assert_close_vec(model.bounds.min, Vec3::new(0.0, 0.0, -size.y));
        assert_eq!(model.bounds, node.world_aabb);
    }
}

#[test]
fn z_up_puts_file_z_on_world_y() {
    let model = load(&stl_sample());
    let world = model.nodes[0].world;
    assert_close_vec(world.transform_point3(Vec3::Z * 40.0), Vec3::Y * 0.040);
    assert_close_vec(
        (model.bounds.max - model.bounds.min) * 1000.0,
        Vec3::new(20.0, 40.0, 30.0),
    );
}

#[test]
fn y_up_keeps_file_coordinates() {
    let opts = StlOptions {
        up: UpAxis::Y,
        ..options(UnitLabel::Mm)
    };
    let model = load_stl(&stl_sample(), &opts).unwrap();
    let local = model.nodes[0].local;
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        assert_close_vec(local.transform_vector3(axis), axis * 0.001);
    }
    assert_close_vec(local.translation.into(), Vec3::ZERO);
    assert_close_vec(model.bounds.min, Vec3::ZERO);
    assert_close_vec(model.bounds.max, Vec3::from(STL_SAMPLE_SIZE) * 0.001);
}

#[test]
fn up_rotation_keeps_positions_and_winding() {
    let bytes = stl_sample();
    let models = [UpAxis::Z, UpAxis::Y].map(|up| {
        load_stl(
            &bytes,
            &StlOptions {
                up,
                ..options(UnitLabel::Mm)
            },
        )
        .unwrap()
    });
    for model in &models {
        assert!(model.nodes[0].local.matrix3.determinant() > 0.0);
    }
    let [a, b] = models.each_ref().map(|model| &model.primitives[0]);
    assert_eq!(a.vertices.len(), b.vertices.len());
    for (left, right) in a.vertices.iter().zip(&b.vertices) {
        assert_eq!(
            left.position.map(f32::to_bits),
            right.position.map(f32::to_bits)
        );
    }
    assert_eq!(a.indices, b.indices);
}

#[test]
fn the_same_input_gives_the_same_model() {
    let bytes = stl_stress(2_000, false);
    assert_eq!(load(&bytes), load(&bytes));
    let ascii = stl_stress(500, true);
    assert_eq!(load(&ascii), load(&ascii));
}

#[test]
fn stress_input_welds_to_about_half_the_triangles() {
    let triangles = 20_000;
    let binary = load(&stl_stress(triangles, false));
    let ascii = load(&stl_stress(triangles, true));
    assert_eq!(binary.primitives, ascii.primitives);
    assert_eq!(binary.stats.unique_triangles, u64::from(triangles));
    let vertices = binary.primitives[0].vertices.len() as u32;
    assert!(vertices < triangles, "{vertices}");
    assert!(vertices > triangles / 4, "{vertices}");
}

#[test]
fn zero_area_triangles_are_kept() {
    let mut triangles = box_triangles([1.0, 1.0, 1.0]);
    triangles.push([[0.5, 0.5, 0.5]; 3]);
    triangles.push([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
    let model = load(&binary_stl(b"flat", &triangles));
    assert_eq!(model.stats.unique_triangles, 14);
    assert_eq!(model.primitives[0].indices.len(), 42);
}

#[test]
fn normals_and_attribute_bytes_are_ignored() {
    let mut bytes = stl_sample();
    for triangle in 0..STL_SAMPLE_TRIANGLES as usize {
        let base = 84 + triangle * 50;
        bytes[base..base + 12].copy_from_slice(&[0xFF; 12]); // NaN の法線
        bytes[base + 48..base + 50].copy_from_slice(&[0x12, 0x34]);
    }
    assert_eq!(load(&bytes), load(&stl_sample()));
}

#[test]
fn trailing_bytes_need_permission_and_are_reported() {
    let bytes = stl_inputs::binary_with_trailing(7);
    assert_eq!(detect(&bytes, false), None);
    assert_eq!(detect(&bytes, true), Some(StlKind::Binary));
    assert_eq!(
        load_stl(&bytes, &options(UnitLabel::Mm)),
        Err(StlError::NotStl)
    );
    let allowed = StlOptions {
        allow_trailing: true,
        ..options(UnitLabel::Mm)
    };
    let model = load_stl(&bytes, &allowed).unwrap();
    assert_eq!(model.stats.unique_triangles, 12);
    assert_eq!(
        model.warnings,
        vec![LoadWarning::StlTrailingBytes { bytes: 7 }]
    );
}

#[test]
fn the_sample_file_matches_the_generator() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("samples")
        .join("review-sample.stl");
    let bytes = std::fs::read(&path).expect("見本の STL を読めません");
    assert_eq!(bytes, stl_sample());
}
