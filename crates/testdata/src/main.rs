use std::env;
use std::path::{Component, Path};
use std::process::ExitCode;

use model_reviewer_testdata::{
    MESH_COUNT, NODE_COUNT, PLACED_TRIANGLES, PRIMITIVE_COUNT, Pattern, STL_SAMPLE_TRIANGLES,
    STL_SAMPLE_VERTICES, UNIQUE_TRIANGLES, nested_descendants, pattern_png, placed_triangles,
    sample_glb, stl_sample, stl_stress, stress_glb, stress_nested_glb, textured_scene_glb,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(Exit::Usage(message)) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
        Err(Exit::Write(message)) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

enum Exit {
    Usage(&'static str),
    Write(String),
}

fn run() -> Result<(), Exit> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "sample".to_owned());
    match command.as_str() {
        "sample" => {
            let path = args
                .next()
                .unwrap_or_else(|| "assets/samples/review-sample.glb".to_owned());
            if args.next().is_some() {
                return Err(Exit::Usage(usage()));
            }
            let bytes = sample_glb();
            write_file(&path, &bytes)?;
            println!("nodes {NODE_COUNT}");
            println!("meshes {MESH_COUNT}");
            println!("primitives {PRIMITIVE_COUNT}");
            println!("unique_triangles {UNIQUE_TRIANGLES}");
            println!("placed_triangles {PLACED_TRIANGLES}");
            println!("bytes {}", bytes.len());
            Ok(())
        }
        "stress" | "stress-nested" => {
            let nested = command == "stress-nested";
            let parts = args
                .next()
                .ok_or(Exit::Usage(usage()))?
                .parse::<u32>()
                .map_err(|_| Exit::Usage(usage()))?;
            let triangles = args
                .next()
                .ok_or(Exit::Usage(usage()))?
                .parse::<u32>()
                .map_err(|_| Exit::Usage(usage()))?;
            let default_path = if nested {
                "target/stress-nested.glb"
            } else {
                "target/stress.glb"
            };
            let path = args.next().unwrap_or_else(|| default_path.to_owned());
            if args.next().is_some() || !is_under_target(&path) {
                return Err(Exit::Usage(
                    "stress の出力は target/ 以下の相対パスだけです",
                ));
            }
            let bytes = if nested {
                stress_nested_glb(parts, triangles)
            } else {
                stress_glb(parts, triangles)
            }
            .map_err(Exit::Usage)?;
            write_file(&path, &bytes)?;
            println!("nodes {parts}");
            if nested {
                println!("group_descendants {}", nested_descendants(parts));
            }
            println!("unique_triangles {triangles}");
            println!("placed_triangles {}", placed_triangles(parts, triangles));
            println!("bytes {}", bytes.len());
            Ok(())
        }
        "pattern" => {
            let path = args.next().ok_or(Exit::Usage(usage()))?;
            if args.next().is_some() || !is_under_target(&path) {
                return Err(Exit::Usage(
                    "pattern の出力は target/ 以下の相対パスだけです",
                ));
            }
            write_file(&path, &pattern_png(Pattern::Quadrants, 128, 0))
        }
        "textured" => {
            let mut number = || {
                args.next()
                    .and_then(|arg| arg.parse::<u32>().ok())
                    .ok_or(Exit::Usage(usage()))
            };
            let images = number()?;
            let size = number()?;
            let parts = number()?;
            let path = args.next().ok_or(Exit::Usage(usage()))?;
            if args.next().is_some() || !is_under_target(&path) {
                return Err(Exit::Usage(
                    "textured の出力は target/ 以下の相対パスだけです",
                ));
            }
            if images > 0 && size == 0 {
                return Err(Exit::Usage("画像の辺は1画素以上です"));
            }
            if images > 0 && u64::from(size).pow(2) > isize::MAX as u64 / 4 {
                return Err(Exit::Usage(
                    "RGBA8 の画像がメモリに収まる大きさを超えています",
                ));
            }
            let bytes = textured_scene_glb(images, size, parts);
            write_file(&path, &bytes)?;
            println!("images {images}");
            println!("nodes {parts}");
            println!("bytes {}", bytes.len());
            Ok(())
        }
        "stl-sample" => {
            let path = args
                .next()
                .unwrap_or_else(|| "assets/samples/review-sample.stl".to_owned());
            if args.next().is_some() {
                return Err(Exit::Usage(usage()));
            }
            let bytes = stl_sample();
            write_file(&path, &bytes)?;
            println!("triangles {STL_SAMPLE_TRIANGLES}");
            println!("vertices {STL_SAMPLE_VERTICES}");
            println!("bytes {}", bytes.len());
            Ok(())
        }
        "stl-stress" => {
            let triangles = args
                .next()
                .ok_or(Exit::Usage(usage()))?
                .parse::<u32>()
                .map_err(|_| Exit::Usage(usage()))?;
            if triangles == 0 || triangles > STL_STRESS_MAX_TRIANGLES {
                return Err(Exit::Usage(
                    "stl-stress の三角形数は 1 から 20000000 までです",
                ));
            }
            let ascii = match args.next().as_deref() {
                Some("binary") => false,
                Some("ascii") => true,
                _ => return Err(Exit::Usage(usage())),
            };
            let path = args
                .next()
                .unwrap_or_else(|| "target/stress.stl".to_owned());
            if args.next().is_some() || !is_under_target(&path) {
                return Err(Exit::Usage(
                    "stl-stress の出力は target/ 以下の相対パスだけです",
                ));
            }
            let bytes = stl_stress(triangles, ascii);
            write_file(&path, &bytes)?;
            println!("triangles {triangles}");
            println!("bytes {}", bytes.len());
            Ok(())
        }
        _ => Err(Exit::Usage(usage())),
    }
}

/// 重い STL の三角形の上限（バイナリで約1GB）。
const STL_STRESS_MAX_TRIANGLES: u32 = 20_000_000;

fn usage() -> &'static str {
    "使い方: model-reviewer-testdata sample [出力パス]\n       model-reviewer-testdata stress <部品数> <三角形数> [target/ 以下のパス]\n       model-reviewer-testdata stress-nested <部品数> <三角形数> [target/ 以下のパス]\n         （根の下に、部品数の半分を子孫に持つまとめ役を置く。部品数は 4 以上）\n       model-reviewer-testdata textured <画像の枚数> <辺の画素数> <部品数> <target/ 以下の出力.glb>\n       model-reviewer-testdata pattern <target/ 以下の出力.png>（128×128画素の4色の見本）\n       model-reviewer-testdata stl-sample [出力パス]\n       model-reviewer-testdata stl-stress <三角形数> <binary|ascii> [target/ 以下のパス]"
}

fn is_under_target(path: &str) -> bool {
    let path = Path::new(path);
    if path.is_absolute() {
        return false;
    }
    let mut saw_target = false;
    for component in path.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
            Component::CurDir => {}
            Component::Normal(name) if !saw_target && name == "target" => saw_target = true,
            Component::Normal(_) if saw_target => {}
            Component::Normal(_) => return false,
        }
    }
    saw_target
}

fn write_file(path: &str, bytes: &[u8]) -> Result<(), Exit> {
    let parent = Path::new(path).parent();
    if let Some(parent) = parent
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| Exit::Write(format!("ディレクトリを作れません: {error}")))?;
    }
    std::fs::write(path, bytes).map_err(|error| Exit::Write(format!("書き込めません: {error}")))
}
