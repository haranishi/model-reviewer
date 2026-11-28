use std::{fs, process::Command};

use super::*;

fn chunks(bytes: &[u8]) -> (&str, &[u8]) {
    let length = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(&bytes[..4], b"glTF");
    assert_eq!(length(4), 2);
    assert_eq!(length(8), bytes.len());
    assert_eq!(&bytes[16..20], b"JSON");
    let end = 20 + length(12);
    let json = std::str::from_utf8(&bytes[20..end]).unwrap();
    if end == bytes.len() {
        return (json, &[]);
    }
    assert_eq!(&bytes[end + 4..end + 8], b"BIN\0");
    let bin = &bytes[end + 8..];
    assert_eq!(length(end), bin.len());
    (json, bin)
}

#[test]
fn textured_quad_opens() {
    let dir = std::env::temp_dir().join(format!(
        "model-reviewer-textured-quad-{}",
        std::process::id()
    ));
    fs::create_dir(&dir).unwrap();
    let cases = [
        TexturedOptions::default(),
        TexturedOptions {
            uv_format: UvFormat::U8,
            tex_coord: 1,
            sampler: Some(SamplerOptions {
                wrap_s: 33071,
                wrap_t: 33648,
                mag_filter: 9728,
                min_filter: 9987,
            }),
            mask_cutoff: Some(0.375),
            ..TexturedOptions::default()
        },
        TexturedOptions {
            image: QuadImage::Jpeg,
            uv_format: UvFormat::U16,
            transform: Some(TextureTransform {
                offset: [0.5, 0.0],
                rotation: std::f32::consts::FRAC_PI_2,
                scale: [2.0, 1.0],
                tex_coord: Some(1),
            }),
            ..TexturedOptions::default()
        },
    ];
    for (i, options) in cases.iter().enumerate() {
        let bytes = textured_quad_glb(options);
        let (json, _) = chunks(&bytes);
        assert_eq!(json.matches(r#""pbrMetallicRoughness""#).count(), 1);
        assert!(json.contains(r#""material":0"#));
        fs::write(dir.join(format!("quad-{i}.glb")), bytes).unwrap();
    }

    // 読み込み側の開発依存がこのクレートを使う。逆向きの依存を足さず、既存の読込試験に渡す。
    let result = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "test",
            "--locked",
            "-p",
            "model-reviewer-gltf",
            "--test",
            "external",
            "--",
            "--ignored",
            "--exact",
            "external_glb_directory",
            "--nocapture",
        ])
        .env("MR_EXTERNAL_GLB_DIR", &dir)
        .output()
        .unwrap();
    fs::remove_dir_all(&dir).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    for i in 0..cases.len() {
        let expected = format!(
            r#"quad-{i}.glb" nodes=1 meshes=1 primitives=1 unique_triangles=2 placed_triangles=2"#
        );
        assert!(output.contains(&expected), "{output}");
    }
}

#[test]
fn textured_scene_has_requested_counts() {
    for (images, parts) in [(0, 5), (1, 1), (3, 7), (5, 2), (0, 0)] {
        let bytes = textured_scene_glb(images, 7, parts);
        assert_eq!(bytes, textured_scene_glb(images, 7, parts));
        let (json, bin) = chunks(&bytes);
        assert_eq!(json.matches(r#""mesh""#).count(), parts as usize);
        assert_eq!(json.matches(r#""primitives""#).count(), parts as usize);
        for key in [
            r#""mimeType""#,
            r#""source""#,
            r#""pbrMetallicRoughness""#,
            r#""baseColorTexture""#,
        ] {
            assert_eq!(json.matches(key).count(), images as usize, "{key}");
        }
        let mut distinct = std::collections::HashSet::new();
        for i in 0..images {
            let png = pattern_png(Pattern::Stripes, 7, i);
            assert!(bin.windows(png.len()).any(|window| window == png));
            assert!(distinct.insert(png));
        }
    }
}
