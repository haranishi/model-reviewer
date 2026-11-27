use model_reviewer_gltf::load_glb;
use model_reviewer_testdata::sample_glb;

#[test]
fn fixed_mutations_and_truncations_do_not_panic() {
    let original = sample_glb();
    let mut state = 0xC0FFEE_u64;
    for _ in 0..500 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let mut bytes = original.clone();
        let index = (state as usize) % bytes.len();
        let flip = ((state >> 33) as u8) | 1;
        bytes[index] ^= flip;
        let _ = load_glb(&bytes);
    }
    let len = original.len();
    let mut cuts = vec![0, 1, 4, 8, 12, len / 2];
    if len > 0 {
        cuts.push(len - 1);
    }
    for cut in cuts {
        let _ = load_glb(&original[..cut]);
    }
}
