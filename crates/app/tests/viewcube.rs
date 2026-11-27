//! 向きの立方体。面の見え方と当たり判定は画面なしで、押す・ドラッグは egui_kittest で確かめる。

use egui_kittest::Harness;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, ViewDir};
use model_reviewer::ui::ids;
use model_reviewer::ui::viewcube::{self, ViewCubeInput, hit_face, visible_faces};

const RECT_MIN: egui::Pos2 = egui::pos2(300.0, 20.0);

fn cube_rect() -> egui::Rect {
    egui::Rect::from_min_size(RECT_MIN, egui::vec2(72.0, 72.0))
}

fn dirs(faces: &[viewcube::CubeFace]) -> Vec<ViewDir> {
    faces.iter().map(|face| face.dir).collect()
}

fn centroid(points: &[egui::Pos2]) -> egui::Pos2 {
    let sum = points
        .iter()
        .fold(egui::Vec2::ZERO, |acc, p| acc + p.to_vec2());
    (sum / points.len() as f32).to_pos2()
}

#[test]
fn straight_views_show_one_face() {
    let rect = cube_rect();
    let cases = [
        (0.0_f32, 0.0_f32, ViewDir::Front, "前"),
        (180.0, 0.0, ViewDir::Back, "後"),
        (90.0, 0.0, ViewDir::Right, "右"),
        (-90.0, 0.0, ViewDir::Left, "左"),
        (0.0, 90.0, ViewDir::Top, "上"),
        (0.0, -90.0, ViewDir::Bottom, "下"),
    ];
    for (yaw, pitch, dir, label) in cases {
        let faces = visible_faces(yaw.to_radians(), pitch.to_radians(), rect);
        assert_eq!(dirs(&faces), vec![dir], "{label}");
        assert_eq!(faces[0].label, label);
        assert_eq!(faces[0].polygon.len(), 4);
        let center = centroid(&faces[0].polygon);
        assert!(center.distance(rect.center()) < 0.01, "{label}: {center:?}");
        assert_eq!(hit_face(&faces, rect.center()), Some(dir));
    }
}

#[test]
fn oblique_view_shows_three_faces_inside_the_rect() {
    let rect = cube_rect();
    // 初期視点と同じ向き（右上の手前から見下ろす）。
    let faces = visible_faces(35_f32.to_radians(), 20_f32.to_radians(), rect);
    let mut seen = dirs(&faces);
    seen.sort_by_key(|dir| format!("{dir:?}"));
    assert_eq!(seen, vec![ViewDir::Front, ViewDir::Right, ViewDir::Top]);
    for face in &faces {
        for p in &face.polygon {
            assert!(rect.contains(*p), "{:?} {p:?}", face.dir);
        }
        // 面の中心を押すとその面。
        assert_eq!(
            hit_face(&faces, centroid(&face.polygon)),
            Some(face.dir),
            "{:?}",
            face.dir
        );
    }
    // 右の面は画面の右、上の面は画面の上にある。
    let center_of = |dir| centroid(&faces.iter().find(|face| face.dir == dir).unwrap().polygon);
    assert!(center_of(ViewDir::Right).x > center_of(ViewDir::Front).x);
    assert!(center_of(ViewDir::Top).y < center_of(ViewDir::Front).y);
    // 奥から手前の順（最も正面を向いた面が最後）。
    assert_eq!(faces.last().unwrap().dir, ViewDir::Front);
    // 立方体の外は当たらない。
    assert_eq!(hit_face(&faces, rect.min + egui::vec2(1.0, 1.0)), None);
    assert_eq!(hit_face(&faces, egui::pos2(0.0, 0.0)), None);
}

#[test]
fn looking_from_below_and_behind() {
    let rect = cube_rect();
    let faces = visible_faces(200_f32.to_radians(), (-30_f32).to_radians(), rect);
    let seen = dirs(&faces);
    assert!(seen.contains(&ViewDir::Back));
    assert!(seen.contains(&ViewDir::Bottom));
    assert!(!seen.contains(&ViewDir::Front));
    assert!(!seen.contains(&ViewDir::Top));
}

#[test]
fn bad_input_gives_no_faces() {
    assert!(visible_faces(0.0, 0.0, egui::Rect::NOTHING).is_empty());
    let empty = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(0.0, 10.0));
    assert!(visible_faces(0.0, 0.0, empty).is_empty());
    assert_eq!(hit_face(&[], egui::pos2(1.0, 1.0)), None);
}

struct Probe {
    input: ViewCubeInput,
    out: Vec<Command>,
}

fn harness(yaw_deg: f32, pitch_deg: f32) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::vec2(420.0, 200.0))
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            |ui, probe: &mut Probe| {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::NULL, WidgetFrame::default()));
                let mut out = Vec::new();
                viewcube::show(ui, &probe.input, &mut out);
                probe.out.extend(out);
            },
            Probe {
                input: ViewCubeInput {
                    yaw: yaw_deg.to_radians(),
                    pitch: pitch_deg.to_radians(),
                    rect: cube_rect(),
                },
                out: Vec::new(),
            },
        )
}

fn registered(harness: &Harness<'static, Probe>) -> Vec<(String, egui::Rect)> {
    harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default()
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                egui::Rect::from_min_max(entry.min.into(), entry.max.into()),
            )
        })
        .collect()
}

fn pointer(harness: &mut Harness<'static, Probe>, pos: egui::Pos2, pressed: Option<bool>) {
    harness.event(egui::Event::PointerMoved(pos));
    if let Some(pressed) = pressed {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.step();
}

fn click(harness: &mut Harness<'static, Probe>, pos: egui::Pos2) {
    pointer(harness, pos, None);
    pointer(harness, pos, Some(true));
    pointer(harness, pos, Some(false));
    harness.step();
}

#[test]
fn pressing_each_visible_face_snaps_to_it() {
    let mut harness = harness(35.0, 20.0);
    harness.step();
    harness.step();
    let names = registered(&harness);
    let mut found: Vec<&str> = names.iter().map(|(name, _)| name.as_str()).collect();
    found.sort_unstable();
    assert_eq!(
        found,
        vec![ids::VIEWCUBE_FRONT, ids::VIEWCUBE_RIGHT, ids::VIEWCUBE_TOP]
    );
    for (name, rect) in names {
        harness.state_mut().out.clear();
        click(&mut harness, rect.center());
        let want = match name.as_str() {
            ids::VIEWCUBE_FRONT => ViewDir::Front,
            ids::VIEWCUBE_RIGHT => ViewDir::Right,
            _ => ViewDir::Top,
        };
        assert_eq!(harness.state().out, vec![Command::SnapView(want)], "{name}");
    }
}

#[test]
fn pressing_outside_the_faces_does_nothing() {
    let mut harness = harness(0.0, 0.0);
    harness.step();
    // 正面だけが見える。矩形の角は立方体の外。
    click(&mut harness, cube_rect().min + egui::vec2(2.0, 2.0));
    assert!(harness.state().out.is_empty(), "{:?}", harness.state().out);
    let names = registered(&harness);
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].0, ids::VIEWCUBE_FRONT);
}

#[test]
fn dragging_orbits_instead_of_snapping() {
    let mut harness = harness(35.0, 20.0);
    harness.step();
    let start = cube_rect().center();
    pointer(&mut harness, start, None);
    pointer(&mut harness, start, Some(true));
    pointer(&mut harness, start + egui::vec2(10.0, 0.0), None);
    pointer(&mut harness, start + egui::vec2(20.0, 6.0), None);
    pointer(&mut harness, start + egui::vec2(20.0, 6.0), Some(false));
    let out = &harness.state().out;
    assert!(!out.is_empty());
    assert!(
        out.iter()
            .all(|command| matches!(command, Command::Orbit(_))),
        "{out:?}"
    );
    let total = out
        .iter()
        .fold(glam::Vec2::ZERO, |acc, command| match command {
            Command::Orbit(delta) => acc + *delta,
            _ => acc,
        });
    // ビューと同じ向き（y は上が正）。
    assert!((total.x - 20.0).abs() < 0.5, "{total:?}");
    assert!((total.y + 6.0).abs() < 0.5, "{total:?}");
}

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_viewcube_for_visual_check() {
    for (yaw, pitch, name) in [
        (35.0, 20.0, "viewcube-home"),
        (0.0, 0.0, "viewcube-front"),
        (0.0, 90.0, "viewcube-top"),
        (200.0, -30.0, "viewcube-below"),
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 120.0))
            .with_pixels_per_point(2.0)
            .wgpu()
            .with_accessibility_check(false)
            .allow_missing_glyphs()
            .with_context_setup(|ctx| {
                model_reviewer::fonts::install(ctx);
                model_reviewer::theme::install(ctx);
            })
            .build_ui(move |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(64.0, 24.0), egui::vec2(72.0, 72.0));
                ui.painter()
                    .rect_filled(ui.max_rect(), 0.0, model_reviewer::theme::VIEW_BG);
                let input = ViewCubeInput {
                    yaw: f32::to_radians(yaw),
                    pitch: f32::to_radians(pitch),
                    rect,
                };
                viewcube::show(ui, &input, &mut Vec::new());
            });
        for _ in 0..4 {
            harness.step();
        }
        save(&mut harness, name);
    }
}

fn save<S>(harness: &mut Harness<'static, S>, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-check");
    std::fs::create_dir_all(&dir).expect("画像の出力先");
    let path = dir.join(format!("{name}.png"));
    harness
        .render()
        .expect("描画")
        .save(&path)
        .expect("PNGの保存");
    eprintln!("wrote target/ui-check/{name}.png");
}
