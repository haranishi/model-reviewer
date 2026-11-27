use model_reviewer_core::scene::LoadWarning;
use model_reviewer_gltf::{ExtensionKind, LoadError, load_glb};
use model_reviewer_testdata::inputs;
use model_reviewer_testdata::{placed_triangles, sample_glb, stress_glb};

#[test]
fn sparse_accessor_is_unsupported_not_corrupt() {
    let error = load_err(&inputs::sparse_accessor());
    assert_eq!(error.message_ja(), "未対応の形式です（sparse アクセサ）");
    assert!(!error.message_ja().contains("破損"));
}

fn load_err(bytes: &[u8]) -> LoadError {
    match load_glb(bytes) {
        Ok(_) => panic!("expected an error"),
        Err(error) => error,
    }
}

fn assert_order(warnings: &[LoadWarning]) {
    fn rank(warning: &LoadWarning) -> u8 {
        match warning {
            LoadWarning::Textures { .. } => 0,
            LoadWarning::VertexColors { .. } => 1,
            LoadWarning::AlphaBlend { .. } => 2,
            LoadWarning::Animations { .. } => 3,
            LoadWarning::Skins { .. } => 4,
            LoadWarning::Morphs { .. } => 5,
            LoadWarning::DerivedNormals { .. } => 6,
            LoadWarning::OmittedLinesAndPoints { .. } => 7,
            LoadWarning::SimplifiedExtension { .. } => 8,
            LoadWarning::UnsupportedOptionalExtension { .. } => 9,
            LoadWarning::StlSolidsMerged { .. } => 10,
            LoadWarning::StlTrailingBytes { .. } => 11,
        }
    }
    let ranks: Vec<_> = warnings.iter().map(rank).collect();
    for pair in ranks.windows(2) {
        assert!(pair[0] <= pair[1], "{warnings:?}");
    }
}

#[test]
fn required_geometry_compression_names_the_reason() {
    for name in ["KHR_draco_mesh_compression", "EXT_meshopt_compression"] {
        let error = load_err(&inputs::required_extension(name));
        match error {
            LoadError::UnsupportedRequiredExtension { ref name, kind } => {
                assert_eq!(kind, ExtensionKind::GeometryCompression);
                assert!(error.message_ja().contains(name));
                assert!(
                    error
                        .message_ja()
                        .contains("形状が圧縮されているため未対応")
                );
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn required_quantization_is_rejected() {
    let error = load_err(&inputs::required_extension("KHR_mesh_quantization"));
    match error {
        LoadError::UnsupportedRequiredExtension { kind, .. } => {
            assert_eq!(kind, ExtensionKind::Quantization);
            assert!(error.message_ja().contains("量子化された頂点形式は未対応"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn required_instancing_is_rejected() {
    let error = load_err(&inputs::required_extension("EXT_mesh_gpu_instancing"));
    match error {
        LoadError::UnsupportedRequiredExtension { kind, .. } => {
            assert_eq!(kind, ExtensionKind::Instancing);
            assert!(
                error
                    .message_ja()
                    .contains("拡張によるインスタンス配置は未対応")
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn unknown_required_extension_includes_the_name() {
    let error = load_err(&inputs::required_extension("TEST_unknown_extension"));
    match error {
        LoadError::UnsupportedRequiredExtension { ref name, kind } => {
            assert_eq!(kind, ExtensionKind::Unknown);
            assert_eq!(name, "TEST_unknown_extension");
            assert!(error.message_ja().contains("未知の必須拡張"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn external_uri_and_data_uri_are_rejected() {
    for bytes in [inputs::external_buffer(), inputs::data_uri_buffer()] {
        let error = load_err(&bytes);
        assert!(matches!(error, LoadError::ExternalBuffer));
        assert_eq!(error.message_ja(), "外部バッファは未対応です");
    }
}

#[test]
fn missing_bin_is_rejected() {
    let error = load_err(&inputs::missing_bin());
    assert!(matches!(error, LoadError::MissingBin));
    assert_eq!(error.message_ja(), "BINチャンクがありません");
}

#[test]
fn range_and_count_mismatches_are_bad_data() {
    let past = load_err(&inputs::view_past_bin());
    assert!(matches!(past, LoadError::BadData { .. }));
    assert!(past.message_ja().contains("データが破損しています"));
    let zero = load_err(&inputs::zero_count());
    match zero {
        LoadError::BadData { what } => assert_eq!(what, "個数"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn index_and_non_finite_positions_are_bad_data() {
    match load_err(&inputs::index_oob()) {
        LoadError::BadData { what } => assert_eq!(what, "インデックス"),
        other => panic!("{other:?}"),
    }
    match load_err(&inputs::nan_positions()) {
        LoadError::BadData { what } => assert_eq!(what, "位置"),
        other => panic!("{other:?}"),
    }
    match load_err(&inputs::inf_positions()) {
        LoadError::BadData { what } => assert_eq!(what, "位置"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn node_graph_errors_cover_cycles_two_parents_and_a_child_root() {
    match load_err(&inputs::cyclic_nodes()) {
        LoadError::NodeGraph(detail) => assert!(detail.contains("循環")),
        other => panic!("{other:?}"),
    }
    match load_err(&inputs::two_parents()) {
        LoadError::NodeGraph(detail) => assert!(detail.contains("複数の親")),
        other => panic!("{other:?}"),
    }
    match load_err(&inputs::scene_root_is_child()) {
        LoadError::NodeGraph(detail) => assert!(detail.contains("シーンの根")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_mesh_and_lines_only_have_nothing_to_draw() {
    assert!(matches!(load_err(&inputs::no_mesh()), LoadError::NoMesh));
    assert_eq!(
        load_err(&inputs::lines_only()).message_ja(),
        "表示できる形状がありません"
    );
}

#[test]
fn vertex_count_above_u32_is_too_large() {
    let error = load_err(&inputs::huge_vertex_count());
    match error {
        LoadError::TooLarge { ref what } => {
            assert_eq!(what, "頂点数");
            assert!(error.message_ja().contains("大きすぎます"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn appearance_extras_warn_and_still_load() {
    let textures = load_glb(&inputs::textures()).expect("textures");
    assert!(
        textures
            .warnings
            .contains(&LoadWarning::Textures { count: 1 })
    );
    assert_order(&textures.warnings);

    let colors = load_glb(&inputs::vertex_colors()).expect("colors");
    assert!(
        colors
            .warnings
            .contains(&LoadWarning::VertexColors { count: 1 })
    );
    assert_order(&colors.warnings);

    let blend = load_glb(&inputs::alpha_blend()).expect("blend");
    assert!(
        blend
            .warnings
            .contains(&LoadWarning::AlphaBlend { count: 1 })
    );
    assert_order(&blend.warnings);
}

#[test]
fn animation_skin_and_morph_stay_in_the_bind_pose() {
    let model = load_glb(&inputs::animation_skin_morph()).expect("static");
    assert!(
        model
            .warnings
            .contains(&LoadWarning::Animations { count: 1 })
    );
    assert!(model.warnings.contains(&LoadWarning::Skins { count: 1 }));
    assert!(model.warnings.contains(&LoadWarning::Morphs { count: 1 }));
    assert_eq!(model.stats.placed_triangles, 1);
    assert_order(&model.warnings);
}

#[test]
fn lines_and_points_are_omitted_when_a_triangle_remains() {
    let model = load_glb(&inputs::lines_and_points()).expect("mixed");
    assert!(
        model
            .warnings
            .contains(&LoadWarning::OmittedLinesAndPoints {
                lines: 1,
                points: 1
            })
    );
    assert_eq!(model.stats.placed_triangles, 1);
    assert_order(&model.warnings);
}

#[test]
fn missing_normals_mark_the_whole_primitive() {
    let model = load_glb(&sample_glb()).expect("sample");
    let lamp = model
        .nodes
        .iter()
        .find(|node| node.name == "lamp")
        .expect("lamp");
    let draw = model.draws[lamp.draws.start as usize];
    assert!(!model.primitives[draw.prim.0 as usize].has_normals);
    assert!(
        model
            .warnings
            .contains(&LoadWarning::DerivedNormals { count: 1 })
    );
}

#[test]
fn unknown_optional_extension_is_listed() {
    let model = load_glb(&inputs::unknown_optional("TEST_optional")).expect("optional");
    assert!(
        model
            .warnings
            .contains(&LoadWarning::UnsupportedOptionalExtension {
                name: "TEST_optional".to_owned()
            })
    );
    assert_order(&model.warnings);
}

#[test]
fn allowlisted_required_extensions_load_with_a_simplification_warning() {
    let unlit = load_glb(&inputs::allowlisted_unlit()).expect("unlit");
    assert!(unlit.warnings.contains(&LoadWarning::SimplifiedExtension {
        name: "KHR_materials_unlit".to_owned()
    }));
    let emissive = load_glb(&inputs::allowlisted_emissive()).expect("emissive");
    assert!(
        emissive
            .warnings
            .contains(&LoadWarning::SimplifiedExtension {
                name: "KHR_materials_emissive_strength".to_owned()
            })
    );
    assert_order(&unlit.warnings);
    assert_order(&emissive.warnings);
}

#[test]
fn optional_compression_uses_the_fallback_accessor() {
    let draco = load_glb(&inputs::optional_draco_with_fallback()).expect("draco");
    assert!(draco
        .warnings
        .iter()
        .any(|warning| matches!(warning, LoadWarning::UnsupportedOptionalExtension { name } if name == "KHR_draco_mesh_compression")));
    assert_eq!(draco.stats.placed_triangles, 1);
    let meshopt = load_glb(&inputs::optional_meshopt_with_fallback()).expect("meshopt");
    assert!(meshopt.warnings.iter().any(|warning| {
        matches!(warning, LoadWarning::UnsupportedOptionalExtension { name } if name == "EXT_meshopt_compression")
    }));
    match load_err(&inputs::optional_draco_without_position()) {
        LoadError::BadData { what } => assert_eq!(what, "POSITION"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn strips_and_fans_become_triangle_lists() {
    let strip = load_glb(&inputs::triangle_strip()).expect("strip");
    assert_eq!(strip.stats.placed_triangles, 2);
    assert_eq!(strip.primitives[0].indices, vec![0, 1, 2, 2, 1, 3]);
    let fan = load_glb(&inputs::triangle_fan()).expect("fan");
    assert_eq!(fan.stats.placed_triangles, 2);
    assert_eq!(fan.primitives[0].indices, vec![0, 1, 2, 0, 2, 3]);
}

#[test]
fn deep_chain_does_not_recurse_on_the_stack() {
    let model = load_glb(&inputs::deep_chain(10_000)).expect("deep");
    assert_eq!(model.stats.nodes, 10_000);
    assert_eq!(model.stats.placed_triangles, 1);
    assert_eq!(
        model.subtree(model_reviewer_core::scene::NodeId(0)).end,
        10_000
    );
}

#[test]
fn small_stress_file_shares_one_mesh() {
    let bytes = stress_glb(3, 2).expect("stress");
    let model = load_glb(&bytes).expect("load stress");
    assert_eq!(model.stats.nodes, 3);
    assert_eq!(model.stats.primitives, 1);
    assert_eq!(model.stats.unique_triangles, 2);
    assert_eq!(model.stats.placed_triangles, placed_triangles(3, 2));
}

#[test]
fn container_and_json_failures_are_not_panics() {
    assert!(matches!(
        load_err(&inputs::wrong_magic()),
        LoadError::NotGlb
    ));
    assert!(matches!(
        load_err(&inputs::truncated()),
        LoadError::Container(_)
    ));
    assert!(matches!(load_err(&inputs::bad_json()), LoadError::Json(_)));
    assert!(matches!(load_err(&[]), LoadError::NotGlb));
}
