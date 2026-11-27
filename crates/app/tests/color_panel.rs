//! 色のパネルの確定・取り消し・入力エラーを、入力用の構造体だけで確かめる。

mod support;

use egui::accesskit::Role;
use egui_kittest::{Harness, kittest::Queryable};
use model_reviewer::command::Command;
use model_reviewer::session::{ColorState, ColorView};
use model_reviewer::ui::color;
use model_reviewer_core::color::{SWATCHES, linear_to_srgb8, srgb8_to_linear, to_hex};

struct Probe {
    input: ColorView,
    commands: Vec<Command>,
}

fn panel(state: ColorState) -> Harness<'static, Probe> {
    Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(model_reviewer::fonts::install)
        .with_size(egui::vec2(250.0, 300.0))
        .build_ui_state(
            |ui, probe: &mut Probe| {
                let mut out = Vec::new();
                color::show(ui, &probe.input, &mut out);
                for command in &out {
                    if let Command::SetColor(rgb) = command {
                        probe.input.state = ColorState::One(linear_to_srgb8(*rgb));
                        probe.input.selection_rev.1 += 1;
                    }
                }
                probe.commands.extend(out);
            },
            Probe {
                input: ColorView {
                    state,
                    note: None,
                    locked: false,
                    selection_rev: (0, 0),
                },
                commands: Vec::new(),
            },
        )
}

fn replace(h: &mut Harness<'_, Probe>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
}

fn value(h: &Harness<'_, Probe>, label: &str) -> String {
    h.get_by_role_and_label(Role::TextInput, label)
        .value()
        .unwrap_or_default()
}

fn press(h: &mut Harness<'_, Probe>, key: egui::Key) {
    h.key_press(key);
    h.run();
}

#[test]
fn swatch_click_sets_color() {
    let mut h = panel(ColorState::One([0; 3]));
    for (index, (name, rgb)) in SWATCHES.into_iter().enumerate() {
        h.get_by_label(&format!("{name} #{}", to_hex(rgb))).click();
        h.run();
        assert_eq!(h.state().commands.len(), index + 1);
        assert_eq!(
            h.state().commands[index],
            Command::SetColor(srgb8_to_linear(rgb))
        );
        assert_eq!(value(&h, "16進の色"), to_hex(rgb));
    }
}

#[test]
fn hex_and_rgb_commit_on_enter() {
    let mut h = panel(ColorState::One([0; 3]));
    for (index, (field, text, expected)) in [
        ("16進の色", "#3d7bd9", [61, 123, 217]),
        ("R", "255", [255, 123, 217]),
        ("G", "0", [255, 0, 217]),
        ("B", "17", [255, 0, 17]),
    ]
    .into_iter()
    .enumerate()
    {
        replace(&mut h, field, text);
        assert_eq!(h.state().commands.len(), index, "入力途中");
        press(&mut h, egui::Key::Enter);
        assert_eq!(
            h.state().commands[index],
            Command::SetColor(srgb8_to_linear(expected))
        );
        assert_eq!(value(&h, "16進の色"), to_hex(expected));
    }
    replace(&mut h, "R", "2");
    press(&mut h, egui::Key::Tab);
    assert_eq!(value(&h, "16進の色"), "020011");
    replace(&mut h, "G", "99");
    press(&mut h, egui::Key::Escape);
    assert_eq!(value(&h, "G"), "0");
    assert_eq!(h.state().commands.len(), 5);
    // 入力位置を移すだけでは、丸めて表示した値を確定しない。
    h.get_by_role_and_label(Role::TextInput, "B").click();
    h.run();
    press(&mut h, egui::Key::Tab);
    assert_eq!(h.state().commands.len(), 5);
}

#[test]
fn bad_hex_is_refused_with_reason() {
    let mut h = panel(ColorState::One([1, 2, 3]));
    for (field, text, original, reason) in [
        ("16進の色", "ABCDE", "010203", "6桁の16進数を入れてください"),
        (
            "16進の色",
            "1122334",
            "010203",
            "6桁の16進数を入れてください",
        ),
        ("R", "-1", "1", "0〜255 の整数を入れてください"),
        ("G", "256", "2", "0〜255 の整数を入れてください"),
        ("B", "1.5", "3", "0〜255 の整数を入れてください"),
    ] {
        replace(&mut h, field, text);
        press(&mut h, egui::Key::Enter);
        h.get_by_label(reason);
        assert_eq!(value(&h, field), original);
    }
    assert!(h.state().commands.is_empty());
}

#[test]
fn mixed_and_no_faces_states() {
    let mut h = panel(ColorState::Mixed { count: 3 });
    h.get_by_label("複数の色（3色）");
    for label in ["16進の色", "R", "G", "B"] {
        assert!(value(&h, label).is_empty());
    }
    for (index, (label, text)) in [("R", "32"), ("G", "64"), ("B", "128")]
        .into_iter()
        .enumerate()
    {
        replace(&mut h, label, text);
        assert!(h.state().commands.is_empty());
        press(&mut h, egui::Key::Enter);
        if index < 2 {
            assert!(h.state().commands.is_empty());
        }
    }
    assert_eq!(
        h.state().commands,
        [Command::SetColor(srgb8_to_linear([32, 64, 128]))]
    );
    h.state_mut().input.state = ColorState::Mixed { count: 99 };
    h.state_mut().input.selection_rev.1 += 1;
    h.step();
    h.get_by_label("複数の色（99色以上）");

    let mut h = panel(ColorState::NoFaces);
    h.get_by_label("この部品には、色を付ける面がありません");
    for state in [ColorState::NoFaces, ColorState::One([1, 2, 3])] {
        h.state_mut().input.state = state;
        h.state_mut().input.locked = state != ColorState::NoFaces;
        h.step();
        h.get_by_label("赤 #D64545").click();
        h.run();
        h.get_by_role_and_label(Role::TextInput, "16進の色").click();
        h.run();
        assert!(!h.ctx.text_edit_focused());
        assert!(h.state().commands.is_empty());
    }
}

#[test]
fn leaving_the_field_for_another_part_paints_the_original_selection() {
    use model_reviewer_core::scene::{NodeId, SceneRead};
    let mut session = support::session();
    let original = NodeId(0);
    let other = NodeId(1);
    let other_color = session.doc.as_ref().unwrap().draws(other)[0].base_color;
    session.apply(Command::Select(Some(original)));
    let (mut h, _) = support::harness(session, egui::vec2(1100.0, 720.0));
    h.run_steps(3);
    let hex = support::widget_rect(&h, model_reviewer::ui::ids::COLOR_HEX).unwrap();
    support::click(&mut h, hex.center());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.step();
    h.event(egui::Event::Text("FF0000".to_owned()));
    h.step();
    assert!(h.state().history.is_empty());
    let row = support::widget_rect(&h, "parts.row.1").unwrap();
    support::click(&mut h, row.center());
    assert_eq!(h.state().selection, Some(other));
    let doc = h.state().doc.as_ref().unwrap();
    assert_eq!(doc.draws(original)[0].base_color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(doc.draws(other)[0].base_color, other_color);
    assert_eq!(h.state().history.len(), 1);
}

fn editing_number(field: &str) -> Harness<'static, model_reviewer::Session> {
    use model_reviewer_core::scene::NodeId;
    let mut session = support::session();
    session.apply(Command::Select(Some(NodeId(0))));
    let (mut h, _) = support::harness(session, egui::vec2(1100.0, 720.0));
    h.run_steps(3);
    let rect = support::widget_rect(&h, field).unwrap();
    support::click(&mut h, rect.center());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("25".into()));
    h.run();
    assert!(h.ctx.text_edit_focused(), "{field}");
    h
}

#[test]
fn close_treats_uncommitted_color_and_transform_fields_alike() {
    use model_reviewer::ui::ids;
    for field in [ids::INSPECTOR_POS_X, ids::COLOR_R] {
        let mut h = editing_number(field);
        assert!(!h.state_mut().on_close_requested(), "{field}");
        assert!(h.state().history.is_empty());
    }
}

#[test]
fn numeric_input_keeps_all_keys_in_its_commit_frame() {
    use egui::Key;
    use model_reviewer::ui::ids;
    use model_reviewer_core::scene::{NodeId, SceneRead};
    for field in [ids::COLOR_R, ids::INSPECTOR_POS_X] {
        let mut h = editing_number(field);
        for key in [Key::Backspace, Key::G, Key::Enter] {
            h.input_mut().events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.step();
        let doc = h.state().doc.as_ref().unwrap();
        assert_eq!(doc.node_count(), 2, "{field}");
        assert_eq!(
            h.state().tool,
            model_reviewer::command::Tool::Select,
            "{field}"
        );
        if field == ids::COLOR_R {
            assert_eq!(
                linear_to_srgb8(doc.draws(NodeId(0))[0].base_color[..3].try_into().unwrap())[0],
                2
            );
        } else {
            assert_eq!(doc.node(NodeId(0)).unwrap().local.translation.x, 2.0);
        }
    }
}

#[test]
fn color_fields_show_whole_values() {
    let view = ColorView {
        state: ColorState::One([227, 198, 59]),
        note: None,
        locked: false,
        selection_rev: (0, 0),
    };
    let mut h = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(model_reviewer::fonts::install)
        .with_size(egui::vec2(250.0, 300.0))
        .build_ui_state(
            move |ui, widest: &mut Vec<f32>| {
                color::show(ui, &view, &mut Vec::new());
                // 一番長い値（16進6桁と3桁の数）の幅を、画面と同じ字で測る。
                let font = egui::TextStyle::Body.resolve(ui.style());
                *widest = ["DDDDDD", "255", "255", "255"]
                    .map(|text| {
                        ui.painter()
                            .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
                            .size()
                            .x
                    })
                    .to_vec();
            },
            Vec::new(),
        );
    h.run();
    let widest = h.state().clone();
    for (label, text) in ["16進の色", "R", "G", "B"].into_iter().zip(widest) {
        let field = h.get_by_role_and_label(Role::TextInput, label).rect();
        // 欄の内側の余白（左右 4）とカーソルの分を足しても入ること。
        assert!(
            field.width() >= text + 10.0,
            "{label}: 欄 {} に文字 {text} が入らない",
            field.width()
        );
    }
}
