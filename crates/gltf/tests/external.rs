/// 環境変数 `MR_EXTERNAL_GLB_DIR` が無いときは何もしない。
/// `--include-ignored` で、そのディレクトリの `.glb` を読んで結果だけ出す。
#[test]
#[ignore]
fn external_glb_directory() {
    let Ok(dir) = std::env::var("MR_EXTERNAL_GLB_DIR") else {
        return;
    };
    if dir.is_empty() {
        return;
    }
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) => {
            println!("err {dir} {error}");
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                println!("err entry {error}");
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("glb") {
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                println!("err {path:?} {error}");
                continue;
            }
        };
        match model_reviewer_gltf::load_glb(&bytes) {
            Ok(model) => {
                println!(
                    "ok {path:?} nodes={} meshes={} primitives={} unique_triangles={} placed_triangles={} warnings={:?}",
                    model.stats.nodes,
                    model.stats.meshes,
                    model.stats.primitives,
                    model.stats.unique_triangles,
                    model.stats.placed_triangles,
                    model
                        .warnings
                        .iter()
                        .map(|warning| warning.message_ja())
                        .collect::<Vec<_>>()
                );
            }
            Err(error) => println!("err {path:?} {}", error.message_ja()),
        }
    }
}
