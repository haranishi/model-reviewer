use std::collections::BTreeSet;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use model_reviewer::keymap::{Action, Keymap, Platform};
use model_reviewer::ui::keys::{UNASSIGNED, fixed_keys};
use serde_json::Value;

// 各ページで読者に見せる画像。撮影手順の出力名もここに合わせる。
const PAGES: &[(&str, &[&str])] = &[
    ("README.md", &["overview.png"]),
    ("01-start.md", &["start-empty.png"]),
    ("02-open-view.md", &["view-sample.png", "view-xray.png"]),
    ("03-measure.md", &["measure-snap.png", "measure-panel.png"]),
    ("04-section.md", &["section-on.png"]),
    ("05-stl.md", &["stl-open.png"]),
    ("06-shapes.md", &["shapes-dims.png"]),
    ("07-review.md", &["review-notes.png"]),
    ("08-export.md", &["export-result.png"]),
    ("09-shortcuts.md", &["keys-window.png", "keys-conflict.png"]),
    ("10-troubleshooting.md", &[]),
    (
        "11-create.md",
        &["create-new.png", "create-keymove.png", "create-color.png"],
    ),
    ("12-texture.md", &["texture-view.png", "texture-paste.png"]),
];

fn app_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn guide_dir() -> PathBuf {
    app_dir().join("..").join("..").join("docs").join("guide")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("{}: {err}", path.display()))
        .replace("\r\n", "\n")
}

fn filenames(dir: &Path, extension: &str) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == extension))
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
        .collect()
}

fn assert_shortcut_table(marker: &str, expected: &str) {
    let page = read(&guide_dir().join("09-shortcuts.md"));
    let start = format!("<!-- {marker}:begin -->");
    let end = format!("<!-- {marker}:end -->");
    let actual = page
        .split_once(&start)
        .and_then(|(_, rest)| rest.split_once(&end))
        .map(|(table, _)| table.trim_matches('\n'));
    assert_eq!(
        actual,
        Some(expected.trim_end()),
        "09-shortcuts.md の {marker} の表を次の内容に合わせてください。\n{expected}"
    );
}

#[test]
fn guide_default_table_matches_registry() {
    let keymap = Keymap::default();
    let mac = keymap.labels(Platform::Mac);
    let other = keymap.labels(Platform::Other);
    let mut table =
        String::from("| まとまり | 画面の言葉 | macOS | Windows・Linux |\n|---|---|---|---|\n");
    for &action in Action::ALL {
        writeln!(
            table,
            "| {} | {} | {} | {} |",
            action.group().label(),
            action.label(),
            mac.get(action).unwrap_or(UNASSIGNED),
            other.get(action).unwrap_or(UNASSIGNED)
        )
        .unwrap();
    }
    assert_shortcut_table("keys", &table);
}

#[test]
fn guide_fixed_table_matches_registry() {
    let mut table = String::from("| キー | 役目 |\n|---|---|\n");
    for (key, role) in fixed_keys() {
        writeln!(table, "| {key} | {role} |").unwrap();
    }
    assert_shortcut_table("fixed", &table);
}

// 本文のインラインリンクを1行ずつ読む。同じ行の別のリンクも検査する。
fn references(markdown: &str) -> impl Iterator<Item = (bool, &str)> {
    markdown.lines().flat_map(|line| {
        line.match_indices("](").map(move |(end, _)| {
            let (prefix, label) = line[..end].rsplit_once('[').expect(line);
            let (target, _) = line[end + 2..].split_once(')').expect(line);
            assert!(!target.is_empty() && !label.trim().is_empty(), "{line}");
            (prefix.ends_with('!'), target)
        })
    })
}

fn check_relative_files(images: bool) {
    let dir = guide_dir();
    for &(page, _) in PAGES {
        for (image, target) in references(&read(&dir.join(page))) {
            if image != images || target.starts_with("https://") || target.starts_with("http://") {
                continue;
            }
            let path = target.split('#').next().unwrap();
            if !path.is_empty() {
                assert!(
                    Path::new(path).is_relative() && dir.join(path).is_file(),
                    "{page} の参照先を確認してください: {target}"
                );
            }
        }
    }
}

#[test]
fn guide_links_resolve() {
    check_relative_files(false);
    let root = app_dir().join("..").join("..");
    for file in ["README.md", "README.ja.md"] {
        let page = read(&root.join(file));
        for target in ["docs/guide/README.md", "docs/guide/09-shortcuts.md"] {
            assert!(
                references(&page).any(|r| r == (false, target)),
                "{file} → {target}"
            );
        }
    }
}

#[test]
fn guide_image_files_exist() {
    check_relative_files(true);
}

#[test]
fn guide_has_all_pages_within_limit() {
    let dir = guide_dir();
    let expected = PAGES.iter().map(|&(name, _)| name.to_owned()).collect();
    assert_eq!(filenames(&dir, "md"), expected);
    let index = read(&dir.join("README.md"));
    for &(name, _) in PAGES {
        let count = read(&dir.join(name)).lines().count();
        assert!((1..=100).contains(&count), "{name} は {count} 行です");
        if name != "README.md" {
            assert!(
                references(&index).any(|r| r == (false, name)),
                "入口 → {name}"
            );
        }
    }
}

// verify.md の一覧は読点で区切られ、各項目の最初のコードが op の名前。
fn documented_ops(verify: &str) -> BTreeSet<&str> {
    let (_, list) = verify
        .split_once("手順の `op` は ")
        .expect("verify.md の op 一覧");
    let (list, _) = list.split_once(" です。").unwrap();
    list.split('、')
        .map(|item| item.split('`').nth(1).expect(item))
        .collect()
}

fn screenshot_outputs(script: &Path, known_ops: &BTreeSet<&str>) -> Vec<String> {
    let json: Value = serde_json::from_str(&read(script))
        .unwrap_or_else(|err| panic!("{}: {err}", script.display()));
    let steps = json["steps"].as_array().expect("steps は配列");
    let mut shots = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let op = step["op"].as_str().unwrap_or("");
        assert!(
            known_ops.contains(op),
            "{} の steps[{index}]: {op:?}",
            script.display()
        );
        if op == "screenshot" {
            assert!(step.get("relative_to").is_none_or(|base| base == "out"));
            shots.push(step["path"].as_str().expect("画像の出力名").to_owned());
        }
    }
    shots
}

#[test]
fn guide_scripts_use_known_ops() {
    let dir = guide_dir();
    let verify = read(&dir.join("..").join("verify.md"));
    let known_ops = documented_ops(&verify);
    let e2e = app_dir().join("e2e");
    let mut scripts = BTreeSet::new();
    for &(page, images) in PAGES {
        let markdown = read(&dir.join(page));
        let linked: Vec<_> = references(&markdown)
            .filter(|(image, _)| *image)
            .map(|(_, target)| {
                target
                    .strip_prefix("images/")
                    .unwrap_or_else(|| panic!("{page}: {target}"))
            })
            .collect();
        assert_eq!(linked, images, "{page} の画像");
        if images.is_empty() {
            continue;
        }
        let stem = if page == "README.md" {
            "00-overview"
        } else {
            page.trim_end_matches(".md")
        };
        let name = format!("guide-{stem}.json");
        assert_eq!(
            screenshot_outputs(&e2e.join(&name), &known_ops),
            images,
            "{name}"
        );
        scripts.insert(name);
    }
    let found = filenames(&e2e, "json")
        .into_iter()
        .filter(|name| name.starts_with("guide-"))
        .collect();
    assert_eq!(scripts, found);
}
