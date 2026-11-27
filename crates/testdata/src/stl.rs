//! STL の試験データ（見本の箱、重い STL、壊れた STL）。`model-reviewer-stl` には依存しない。
//!
//! ASCII の改行は呼び出し側が選ぶ（`\n` と `\r\n` の両方を生成器で作り、リポジトリの改行の変換に頼らない）。

/// 三角形1つ（頂点3つ）。
pub type Triangle = [[f32; 3]; 3];

/// 見本の箱の大きさ（mm）。
pub const STL_SAMPLE_SIZE: [f32; 3] = [20.0, 30.0, 40.0];
/// 見本の箱の三角形の数。
pub const STL_SAMPLE_TRIANGLES: u32 = 12;
/// 見本の箱の頂点の数（まとめた後）。
pub const STL_SAMPLE_VERTICES: u32 = 8;

const SAMPLE_HEADER: &[u8] = b"model-reviewer sample box 20 x 30 x 40 mm";

/// 原点の角から `size` まで伸びる箱の三角形12個。外向きに反時計回り。
pub fn box_triangles(size: [f32; 3]) -> Vec<Triangle> {
    let [x, y, z] = size;
    let p = |i: u8| {
        [
            if i & 1 != 0 { x } else { 0.0 },
            if i & 2 != 0 { y } else { 0.0 },
            if i & 4 != 0 { z } else { 0.0 },
        ]
    };
    // 面ごとの四角形（外から見て反時計回り）。
    let quads: [[u8; 4]; 6] = [
        [0, 2, 3, 1], // -Z
        [4, 5, 7, 6], // +Z
        [0, 1, 5, 4], // -Y
        [2, 6, 7, 3], // +Y
        [0, 4, 6, 2], // -X
        [1, 3, 7, 5], // +X
    ];
    let mut out = Vec::with_capacity(12);
    for [a, b, c, d] in quads {
        out.push([p(a), p(b), p(c)]);
        out.push([p(a), p(c), p(d)]);
    }
    out
}

/// 三角形の面の法線（ファイルに書くだけ。読み込みは使わない）。
fn face_normal(t: &Triangle) -> [f32; 3] {
    let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 0.0 && len.is_finite() {
        [n[0] / len, n[1] / len, n[2] / len]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// バイナリ STL。ヘッダは `header` を80バイトに切るか0で詰める。三角形の数は実際の数。
pub fn binary_stl(header: &[u8], triangles: &[Triangle]) -> Vec<u8> {
    binary_stl_with_count(header, triangles, triangles.len() as u32)
}

/// 三角形の数の欄に `claimed` を書いたバイナリ STL（壊れた入力を作るため）。
pub fn binary_stl_with_count(header: &[u8], triangles: &[Triangle], claimed: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(84 + 50 * triangles.len());
    let mut head = [0u8; 80];
    let used = header.len().min(80);
    head[..used].copy_from_slice(&header[..used]);
    out.extend_from_slice(&head);
    out.extend_from_slice(&claimed.to_le_bytes());
    for triangle in triangles {
        push_binary_triangle(&mut out, triangle);
    }
    out
}

fn push_binary_triangle(out: &mut Vec<u8>, triangle: &Triangle) {
    for value in face_normal(triangle) {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for vertex in triangle {
        for value in vertex {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    // 付属の2バイト（読み込みは使わない）。0でない値を入れて、無視されることを確かめられるようにする。
    out.extend_from_slice(&0x7c1fu16.to_le_bytes());
}

/// ASCII STL。`solids` の1つずつを `solid`〜`endsolid` で書く。改行は `newline`。
pub fn ascii_stl(solids: &[(&str, &[Triangle])], newline: &str) -> Vec<u8> {
    let mut out = String::new();
    for (name, triangles) in solids {
        out.push_str(&format!("solid {name}{newline}"));
        for triangle in *triangles {
            push_ascii_triangle(&mut out, triangle, newline);
        }
        out.push_str(&format!("endsolid {name}{newline}"));
    }
    out.into_bytes()
}

fn push_ascii_triangle(out: &mut String, triangle: &Triangle, newline: &str) {
    let [nx, ny, nz] = face_normal(triangle);
    out.push_str(&format!("  facet normal {nx:e} {ny:e} {nz:e}{newline}"));
    out.push_str(&format!("    outer loop{newline}"));
    for [x, y, z] in triangle {
        // `{:?}` は f32 を読み戻すと同じビットになる最短の表記。
        out.push_str(&format!("      vertex {x:?} {y:?} {z:?}{newline}"));
    }
    out.push_str(&format!("    endloop{newline}"));
    out.push_str(&format!("  endfacet{newline}"));
}

/// 見本の STL（20 × 30 × 40 mm の箱。バイナリ）。`assets/samples/review-sample.stl` と同じバイト列。
pub fn stl_sample() -> Vec<u8> {
    binary_stl(SAMPLE_HEADER, &box_triangles(STL_SAMPLE_SIZE))
}

/// 重い STL の三角形を順に作る。横 `STRESS_COLUMNS` 列の格子を波打たせた面で、頂点をまとめると
/// 三角形の数のおよそ半分になる。同じ数からは同じ三角形の列になる。
pub fn stress_triangles(triangles: u32) -> impl Iterator<Item = Triangle> {
    const STRESS_COLUMNS: u32 = 1000;
    let height = |column: u32, row: u32| ((column % 17) as f32 * 0.25) + ((row % 13) as f32 * 0.5);
    let point = move |column: u32, row: u32| [column as f32, row as f32, height(column, row)];
    (0..triangles).map(move |index| {
        let quad = index / 2;
        let column = quad % STRESS_COLUMNS;
        let row = quad / STRESS_COLUMNS;
        let a = point(column, row);
        let b = point(column + 1, row);
        let c = point(column + 1, row + 1);
        let d = point(column, row + 1);
        if index % 2 == 0 { [a, b, c] } else { [a, c, d] }
    })
}

/// 重い STL を作る。`ascii` が偽ならバイナリ。
pub fn stl_stress(triangles: u32, ascii: bool) -> Vec<u8> {
    if ascii {
        let mut out = String::new();
        out.push_str("solid stress\n");
        for triangle in stress_triangles(triangles) {
            push_ascii_triangle(&mut out, &triangle, "\n");
        }
        out.push_str("endsolid stress\n");
        out.into_bytes()
    } else {
        let mut out = Vec::with_capacity(84 + 50 * triangles as usize);
        let mut head = [0u8; 80];
        let text = b"model-reviewer stress";
        head[..text.len()].copy_from_slice(text);
        out.extend_from_slice(&head);
        out.extend_from_slice(&triangles.to_le_bytes());
        for triangle in stress_triangles(triangles) {
            push_binary_triangle(&mut out, &triangle);
        }
        out
    }
}

/// 壊れた STL と、間違えやすい正しい STL。
pub mod stl_inputs {
    use super::{Triangle, ascii_stl, binary_stl, binary_stl_with_count, box_triangles};

    fn one() -> Vec<Triangle> {
        vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]
    }

    /// 先頭が `solid` で、ヘッダを空白で詰めたバイナリ（実在する書き方）。
    pub fn binary_starting_with_solid() -> Vec<u8> {
        let mut header = b"solid box".to_vec();
        header.resize(80, b' ');
        binary_stl(&header, &box_triangles([1.0, 2.0, 3.0]))
    }

    /// 三角形の数に `u32::MAX` を書いた84バイト。
    pub fn huge_count() -> Vec<u8> {
        binary_stl_with_count(b"huge", &[], u32::MAX)
    }

    /// 三角形の数が実際より1つ多いバイナリ。
    pub fn count_one_too_many() -> Vec<u8> {
        binary_stl_with_count(b"short", &box_triangles([1.0, 1.0, 1.0]), 13)
    }

    /// 三角形が0個のバイナリ。
    pub fn binary_without_triangles() -> Vec<u8> {
        binary_stl(b"empty", &[])
    }

    /// 末尾に余りのバイトがあるバイナリ。
    pub fn binary_with_trailing(extra: usize) -> Vec<u8> {
        let mut bytes = binary_stl(b"trailing", &box_triangles([1.0, 1.0, 1.0]));
        bytes.extend(std::iter::repeat_n(0xAB, extra));
        bytes
    }

    /// 2番目の三角形の座標に `value` を入れたバイナリ。
    pub fn binary_with_coordinate(value: f32) -> Vec<u8> {
        let mut triangles = box_triangles([1.0, 1.0, 1.0]);
        triangles[1][2][1] = value;
        binary_stl(b"bad number", &triangles)
    }

    /// 2番目の三角形の座標の語を `word` にした ASCII。
    pub fn ascii_with_word(word: &str) -> Vec<u8> {
        let triangles = box_triangles([1.0, 1.0, 1.0]);
        let text = String::from_utf8(ascii_stl(&[("bad", &triangles)], "\n")).unwrap_or_default();
        // 2番目の三角形の最初の `vertex` 行の y を置き換える。
        let mut seen = 0;
        let mut out = String::new();
        for line in text.split_inclusive('\n') {
            if line.trim_start().starts_with("vertex") {
                seen += 1;
                if seen == 4 {
                    let mut words: Vec<&str> = line.split_whitespace().collect();
                    words[2] = word;
                    out.push_str(&format!("      {}\n", words.join(" ")));
                    continue;
                }
            }
            out.push_str(line);
        }
        out.into_bytes()
    }

    /// 正しい ASCII の途中 `keep` バイトで切ったもの。
    pub fn truncated_ascii(keep: usize) -> Vec<u8> {
        let mut bytes = ascii_stl(&[("cut", &box_triangles([1.0, 1.0, 1.0]))], "\n");
        bytes.truncate(keep.min(bytes.len()));
        bytes
    }

    /// `endsolid` の無い ASCII（最後の面までは正しい）。
    pub fn ascii_without_endsolid() -> Vec<u8> {
        let mut bytes = ascii_stl(&[("open", &one())], "\n");
        let text = b"endsolid open\n";
        bytes.truncate(bytes.len() - text.len());
        bytes
    }

    /// `outer loop` の中の `vertex` が `count` 個の ASCII。
    pub fn ascii_with_vertices(count: usize) -> Vec<u8> {
        let mut out = String::from("solid v\nfacet normal 0 0 1\nouter loop\n");
        for index in 0..count {
            out.push_str(&format!("vertex {index}.0 0.0 0.0\n"));
        }
        out.push_str("endloop\nendfacet\nendsolid v\n");
        out.into_bytes()
    }

    /// 面が1つも無い ASCII。
    pub fn ascii_without_triangles() -> Vec<u8> {
        b"solid nothing\nendsolid nothing\n".to_vec()
    }

    /// `solid` を2つ持つ ASCII。
    pub fn ascii_two_solids(newline: &str) -> Vec<u8> {
        let a = box_triangles([1.0, 1.0, 1.0]);
        let b: Vec<Triangle> = box_triangles([1.0, 1.0, 1.0])
            .into_iter()
            .map(|t| t.map(|[x, y, z]| [x + 5.0, y, z]))
            .collect();
        ascii_stl(&[("first", &a), ("second", &b)], newline)
    }
}
