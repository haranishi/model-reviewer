//! 吸着・断面・単位の試験が共有する小さな場面。GPUは使わない。
//!
//! 2つの立方体（一辺2）を Z 軸に並べる。手前（部品0）は z = 1〜3、奥（部品1）は z = −3〜−1。
//! 視点は +Z 側から原点を見る。断面を Z 軸の中央（z = 0）で入れると、手前が丸ごと消える。

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use glam::{Affine3A, Vec2, Vec3};
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::Command;
use model_reviewer::session::Session;
use model_reviewer_core::camera::{OrbitCamera, Projection};
use model_reviewer_core::identity::{ModelIdentity, ModelRef};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::pick::PickIndex;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::{Draw, Model, ModelStats, Node, PrimId, Primitive, Vertex};
use model_reviewer_core::texture::Textures;

pub const VIEW: [u32; 2] = [800, 800];
pub const NEAR_CENTER: Vec3 = Vec3::new(0.0, 0.0, 2.0);
pub const FAR_CENTER: Vec3 = Vec3::new(0.0, 0.0, -2.0);

fn vertex(position: Vec3, normal: Vec3) -> Vertex {
    Vertex {
        position: position.to_array(),
        normal: normal.to_array(),
    }
}

/// 中心が原点で一辺2の立方体。面ごとに4頂点、面は外向き。
pub fn cube() -> Primitive {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let mut n = Vec3::ZERO;
            n[axis] = sign;
            let mut u = Vec3::ZERO;
            let mut v = Vec3::ZERO;
            u[(axis + 1) % 3] = 1.0;
            v[(axis + 2) % 3] = 1.0;
            if sign < 0.0 {
                std::mem::swap(&mut u, &mut v);
            }
            let base = vertices.len() as u32;
            for corner in [n - u - v, n + u - v, n + u + v, n - u + v] {
                vertices.push(vertex(corner, n));
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    Primitive {
        vertices,
        indices,
        local_aabb: Aabb {
            min: Vec3::splat(-1.0),
            max: Vec3::splat(1.0),
        },
        has_normals: true,
    }
}

/// 手前と奥の2つの立方体（どちらも根）。
pub fn two_cubes() -> Model {
    let prim = cube();
    let node = |name: &str, index: u32, center: Vec3| {
        let world = Affine3A::from_translation(center);
        Node {
            name: name.to_owned(),
            gltf_index: index,
            parent: None,
            subtree_end: index + 1,
            depth: 0,
            local: world,
            world,
            draws: index..index + 1,
            world_aabb: prim.local_aabb.transformed(&world),
        }
    };
    let nodes = vec![node("手前", 0, NEAR_CENTER), node("奥", 1, FAR_CENTER)];
    let bounds = nodes[0].world_aabb.union(nodes[1].world_aabb);
    Model {
        textures: Textures::default(),
        nodes,
        draws: vec![
            Draw {
                prim: PrimId(0),
                base_color: [0.8, 0.8, 0.8, 1.0],
                tex: None,
            },
            Draw {
                prim: PrimId(0),
                base_color: [0.6, 0.7, 0.9, 1.0],
                tex: None,
            },
        ],
        primitives: vec![prim],
        bounds,
        stats: ModelStats::default(),
        warnings: Vec::new(),
    }
}

/// 2つの立方体を開いた状態（読み込みを通さない）。視点は +Z 側から原点を見る。
pub fn session() -> Session {
    let model = Arc::new(two_cubes());
    let mut session = Session::default();
    session.review = Some(ReviewState::new(&model));
    session.model_ref = Some(ModelRef {
        file_name: "two-cubes.glb".to_owned(),
        path: "two-cubes.glb".into(),
        relative_path: None,
        identity: ModelIdentity::from_bytes(b"two-cubes"),
        stl_unit: None,
        stl_up: None,
    });
    session.pick_index = Some(PickIndex::build(&*model));
    session.set_model(model);
    session.set_viewport(VIEW);
    session.camera = OrbitCamera {
        target: Vec3::ZERO,
        yaw: 0.0,
        pitch: 0.0,
        distance: 14.0,
        projection: Projection::Perspective,
        ..OrbitCamera::default()
    };
    let _ = session.take_scene_dirty();
    session
}

pub fn aspect() -> f32 {
    VIEW[0] as f32 / VIEW[1] as f32
}

/// ワールドの点の画面上の位置を、物理画素で `(dx, dy)` だけずらした正規化座標（+dy は上）。
pub fn ndc_of(session: &Session, world: Vec3, dx: f32, dy: f32) -> Vec2 {
    let ndc = session
        .camera
        .project(world, aspect())
        .expect("画面に映る点");
    ndc + Vec2::new(dx * 2.0 / VIEW[0] as f32, dy * 2.0 / VIEW[1] as f32)
}

pub fn close3(actual: Vec3, expected: Vec3) -> bool {
    (actual - expected).length() <= 1.0e-4 * (1.0 + expected.length())
}

pub fn near(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= 1.0e-6 + expected.abs() * 1.0e-5
}

/// 画面ごと動かす試験台。積まれた `Command` を `seen` に残す。
pub fn harness(
    session: Session,
    size: egui::Vec2,
) -> (Harness<'static, Session>, Arc<Mutex<Vec<Command>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&seen);
    let harness = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(40)
        .with_size(size)
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui_state(
            move |ui, session| {
                let mut commands = Vec::new();
                let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                commands.extend(model_reviewer::shortcut_commands(ui.ctx(), session));
                slot.lock().unwrap().extend(commands.iter().cloned());
                for command in commands {
                    session.apply(command);
                }
                session.set_viewport(pixels);
            },
            session,
        );
    (harness, seen)
}

pub fn widget_rect(harness: &Harness<Session>, name: &str) -> Option<egui::Rect> {
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    frame
        .entries()
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| {
            egui::Rect::from_min_max(
                egui::pos2(entry.min[0], entry.min[1]),
                egui::pos2(entry.max[0], entry.max[1]),
            )
        })
}

pub fn widget_names(harness: &Harness<Session>) -> Vec<(String, egui::Rect)> {
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    frame
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                egui::Rect::from_min_max(
                    egui::pos2(entry.min[0], entry.min[1]),
                    egui::pos2(entry.max[0], entry.max[1]),
                ),
            )
        })
        .collect()
}

pub fn click(harness: &mut Harness<Session>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.step();
    harness.step();
}

/// 3Dビューの上へポインタを置く（キーが3Dビューの上で働くように）。
pub fn hover_view(harness: &mut Harness<Session>) {
    let view = widget_rect(harness, model_reviewer::ui::ids::VIEWPORT).expect("3Dビュー");
    harness.event(egui::Event::PointerMoved(view.center()));
    harness.step();
}
