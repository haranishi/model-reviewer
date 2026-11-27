//! 壊れた STL。決まった `StlError` になり、落ちず、申告の数で確保しないこと。

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use model_reviewer_core::units::UnitLabel;
use model_reviewer_stl::{StlError, StlOptions, detect, load_stl};
use model_reviewer_testdata::{stl_inputs, stl_sample};

// ---- 確保の見張り（この試験の実行ファイルだけ）----

/// 試験のスレッドごとに、1回で確保した最大のバイト数を数える。
struct Watch;

thread_local! {
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Watch {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: 受け取った `layout` をそのまま標準の確保へ渡す。
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: 同上。
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        // SAFETY: 呼び出し側の約束（`ptr` は `layout` で確保した物）をそのまま渡す。
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: 同上。
        unsafe { System.dealloc(ptr, layout) }
    }
}

fn note(size: usize) {
    // スレッドの終わりで値が無くなっていても、確保は続けられるようにする。
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
}

#[global_allocator]
static WATCH: Watch = Watch;

/// `run` の中で1回に確保した最大のバイト数。
fn largest_allocation<T>(run: impl FnOnce() -> T) -> (T, usize) {
    LARGEST.with(|largest| largest.set(0));
    let value = run();
    (value, LARGEST.with(Cell::get))
}

// ---- 試験 ----

fn options(allow_trailing: bool) -> StlOptions<'static> {
    StlOptions {
        name: "壊れた",
        unit: UnitLabel::Mm,
        up: Default::default(),
        allow_trailing,
    }
}

fn load(bytes: &[u8]) -> Result<(), StlError> {
    load_stl(bytes, &options(true)).map(|_| ())
}

#[test]
fn short_inputs_are_too_short() {
    for len in [0usize, 1, 83] {
        let bytes = vec![0u8; len];
        assert_eq!(detect(&bytes, true), None, "{len}");
        assert_eq!(load(&bytes), Err(StlError::TooShort), "{len}");
    }
}

#[test]
fn claimed_count_never_drives_allocation() {
    let bytes = stl_inputs::huge_count();
    assert_eq!(bytes.len(), 84);
    assert_eq!(detect(&bytes, true), None);
    let (result, largest) = largest_allocation(|| load(&bytes));
    assert_eq!(
        result,
        Err(StlError::CountMismatch {
            claimed: u64::from(u32::MAX),
            fits: 0,
        })
    );
    assert!(largest < 4096, "{largest} バイトを確保した");
    assert_eq!(
        load(&stl_inputs::count_one_too_many()),
        Err(StlError::CountMismatch {
            claimed: 13,
            fits: 12,
        })
    );
}

#[test]
fn allocation_follows_the_file_size() {
    // 正しいファイルでも、確保はファイルの大きさに比例する量まで。
    let bytes = stl_sample();
    let (result, largest) = largest_allocation(|| load(&bytes));
    assert!(result.is_ok());
    assert!(largest <= 4 * bytes.len(), "{largest} / {}", bytes.len());
}

#[test]
fn non_finite_and_huge_coordinates_are_rejected() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 2.0e18, -5.0e30] {
        assert_eq!(
            load(&stl_inputs::binary_with_coordinate(value)),
            Err(StlError::BadNumber { triangle: 2 }),
            "{value}"
        );
    }
    // 上限より十分小さい値は読める。
    assert!(load(&stl_inputs::binary_with_coordinate(1.0e17)).is_ok());
    // `f32` の文字列の読み取りが成功として返す、有限でない値と大きすぎる値。
    for word in [
        "nan", "NaN", "inf", "-inf", "infinity", "1e39", "-1e999", "2e18",
    ] {
        assert_eq!(
            load(&stl_inputs::ascii_with_word(word)),
            Err(StlError::BadNumber { triangle: 2 }),
            "{word}"
        );
    }
    assert!(load(&stl_inputs::ascii_with_word("1e17")).is_ok());
}

#[test]
fn unreadable_words_are_syntax_errors() {
    // 2番目の三角形の最初の頂点は 11 行目（solid の1行と、1つ目の面の7行と、facet・outer loop の次）。
    for word in ["abc", "1.0.0", "0x10", ""] {
        let bytes = stl_inputs::ascii_with_word(word);
        let result = load(&bytes);
        assert!(
            matches!(result, Err(StlError::Syntax { line: 11 })),
            "{word:?} {result:?}"
        );
    }
}

#[test]
fn truncated_ascii_is_an_error() {
    let full = stl_inputs::truncated_ascii(usize::MAX);
    assert!(load(&full).is_ok());
    let last_line = full[..full.len() - 1]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |at| at + 1);
    for keep in 0..last_line {
        let result = load(&stl_inputs::truncated_ascii(keep));
        if keep >= 5 {
            // `solid` の5文字があれば ASCII として読み、途中で終わったと分かる。
            assert_eq!(result, Err(StlError::Truncated), "{keep}");
        } else {
            assert_eq!(result, Err(StlError::TooShort), "{keep}");
        }
    }
    assert_eq!(
        load(&stl_inputs::ascii_without_endsolid()),
        Err(StlError::Truncated)
    );
}

#[test]
fn a_loop_needs_exactly_three_vertices() {
    for count in [0usize, 1, 2, 4] {
        let result = load(&stl_inputs::ascii_with_vertices(count));
        assert!(
            matches!(result, Err(StlError::Syntax { .. })),
            "{count} {result:?}"
        );
    }
    assert!(load(&stl_inputs::ascii_with_vertices(3)).is_ok());
}

#[test]
fn files_without_triangles_are_rejected() {
    assert_eq!(
        load(&stl_inputs::binary_without_triangles()),
        Err(StlError::NoTriangles)
    );
    assert_eq!(
        load(&stl_inputs::ascii_without_triangles()),
        Err(StlError::NoTriangles)
    );
}

#[test]
fn trailing_bytes_are_refused_unless_allowed() {
    let bytes = stl_inputs::binary_with_trailing(3);
    assert_eq!(
        load_stl(&bytes, &options(false)).map(|_| ()),
        Err(StlError::NotStl)
    );
    assert!(load(&bytes).is_ok());
}

#[test]
fn every_error_explains_itself_in_japanese() {
    let inputs = [
        vec![0u8; 10],
        stl_inputs::huge_count(),
        stl_inputs::binary_with_coordinate(f32::NAN),
        stl_inputs::ascii_with_word("abc"),
        stl_inputs::truncated_ascii(60),
        stl_inputs::binary_without_triangles(),
    ];
    for bytes in inputs {
        let error = load(&bytes).unwrap_err();
        let message = error.message_ja();
        assert!(!message.is_empty());
        assert!(!message.is_ascii(), "{message}");
    }
}

/// 決まった種の線形合同法（試験の再現のため）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Lcg(0x5EED_0005_7100_0001);
    let sample = stl_sample();
    let ascii = stl_inputs::ascii_two_solids("\r\n");
    for round in 0..10_000u32 {
        let bytes: Vec<u8> = match round % 4 {
            // まったくのでたらめ。
            0 => {
                let len = rng.below(400) as usize;
                (0..len).map(|_| rng.next() as u8).collect()
            }
            // `solid` で始まるでたらめ。
            1 => {
                let len = rng.below(300) as usize;
                let mut bytes = b"solid x\n".to_vec();
                bytes.extend((0..len).map(|_| {
                    b" \n\rfacetnormalouterloopvertex0123456789.-e"[rng.below(42) as usize]
                }));
                bytes
            }
            // 正しいバイナリの一部を壊したもの。
            2 => {
                let mut bytes = sample.clone();
                for _ in 0..=rng.below(8) {
                    let at = rng.below(bytes.len() as u32) as usize;
                    bytes[at] = rng.next() as u8;
                }
                let keep = rng.below(bytes.len() as u32 + 1) as usize;
                bytes.truncate(keep.max(rng.below(2) as usize * bytes.len()));
                bytes
            }
            // 正しい ASCII の一部を壊したもの。
            _ => {
                let mut bytes = ascii.clone();
                for _ in 0..=rng.below(8) {
                    let at = rng.below(bytes.len() as u32) as usize;
                    bytes[at] = rng.next() as u8;
                }
                bytes
            }
        };
        for allow_trailing in [false, true] {
            let _ = detect(&bytes, allow_trailing);
            if let Ok(model) = load_stl(&bytes, &options(allow_trailing)) {
                let prim = &model.primitives[0];
                assert!(!prim.indices.is_empty());
                assert!(
                    prim.indices
                        .iter()
                        .all(|&index| (index as usize) < prim.vertices.len())
                );
                assert!(model.bounds.min.is_finite() && model.bounds.max.is_finite());
            }
        }
    }
}
