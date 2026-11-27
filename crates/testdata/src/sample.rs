//! 確認用の手押し車。回転はリテラルで、期待するワールド位置は同じ四元数から計算する。

use crate::glb_writer::{num, vec3_json, vec4_json, write_glb};
use crate::shapes::{
    Geometry, box_extents, box_mesh, cylinder_mesh, scale_uniform, tetrahedron,
    tetrahedron_with_normals, translate,
};

pub const NODE_COUNT: u32 = 12;
pub const MESH_COUNT: u32 = 6;
pub const PRIMITIVE_COUNT: u32 = 6;
pub const UNIQUE_TRIANGLES: u64 = 140;
pub const PLACED_TRIANGLES: u64 = 524;

pub const NODE_NAMES: [&str; 12] = [
    "assembly",
    "base",
    "axle_front",
    "wheel_FL",
    "wheel_FR",
    "axle_rear",
    "wheel_RL",
    "wheel_RR",
    "spare",
    "arm",
    "arm_tip",
    "lamp",
];

/// Y を 15° 回す回転。成分は (x, y, z, w)。
#[allow(clippy::approx_constant, clippy::excessive_precision)]
pub const ROT_Y_15: [f32; 4] = [0.0, 0.13052619222005157, 0.0, 0.9914448613738104];
/// Z を 30° 回す回転。
#[allow(clippy::approx_constant, clippy::excessive_precision)]
pub const ROT_Z_30: [f32; 4] = [0.0, 0.0, 0.25881904510252074, 0.9659258262890683];
/// X を 90° 回す回転。
#[allow(clippy::approx_constant, clippy::excessive_precision)]
pub const ROT_X_90: [f32; 4] = [0.7071067811865476, 0.0, 0.0, 0.7071067811865476];

const IDENT_Q: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// 組み立て全体を持ち上げる量。車輪（半径 0.32、車軸から 0.25 下）の下端が床（高さ 0）に載る。
/// 回転は Y まわりだけなので、高さはこの値だけで決まる。
pub const LIFT: [f32; 3] = [0.0, 0.57, 0.0];

pub const COLOR_BASE: [f32; 4] = [0.45, 0.5, 0.55, 1.0];
pub const COLOR_WHEEL: [f32; 4] = [0.15, 0.15, 0.16, 1.0];
pub const COLOR_ARM: [f32; 4] = [0.9, 0.45, 0.1, 1.0];
pub const COLOR_TIP: [f32; 4] = [0.95, 0.85, 0.15, 1.0];
pub const COLOR_TIP_END: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
pub const COLOR_LAMP: [f32; 4] = [0.2, 0.35, 0.9, 1.0];
pub const COLOR_SPARE: [f32; 4] = [0.15, 0.7, 0.25, 1.0];

/// ノード名と、その原点のワールド座標。誤差の許容は読み込み側のテストが 1e-5 で見る。
pub const EXPECTED_WORLD_POSITIONS: [(&str, [f32; 3]); 12] = expected_world_positions();

#[derive(Clone, Copy)]
struct Xform {
    c: [[f32; 3]; 3],
    t: [f32; 3],
}

const fn quat_cols(q: [f32; 4]) -> [[f32; 3]; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let x2 = x + x;
    let y2 = y + y;
    let z2 = z + z;
    let xx = x * x2;
    let xy = x * y2;
    let xz = x * z2;
    let yy = y * y2;
    let yz = y * z2;
    let zz = z * z2;
    let wx = w * x2;
    let wy = w * y2;
    let wz = w * z2;
    [
        [1.0 - (yy + zz), xy + wz, xz - wy],
        [xy - wz, 1.0 - (xx + zz), yz + wx],
        [xz + wy, yz - wx, 1.0 - (xx + yy)],
    ]
}

const fn scale_col(col: [f32; 3], scale: f32) -> [f32; 3] {
    [col[0] * scale, col[1] * scale, col[2] * scale]
}

const fn trs(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> Xform {
    let r = quat_cols(q);
    Xform {
        c: [
            scale_col(r[0], s[0]),
            scale_col(r[1], s[1]),
            scale_col(r[2], s[2]),
        ],
        t,
    }
}

const fn apply(a: Xform, v: [f32; 3]) -> [f32; 3] {
    [
        a.c[0][0] * v[0] + a.c[1][0] * v[1] + a.c[2][0] * v[2],
        a.c[0][1] * v[0] + a.c[1][1] * v[1] + a.c[2][1] * v[2],
        a.c[0][2] * v[0] + a.c[1][2] * v[1] + a.c[2][2] * v[2],
    ]
}

const fn mul(a: Xform, b: Xform) -> Xform {
    Xform {
        c: [apply(a, b.c[0]), apply(a, b.c[1]), apply(a, b.c[2])],
        t: {
            let rotated = apply(a, b.t);
            [
                rotated[0] + a.t[0],
                rotated[1] + a.t[1],
                rotated[2] + a.t[2],
            ]
        },
    }
}

const fn expected_world_positions() -> [(&'static str, [f32; 3]); 12] {
    let one = [1.0, 1.0, 1.0];
    let assembly = trs(LIFT, ROT_Y_15, one);
    let base = mul(assembly, trs([0.0, 0.0, 0.0], IDENT_Q, [2.0, 0.2, 1.2]));
    let axle_front = mul(assembly, trs([0.7, 0.0, 0.0], IDENT_Q, one));
    let axle_rear = mul(assembly, trs([-0.7, 0.0, 0.0], IDENT_Q, one));
    let wheel_fl = mul(axle_front, trs([0.0, -0.25, 0.68], ROT_X_90, one));
    let wheel_fr = mul(axle_front, trs([0.0, -0.25, -0.68], ROT_X_90, one));
    let wheel_rl = mul(axle_rear, trs([0.0, -0.25, 0.68], ROT_X_90, one));
    let wheel_rr = mul(axle_rear, trs([0.0, -0.25, -0.68], ROT_X_90, one));
    let spare = mul(axle_rear, trs([0.08, 0.163, 0.0], IDENT_Q, one));
    let arm = mul(assembly, trs([0.05, 0.501, 0.0], ROT_Z_30, [1.0, 2.0, 1.0]));
    let arm_tip = mul(arm, trs([0.0, 0.2525, 0.0], IDENT_Q, one));
    let lamp = mul(assembly, trs([0.75, 0.172, 0.35], IDENT_Q, one));
    [
        ("assembly", assembly.t),
        ("base", base.t),
        ("axle_front", axle_front.t),
        ("wheel_FL", wheel_fl.t),
        ("wheel_FR", wheel_fr.t),
        ("axle_rear", axle_rear.t),
        ("wheel_RL", wheel_rl.t),
        ("wheel_RR", wheel_rr.t),
        ("spare", spare.t),
        ("arm", arm.t),
        ("arm_tip", arm_tip.t),
        ("lamp", lamp.t),
    ]
}

struct Bank {
    bin: Vec<u8>,
    views: Vec<(u32, u32)>,
    accessors: Vec<String>,
}

impl Bank {
    fn new() -> Self {
        Self {
            bin: Vec::new(),
            views: Vec::new(),
            accessors: Vec::new(),
        }
    }

    fn add_vec3(&mut self, values: &[[f32; 3]], min_max: bool) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 12);
        for value in values {
            for component in value {
                bytes.extend_from_slice(&component.to_le_bytes());
            }
        }
        let view = self.add_bytes(&bytes);
        let mut json = format!(
            "{{\"bufferView\":{view},\"componentType\":5126,\"count\":{},\"type\":\"VEC3\"",
            values.len()
        );
        if min_max {
            let (min, max) = bounds(values);
            json.push_str(&format!(
                ",\"min\":{},\"max\":{}",
                vec3_json(min),
                vec3_json(max)
            ));
        }
        json.push('}');
        self.accessors.push(json);
        self.accessors.len() - 1
    }

    fn add_indices(&mut self, values: &[u32]) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let view = self.add_bytes(&bytes);
        self.accessors.push(format!(
            "{{\"bufferView\":{view},\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}",
            values.len()
        ));
        self.accessors.len() - 1
    }

    fn add_bytes(&mut self, bytes: &[u8]) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len() as u32;
        self.bin.extend_from_slice(bytes);
        self.views.push((offset, self.bin.len() as u32 - offset));
        self.views.len() - 1
    }
}

fn bounds(values: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX, f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN, f32::MIN];
    for value in values {
        for axis in 0..3 {
            min[axis] = min[axis].min(value[axis]);
            max[axis] = max[axis].max(value[axis]);
        }
    }
    (min, max)
}

struct Stored {
    position: usize,
    normal: Option<usize>,
    indices: usize,
}

fn store(bank: &mut Bank, geometry: &Geometry) -> Stored {
    let position = bank.add_vec3(&geometry.positions, true);
    let normal = geometry
        .normals
        .as_ref()
        .map(|normals| bank.add_vec3(normals, false));
    let indices = bank.add_indices(&geometry.indices);
    Stored {
        position,
        normal,
        indices,
    }
}

fn primitive(stored: &Stored, material: usize) -> String {
    let mut attributes = format!("\"POSITION\":{}", stored.position);
    if let Some(normal) = stored.normal {
        attributes.push_str(&format!(",\"NORMAL\":{normal}"));
    }
    format!(
        "{{\"attributes\":{{{attributes}}},\"indices\":{},\"material\":{material}}}",
        stored.indices
    )
}

fn material(color: [f32; 4]) -> String {
    format!(
        "{{\"pbrMetallicRoughness\":{{\"baseColorFactor\":{}}}}}",
        vec4_json(color)
    )
}

enum NodeTransform {
    Trs {
        translation: Option<[f32; 3]>,
        rotation: Option<[f32; 4]>,
        scale: Option<[f32; 3]>,
    },
    Matrix([f32; 16]),
}

struct NodeSpec {
    name: &'static str,
    children: &'static [usize],
    mesh: Option<usize>,
    transform: NodeTransform,
}

fn node_json(spec: &NodeSpec) -> String {
    let mut json = format!("{{\"name\":\"{}\"", spec.name);
    if !spec.children.is_empty() {
        let children = spec
            .children
            .iter()
            .map(|child| child.to_string())
            .collect::<Vec<_>>()
            .join(",");
        json.push_str(&format!(",\"children\":[{children}]"));
    }
    if let Some(mesh) = spec.mesh {
        json.push_str(&format!(",\"mesh\":{mesh}"));
    }
    match spec.transform {
        NodeTransform::Trs {
            translation,
            rotation,
            scale,
        } => {
            if let Some(translation) = translation {
                json.push_str(&format!(",\"translation\":{}", vec3_json(translation)));
            }
            if let Some(rotation) = rotation {
                json.push_str(&format!(",\"rotation\":{}", vec4_json(rotation)));
            }
            if let Some(scale) = scale {
                json.push_str(&format!(",\"scale\":{}", vec3_json(scale)));
            }
        }
        NodeTransform::Matrix(matrix) => {
            let values = matrix
                .iter()
                .map(|value| num(*value))
                .collect::<Vec<_>>()
                .join(",");
            json.push_str(&format!(",\"matrix\":[{values}]"));
        }
    }
    json.push('}');
    json
}

pub fn bytes() -> Vec<u8> {
    let mut bank = Bank::new();
    let base = store(&mut bank, &box_mesh());
    let wheel = store(&mut bank, &cylinder_mesh());
    let arm = store(&mut bank, &box_extents([0.09, 0.2, 0.09]));
    let tip_box = store(&mut bank, &box_extents([0.13, 0.05, 0.13]));
    let tip_end = store(
        &mut bank,
        &translate(
            &scale_uniform(&tetrahedron_with_normals(), 0.14),
            [0.0, 0.0875, 0.0],
        ),
    );
    let lamp = store(&mut bank, &tetrahedron());

    let meshes = [
        format!("[{}]", primitive(&base, 0)),
        format!("[{}]", primitive(&wheel, 1)),
        format!("[{}]", primitive(&arm, 2)),
        format!("[{},{}]", primitive(&tip_box, 3), primitive(&tip_end, 4)),
        format!("[{}]", primitive(&lamp, 5)),
        format!("[{}]", primitive(&wheel, 6)),
    ];
    let materials = [
        material(COLOR_BASE),
        material(COLOR_WHEEL),
        material(COLOR_ARM),
        material(COLOR_TIP),
        material(COLOR_TIP_END),
        material(COLOR_LAMP),
        material(COLOR_SPARE),
    ];
    let nodes = [
        NodeSpec {
            name: "assembly",
            children: &[1, 2, 5, 9, 11],
            mesh: None,
            transform: NodeTransform::Trs {
                translation: Some(LIFT),
                rotation: Some(ROT_Y_15),
                scale: None,
            },
        },
        NodeSpec {
            name: "base",
            children: &[],
            mesh: Some(0),
            transform: NodeTransform::Trs {
                translation: None,
                rotation: None,
                scale: Some([2.0, 0.2, 1.2]),
            },
        },
        NodeSpec {
            name: "axle_front",
            children: &[3, 4],
            mesh: None,
            transform: NodeTransform::Trs {
                translation: Some([0.7, 0.0, 0.0]),
                rotation: None,
                scale: None,
            },
        },
        NodeSpec {
            name: "wheel_FL",
            children: &[],
            mesh: Some(1),
            transform: NodeTransform::Trs {
                translation: Some([0.0, -0.25, 0.68]),
                rotation: Some(ROT_X_90),
                scale: None,
            },
        },
        NodeSpec {
            name: "wheel_FR",
            children: &[],
            mesh: Some(1),
            transform: NodeTransform::Trs {
                translation: Some([0.0, -0.25, -0.68]),
                rotation: Some(ROT_X_90),
                scale: None,
            },
        },
        NodeSpec {
            name: "axle_rear",
            children: &[6, 7, 8],
            mesh: None,
            transform: NodeTransform::Trs {
                translation: Some([-0.7, 0.0, 0.0]),
                rotation: None,
                scale: None,
            },
        },
        NodeSpec {
            name: "wheel_RL",
            children: &[],
            mesh: Some(1),
            transform: NodeTransform::Trs {
                translation: Some([0.0, -0.25, 0.68]),
                rotation: Some(ROT_X_90),
                scale: None,
            },
        },
        NodeSpec {
            name: "wheel_RR",
            children: &[],
            mesh: Some(1),
            transform: NodeTransform::Trs {
                translation: Some([0.0, -0.25, -0.68]),
                rotation: Some(ROT_X_90),
                scale: None,
            },
        },
        NodeSpec {
            name: "spare",
            children: &[],
            mesh: Some(5),
            transform: NodeTransform::Trs {
                translation: Some([0.08, 0.163, 0.0]),
                rotation: None,
                scale: None,
            },
        },
        NodeSpec {
            name: "arm",
            children: &[10],
            mesh: Some(2),
            transform: NodeTransform::Trs {
                translation: Some([0.05, 0.501, 0.0]),
                rotation: Some(ROT_Z_30),
                scale: Some([1.0, 2.0, 1.0]),
            },
        },
        NodeSpec {
            name: "arm_tip",
            children: &[],
            mesh: Some(3),
            transform: NodeTransform::Trs {
                translation: Some([0.0, 0.2525, 0.0]),
                rotation: None,
                scale: None,
            },
        },
        NodeSpec {
            name: "lamp",
            children: &[],
            mesh: Some(4),
            transform: NodeTransform::Matrix([
                0.28, 0.0, 0.0, 0.0, 0.0, 0.28, 0.0, 0.0, 0.0, 0.0, 0.28, 0.0, 0.75, 0.172, 0.35,
                1.0,
            ]),
        },
    ];
    let views = bank
        .views
        .iter()
        .map(|(offset, length)| {
            format!("{{\"buffer\":0,\"byteOffset\":{offset},\"byteLength\":{length}}}")
        })
        .collect::<Vec<_>>()
        .join(",");
    let node_jsons = nodes.iter().map(node_json).collect::<Vec<_>>().join(",");
    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"model-reviewer-testdata\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[0]}}],\"nodes\":[{node_jsons}],\"meshes\":[{}],\"materials\":[{}],\"accessors\":[{}],\"bufferViews\":[{views}],\"buffers\":[{{\"byteLength\":{}}}]}}",
        meshes
            .iter()
            .map(|prims| format!("{{\"primitives\":{prims}}}"))
            .collect::<Vec<_>>()
            .join(","),
        materials.join(","),
        bank.accessors.join(","),
        bank.bin.len()
    );
    write_glb(&json, Some(&bank.bin))
}

#[cfg(test)]
mod tests {
    use super::bytes;

    #[test]
    fn two_generations_match_bytes() {
        assert_eq!(bytes(), bytes());
    }
}
