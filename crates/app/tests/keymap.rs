//! 設定ファイルの読み書き（WP-S1）。ファイルは `std::env::temp_dir()` の下の、試験ごとの
//! フォルダだけに作る。利用者の本物の設定の場所は組み立てない。

use std::path::{Path, PathBuf};

use egui::Key;
use model_reviewer::keymap::store::{MAX_BYTES, Store, StoreSource, StoreState};
use model_reviewer::keymap::{Action, Chord, Keymap};
use serde_json::{Value, json};

/// 試験ごとの空のフォルダ。前の実行の残りは消す。
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "model-reviewer-keymap-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn chord(key: Key, shift: bool, command: bool) -> Chord {
    Chord {
        key,
        shift,
        command,
    }
}

/// フォルダの中の名前（並べたもの）。
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn same_bindings(a: &Keymap, b: &Keymap) -> bool {
    Action::ALL
        .iter()
        .all(|&action| a.chord(action) == b.chord(action))
}

#[test]
fn round_trips_changes_only() {
    let dir = scratch("round-trip");
    let path = dir.join("nested").join("keymap.json");
    let opened = Store::open(path.clone());
    assert_eq!(opened.store.state(), &StoreState::Ready);
    assert_eq!(opened.store.source(), StoreSource::Default);
    assert_eq!(opened.notice, None);
    let mut store = opened.store;
    let mut keymap = opened.keymap;

    // 既定のままなら bindings は空。保存先のフォルダが無ければ作る。
    store.save(&keymap).unwrap();
    let file = read_json(&path);
    assert_eq!(file["format"], "model-reviewer-keymap");
    assert_eq!(file["version"], 1);
    assert_eq!(file["bindings"], json!({}));

    keymap
        .assign(Action::ToolMove, chord(Key::T, false, false), false)
        .unwrap();
    keymap
        .assign(Action::EditRedo, chord(Key::Y, true, true), false)
        .unwrap();
    keymap.clear(Action::ToolRotate);
    keymap
        .assign(Action::ViewBack, chord(Key::F5, false, false), false)
        .unwrap();
    store.save(&keymap).unwrap();
    assert_eq!(
        read_json(&path)["bindings"],
        json!({
            "edit.redo": "Shift+Command+Y",
            "tool.move": "T",
            "tool.rotate": null,
            "view.back": "F5",
        })
    );

    let reopened = Store::open(path.clone());
    assert_eq!(reopened.store.state(), &StoreState::Ready);
    assert_eq!(reopened.store.source(), StoreSource::File);
    assert_eq!(reopened.notice, None);
    assert!(same_bindings(&reopened.keymap, &keymap));
    for &action in Action::ALL {
        assert_eq!(
            reopened.keymap.is_changed(action),
            keymap.is_changed(action),
            "{}",
            action.id()
        );
    }
    // 一時ファイルは残らない。
    assert_eq!(names(&dir.join("nested")), vec!["keymap.json"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn same_keymap_same_bytes() {
    let dir = scratch("same-bytes");
    let a = dir.join("a.json");
    let b = dir.join("b.json");

    let mut first = Keymap::default();
    first
        .assign(Action::ToolMove, chord(Key::T, false, false), false)
        .unwrap();
    first.clear(Action::ViewHome);
    first
        .assign(Action::AppKeys, chord(Key::K, true, true), false)
        .unwrap();

    // 違う順で同じ割り当てにする（途中で別の値を経る）。
    let mut second = Keymap::default();
    second
        .assign(Action::AppKeys, chord(Key::J, false, false), false)
        .unwrap();
    second.clear(Action::ViewHome);
    second
        .assign(Action::AppKeys, chord(Key::K, true, true), false)
        .unwrap();
    second
        .assign(Action::ToolMove, chord(Key::T, false, false), false)
        .unwrap();
    assert_ne!(first.rev(), second.rev());

    Store::open(a.clone()).store.save(&first).unwrap();
    Store::open(b.clone()).store.save(&second).unwrap();
    let bytes = std::fs::read(&a).unwrap();
    assert_eq!(bytes, std::fs::read(&b).unwrap());
    // 改行は \n だけで、末尾に1つ。
    assert!(!bytes.contains(&b'\r'));
    assert!(bytes.ends_with(b"}\n"));
    // 書き直しても同じ。
    Store::open(a.clone()).store.save(&first).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_entries_survive() {
    let dir = scratch("unknown");
    let path = dir.join("keymap.json");
    let original = json!({
        "format": "model-reviewer-keymap",
        "version": 1,
        "bindings": {
            "future.action": "Shift+Command+Q",
            "future.other": { "keys": ["T", "U"] },
            "tool.move": "Shift+G",
        },
        "later": { "theme": "dark" },
        "notes": [1, 2, 3],
    });
    std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();

    let opened = Store::open(path.clone());
    assert_eq!(opened.notice, None);
    assert_eq!(
        opened.keymap.chord(Action::ToolMove),
        Some(chord(Key::G, true, false))
    );
    assert_eq!(opened.keymap.unknown().len(), 2);
    assert_eq!(opened.store.extra().len(), 2);

    let mut store = opened.store;
    let mut keymap = opened.keymap;
    keymap
        .assign(Action::ToolRotate, chord(Key::T, false, false), false)
        .unwrap();
    store.save(&keymap).unwrap();
    let saved = read_json(&path);
    assert_eq!(saved["later"], original["later"]);
    assert_eq!(saved["notes"], original["notes"]);
    assert_eq!(
        saved["bindings"],
        json!({
            "future.action": "Shift+Command+Q",
            "future.other": { "keys": ["T", "U"] },
            "tool.move": "Shift+G",
            "tool.rotate": "T",
        })
    );
    // すべて既定に戻しても、知らない項目は残る。
    keymap.reset_all();
    store.save(&keymap).unwrap();
    let saved = read_json(&path);
    assert_eq!(
        saved["bindings"],
        json!({
            "future.action": "Shift+Command+Q",
            "future.other": { "keys": ["T", "U"] },
        })
    );
    assert_eq!(saved["later"], original["later"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn broken_file_is_kept_as_broken_on_first_save() {
    let dir = scratch("broken");
    let path = dir.join("keymap.json");
    let broken_path = dir.join("keymap.json.broken");
    let samples: Vec<Vec<u8>> = vec![
        b"{ not json".to_vec(),
        b"[1, 2]".to_vec(),
        br#"{"format":"other","version":1,"bindings":{}}"#.to_vec(),
        br#"{"version":1,"bindings":{}}"#.to_vec(),
        br#"{"format":"model-reviewer-keymap","version":"1","bindings":{}}"#.to_vec(),
        br#"{"format":"model-reviewer-keymap","version":0,"bindings":{}}"#.to_vec(),
        br#"{"format":"model-reviewer-keymap","version":1.5,"bindings":{}}"#.to_vec(),
        br#"{"format":"model-reviewer-keymap","version":-1,"bindings":{}}"#.to_vec(),
        br#"{"format":"model-reviewer-keymap","version":1,"bindings":[]}"#.to_vec(),
        br#"{"format":"model-reviewer-keymap","version":1}"#.to_vec(),
        vec![0xff, 0xfe, 0x00],
        Vec::new(),
        // 上限を1バイト超える、正しい中身。
        {
            let mut text =
                br#"{"format":"model-reviewer-keymap","version":1,"bindings":{"tool.move":"T"}}"#
                    .to_vec();
            text.resize(MAX_BYTES as usize + 1, b' ');
            text
        },
    ];
    for (i, sample) in samples.iter().enumerate() {
        std::fs::write(&path, sample).unwrap();
        // 前の回の .broken が残っていれば置き換える。
        let opened = Store::open(path.clone());
        if sample.len() as u64 > MAX_BYTES || serde_json::from_slice::<Value>(sample).is_err() {
            let previous_backup = std::fs::read(&broken_path).ok();
            assert_eq!(opened.store.source(), StoreSource::Default);
            assert_eq!(opened.keymap, Keymap::default());
            assert!(opened.notice.unwrap().contains("保存"));
            let mut store = opened.store;
            assert!(!store.saves());
            store.save(&opened.keymap).unwrap();
            assert_eq!(&std::fs::read(&path).unwrap(), sample, "見本 {i}");
            assert_eq!(std::fs::read(&broken_path).ok(), previous_backup);
            continue;
        }
        assert_eq!(opened.store.state(), &StoreState::Broken, "見本 {i}");
        assert_eq!(opened.store.source(), StoreSource::Default);
        assert_eq!(opened.keymap, Keymap::default());
        assert_eq!(
            opened.notice.as_deref(),
            Some("設定を読めなかったため、初期値で起動しました")
        );
        assert!(opened.store.saves());
        // 開いただけでは触らない。
        assert_eq!(&std::fs::read(&path).unwrap(), sample);

        let mut store = opened.store;
        let mut keymap = opened.keymap;
        keymap
            .assign(Action::ToolMove, chord(Key::T, false, false), false)
            .unwrap();
        store.save(&keymap).unwrap();
        assert_eq!(store.state(), &StoreState::Ready);
        assert_eq!(&std::fs::read(&broken_path).unwrap(), sample, "見本 {i}");
        assert_eq!(read_json(&path)["bindings"], json!({ "tool.move": "T" }));

        // 2回目の保存では .broken を作り直さない。
        keymap.clear(Action::ToolMove);
        store.save(&keymap).unwrap();
        assert_eq!(&std::fs::read(&broken_path).unwrap(), sample);
        assert_eq!(names(&dir), vec!["keymap.json", "keymap.json.broken"]);
    }

    // ちょうど上限の大きさなら読める。
    let mut text =
        br#"{"format":"model-reviewer-keymap","version":1,"bindings":{"tool.move":"T"}}"#.to_vec();
    text.resize(MAX_BYTES as usize, b' ');
    std::fs::write(&path, &text).unwrap();
    let opened = Store::open(path.clone());
    assert_eq!(opened.store.state(), &StoreState::Ready);
    assert_eq!(
        opened.keymap.chord(Action::ToolMove),
        Some(chord(Key::T, false, false))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn data_files_never_become_keymaps() {
    let dir = scratch("data");
    let path = dir.join("model");
    let large = vec![b' '; MAX_BYTES as usize + 1];
    for bytes in [
        &b"glTF content"[..],
        &[0xff, 0],
        b"solid example",
        b"{",
        &large,
        br#"{"format":"model-reviewer.review","version":1}"#,
    ] {
        std::fs::write(&path, bytes).unwrap();
        let mut opened = Store::open(path.clone());
        let _ = opened.store.save(&opened.keymap);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(names(&dir), ["model"]);
        assert!(!opened.store.saves());
        assert!(opened.notice.unwrap().contains("保存"));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn protect_data_that_appears_at_the_destination_or_backup() {
    let dir = scratch("late-data");
    let path = dir.join("keymap.json");
    let backup = dir.join("keymap.json.broken");
    for candidate in [&path, &backup] {
        std::fs::write(&path, b"{}").unwrap();
        let mut opened = Store::open(path.clone());
        std::fs::write(candidate, b"glTF").unwrap();
        let reason = opened.store.save(&opened.keymap).unwrap_err();
        assert!(!reason.is_empty());
        assert_eq!(std::fs::read(candidate).unwrap(), b"glTF");
        if candidate == &backup {
            assert_eq!(std::fs::read(&path).unwrap(), b"{}");
        } else {
            assert!(!backup.exists());
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn save_rechecks_the_destination_version() {
    let dir = scratch("version-changed");
    let path = dir.join("keymap.json");
    let newer = br#"{"format":"model-reviewer-keymap","version":2,"bindings":{}}"#;
    for old in [
        None,
        Some(&b"{}"[..]),
        Some(&br#"{"format":"model-reviewer-keymap","version":1,"bindings":{}}"#[..]),
    ] {
        if let Some(old) = old {
            std::fs::write(&path, old).unwrap();
        }
        let mut opened = Store::open(path.clone());
        std::fs::write(&path, newer).unwrap();
        let reason = opened.store.save(&opened.keymap).unwrap_err();
        assert!(reason.contains("新しい版"));
        assert!(!opened.store.saves());
        assert_eq!(std::fs::read(&path).unwrap(), newer);
        assert_eq!(names(&dir), ["keymap.json"]);
        std::fs::remove_file(&path).unwrap();
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn large_unknown_values_stay_readable_after_save() {
    let dir = scratch("large-unknown");
    let path = dir.join("keymap.json");
    let mut json =
        json!({"format":"model-reviewer-keymap","version":1,"bindings":{},"future":vec![0;12000]});
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    let mut opened = Store::open(path.clone());
    opened.keymap.clear(Action::ToolMove);
    opened.store.save(&opened.keymap).unwrap();
    let reopened = Store::open(path.clone());
    assert_eq!(reopened.store.source(), StoreSource::File);
    assert_eq!(reopened.keymap.chord(Action::ToolMove), None);
    assert_eq!(read_json(&path)["future"], json["future"]);

    json["future"] = "".into();
    let room = MAX_BYTES as usize - serde_json::to_vec(&json).unwrap().len();
    json["future"] = "x".repeat(room).into();
    let original = serde_json::to_vec(&json).unwrap();
    assert_eq!(original.len() as u64, MAX_BYTES);
    std::fs::write(&path, &original).unwrap();
    let mut opened = Store::open(path.clone());
    opened.keymap.clear(Action::ToolMove);
    let reason = opened.store.save(&opened.keymap).unwrap_err();
    assert!(reason.contains("64 KiB"));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(names(&dir), ["keymap.json"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn newer_version_is_never_overwritten() {
    let dir = scratch("newer");
    let path = dir.join("keymap.json");
    let text =
        br#"{"format":"model-reviewer-keymap","version":2,"bindings":{"tool.move":"T"},"x":1}"#;
    std::fs::write(&path, text).unwrap();

    let opened = Store::open(path.clone());
    assert_eq!(opened.store.state(), &StoreState::Newer);
    assert_eq!(opened.store.source(), StoreSource::Default);
    assert_eq!(opened.keymap, Keymap::default());
    assert_eq!(
        opened.notice.as_deref(),
        Some("新しい版の設定です。このアプリでは変更を保存しません")
    );
    assert!(!opened.store.saves());

    let mut store = opened.store;
    let mut keymap = opened.keymap;
    keymap
        .assign(Action::ToolRotate, chord(Key::T, false, false), false)
        .unwrap();
    assert_eq!(store.save(&keymap), Ok(()));
    assert_eq!(std::fs::read(&path).unwrap(), text);
    assert_eq!(names(&dir), vec!["keymap.json"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unreadable_file_is_not_overwritten() {
    let dir = scratch("unreadable");
    // 通常のファイルでない（フォルダ）。
    let path = dir.join("keymap.json");
    std::fs::create_dir_all(path.join("inside")).unwrap();
    let opened = Store::open(path.clone());
    assert!(
        matches!(opened.store.state(), StoreState::Unreadable(_)),
        "{:?}",
        opened.store.state()
    );
    assert_eq!(opened.keymap, Keymap::default());
    let notice = opened.notice.clone().unwrap();
    assert!(
        notice.starts_with("設定ファイルを読めませんでした"),
        "{notice}"
    );
    assert!(!opened.store.saves());
    let mut store = opened.store;
    let mut keymap = opened.keymap;
    keymap.clear(Action::ToolMove);
    assert_eq!(store.save(&keymap), Ok(()));
    assert!(path.join("inside").is_dir());
    assert_eq!(names(&dir), vec!["keymap.json"]);

    // 権限で読めないファイル（Unix だけ。権限を無視できる利用者では確かめられないので飛ばす）。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let locked = dir.join("locked.json");
        std::fs::write(&locked, b"{}").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&locked).is_err() {
            let opened = Store::open(locked.clone());
            assert!(matches!(opened.store.state(), StoreState::Unreadable(_)));
            let mut store = opened.store;
            assert_eq!(store.save(&Keymap::default()), Ok(()));
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(std::fs::read(&locked).unwrap(), b"{}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn user_binding_beats_default_of_another_action() {
    let dir = scratch("user-beats-default");
    let path = dir.join("keymap.json");
    std::fs::write(
        &path,
        br#"{"format":"model-reviewer-keymap","version":1,"bindings":{"tool.rotate":"G"}}"#,
    )
    .unwrap();
    let opened = Store::open(path.clone());
    assert_eq!(opened.notice, None);
    assert_eq!(
        opened.keymap.chord(Action::ToolRotate),
        Some(chord(Key::G, false, false))
    );
    assert_eq!(opened.keymap.chord(Action::ToolMove), None);
    assert_eq!(
        opened.keymap.action_for(chord(Key::G, false, false)),
        Some(Action::ToolRotate)
    );
    // ほかの操作は既定のまま。
    assert_eq!(
        opened.keymap.chord(Action::ToolScale),
        Action::ToolScale.default_chord()
    );
    // 書き戻すと、既定を外された側も書かれる。
    let mut store = opened.store;
    store.save(&opened.keymap).unwrap();
    assert_eq!(
        read_json(&path)["bindings"],
        json!({ "tool.move": null, "tool.rotate": "G" })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn duplicate_user_bindings_keep_the_first() {
    let dir = scratch("duplicate");
    let path = dir.join("keymap.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "format": "model-reviewer-keymap",
            "version": 1,
            "bindings": {
                // 名前の順で tool.move が先。tool.rotate の T は捨てる。
                "tool.rotate": "T",
                "tool.move": "T",
                // 書き方が違う・割り当てられない・型が違う。
                "tool.scale": "Ctrl+S",
                "view.top": "Command+C",
                "view.front": "Shift+1",
                "view.right": 3,
                "tool.measure": "",
                // 未割り当ては使える。
                "tool.select": null,
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let opened = Store::open(path.clone());
    assert_eq!(
        opened.notice.as_deref(),
        Some("割り当て 6 件を読めませんでした")
    );
    let keymap = &opened.keymap;
    assert_eq!(
        keymap.chord(Action::ToolMove),
        Some(chord(Key::T, false, false))
    );
    // 捨てた項目の操作は既定。
    for action in [
        Action::ToolRotate,
        Action::ToolScale,
        Action::ViewTop,
        Action::ViewFront,
        Action::ViewRight,
        Action::ToolMeasure,
    ] {
        assert_eq!(
            keymap.chord(action),
            action.default_chord(),
            "{}",
            action.id()
        );
    }
    assert_eq!(keymap.chord(Action::ToolSelect), None);
    // 2つの操作が同じ組を持たない。
    let chords: Vec<Chord> = Action::ALL
        .iter()
        .filter_map(|&a| keymap.chord(a))
        .collect();
    for (i, a) in chords.iter().enumerate() {
        assert!(!chords[i + 1..].contains(a));
    }
    // 捨てた項目は書き戻さない。
    let mut store = opened.store;
    store.save(keymap).unwrap();
    assert_eq!(
        read_json(&path)["bindings"],
        json!({ "tool.move": "T", "tool.select": null })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 決まった種の擬似乱数（xorshift）。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn random_bytes_never_panic() {
    let dir = scratch("random");
    let path = dir.join("keymap.json");
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    // JSON らしい断片を混ぜて、形式の検査の奥まで届く見本も作る。
    let pieces: &[&[u8]] = &[
        br#"{"format":"model-reviewer-keymap","version":1,"bindings":{"#,
        br#""tool.move":"#,
        br#""edit.redo":"#,
        br#""zzz":"#,
        br#""Shift+Command+Z""#,
        br#""Command+""#,
        br#""+""#,
        br#""F1""#,
        b"null",
        b"1e999",
        b"-0",
        b"[",
        b"]",
        b"{",
        b"}",
        b",",
        b":",
        "\"日本語\"".as_bytes(),
        b"\"\\u0000\"",
        b"\"\\ud800\"",
    ];
    for case in 0..10_000 {
        let mut bytes = Vec::new();
        if case % 2 == 0 {
            let len = rng.below(64);
            bytes.extend((0..len).map(|_| rng.next() as u8));
        } else {
            for _ in 0..rng.below(12) {
                bytes.extend_from_slice(pieces[rng.below(pieces.len())]);
            }
            if rng.below(2) == 0 {
                bytes.extend_from_slice(b"}}");
            }
        }
        std::fs::write(&path, &bytes).unwrap();
        let opened = Store::open(path.clone());
        // 表の不変の条件は、どんな中身でも保つ。
        let chords: Vec<Chord> = Action::ALL
            .iter()
            .filter_map(|&a| opened.keymap.chord(a))
            .collect();
        for (i, a) in chords.iter().enumerate() {
            assert!(!chords[i + 1..].contains(a), "見本 {case}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    assert_eq!(names(&dir), vec!["keymap.json"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failed_save_leaves_no_temp_file() {
    let dir = scratch("failed-save");
    let path = dir.join("keymap.json");
    let opened = Store::open(path.clone());
    assert_eq!(opened.store.state(), &StoreState::Ready);
    // 開いた後で、保存先がフォルダになる。
    std::fs::create_dir_all(path.join("inside")).unwrap();
    let mut store = opened.store;
    let mut keymap = opened.keymap;
    keymap.clear(Action::ToolMove);
    let err = store.save(&keymap).unwrap_err();
    assert!(
        err.starts_with("設定ファイルを置き換えられませんでした"),
        "{err}"
    );
    assert_eq!(names(&dir), vec!["keymap.json"]);
    assert!(path.join("inside").is_dir());
    // 割り当ては呼んだ側に残る（表は変えない）。
    assert_eq!(keymap.chord(Action::ToolMove), None);

    // 保存先のフォルダが作れない（開いた後で、親の場所にファイルができる）。
    let blocked = dir.join("plain");
    let mut store = Store::open(blocked.join("keymap.json")).store;
    assert_eq!(store.state(), &StoreState::Ready);
    std::fs::write(&blocked, b"x").unwrap();
    let err = store.save(&keymap).unwrap_err();
    assert!(
        err.starts_with("設定を保存するフォルダを作れませんでした"),
        "{err}"
    );
    assert_eq!(std::fs::read(&blocked).unwrap(), b"x");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opening_writes_nothing() {
    let dir = scratch("opening");
    // 無いフォルダの中の場所を開いても、フォルダを作らない。
    let missing = dir.join("not-yet").join("keymap.json");
    let opened = Store::open(missing.clone());
    assert_eq!(opened.store.state(), &StoreState::Ready);
    assert_eq!(opened.store.path(), Some(missing.as_path()));
    assert_eq!(opened.keymap, Keymap::default());
    assert!(!dir.join("not-yet").exists());
    assert!(names(&dir).is_empty());

    // 読めた・壊れた・新しい版のどれでも、開いただけでは中身も名前も変わらない。
    let files: [(&str, &[u8]); 4] = [
        (
            "good.json",
            br#"{"format":"model-reviewer-keymap","version":1,"bindings":{"tool.move":"T","zz":1,"tool.rotate":5}}"#,
        ),
        ("broken.json", b"{"),
        (
            "newer.json",
            br#"{"format":"model-reviewer-keymap","version":9,"bindings":{}}"#,
        ),
        ("dup.json", br#"{"format":"model-reviewer-keymap","version":1,"bindings":{"tool.move":"R"}}"#),
    ];
    for (name, bytes) in files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    let before = names(&dir);
    for (name, bytes) in files {
        let _ = Store::open(dir.join(name));
        assert_eq!(std::fs::read(dir.join(name)).unwrap(), bytes, "{name}");
    }
    assert_eq!(names(&dir), before);
    // 場所の無い持ち手は、保存しても何も書かない。
    let mut none = Store::none();
    assert_eq!(none.save(&Keymap::default()), Ok(()));
    assert_eq!(names(&dir), before);
    let _ = std::fs::remove_dir_all(&dir);
}
