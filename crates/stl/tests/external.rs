//! 外部のソフトで作った STL での確認。環境変数 `MR_EXTERNAL_STL_DIR` が無いときは何もしない。
//! `--ignored` で、そのフォルダの `.stl` をすべて読み、落ちないこと、三角形が1つ以上で境界箱が
//! 有限であること、バイナリなら三角形の数がファイルの申告と同じことを確かめ、結果を出す。

use model_reviewer_core::units::UnitLabel;
use model_reviewer_stl::{StlKind, StlOptions, detect, load_stl};

#[test]
#[ignore]
fn external_stl_directory() {
    let Ok(dir) = std::env::var("MR_EXTERNAL_STL_DIR") else {
        return;
    };
    if dir.is_empty() {
        return;
    }
    let entries = std::fs::read_dir(&dir).expect("フォルダを読めません");
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("stl"))
        })
        .collect();
    paths.sort();
    for path in paths {
        let bytes = std::fs::read(&path).expect("ファイルを読めません");
        let options = StlOptions {
            name: "外部",
            unit: UnitLabel::Mm,
            up: Default::default(),
            allow_trailing: true,
        };
        let kind = detect(&bytes, true);
        let model = load_stl(&bytes, &options)
            .unwrap_or_else(|error| panic!("{path:?}: {}", error.message_ja()));
        let triangles = model.stats.unique_triangles;
        assert!(triangles >= 1, "{path:?}");
        assert!(
            model.bounds.min.is_finite() && model.bounds.max.is_finite(),
            "{path:?}"
        );
        if kind == Some(StlKind::Binary) {
            let claimed = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]);
            assert_eq!(triangles, u64::from(claimed), "{path:?}");
        }
        let size = model.primitives[0].local_aabb.max - model.primitives[0].local_aabb.min;
        println!(
            "ok {path:?} kind={kind:?} triangles={triangles} vertices={} size={size:?} warnings={:?}",
            model.primitives[0].vertices.len(),
            model
                .warnings
                .iter()
                .map(|warning| warning.message_ja())
                .collect::<Vec<_>>()
        );
    }
}
