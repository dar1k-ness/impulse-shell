//! The product of two bytes that stand for 0..=1, `a * b / 255`, in the two roundings the
//! shell has to repeat. Which one a place takes follows who computes that step in Qt:
//!
//! - `nearest`: the video card, when it blends (Qt Quick's scene graph), and QImage's
//!   conversion of CMYK;
//! - `qt`: Qt on the processor: `qPremultiply`, QPainter's blending and composition.
//!
//! They differ by one for 24 of the 65536 pairs, 253 * 191 among them (190 and 189).

/// `a * b / 255` rounded to nearest. It is what llvmpipe blends with (Mesa 26.2.6,
/// src/gallium/auxiliary/gallivm/lp_bld_arit.c:794, `lp_build_mul_norm`: the half is added
/// to the product, lines 836-849): Qt Quick through OpenGL gives this for all 65536 pairs
/// (tests/blend_qt.rs in the shell). QImage turns CMYK into RGB with the same number
/// (tests/formats_qt.rs in qsg).
pub const fn nearest(a: u32, b: u32) -> u32 {
    let t = a * b + 0x80;
    (t + (t >> 8)) >> 8
}

/// Qt's `qt_div_255` (qdrawhelper_p.h:886): `x / 255` with the half added after the shifted
/// number, which is one less than the nearest for some `x`.
pub const fn qt_div_255(x: i64) -> i64 {
    (x + (x >> 8) + 0x80) >> 8
}

/// What Qt does on the processor: `qt_div_255(a * b)`, as in `qPremultiply` and BYTE_MUL of
/// QPainter's blending. One less than `nearest` for 24 of the 65536 pairs.
pub const fn qt(a: u32, b: u32) -> u32 {
    qt_div_255((a * b) as i64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All 65536 pairs: llvmpipe's steps are `a * b / 255` rounded to nearest, and Qt's
    /// `qt_div_255` is one less in exactly 24.
    #[test]
    fn the_two_products_of_bytes_differ_in_24_pairs() {
        let mut less = Vec::new();
        for a in 0..256u32 {
            for b in 0..256u32 {
                // lp_build_mul_norm, lp_bld_arit.c:836-849 of Mesa 26.2.6, for n = 8.
                let mut ab = a * b;
                ab += 1 << 7;
                ab += ab >> 8;
                ab >>= 8;
                assert_eq!(nearest(a, b), ab, "{a} * {b}");
                assert_eq!(ab, (2 * a * b + 255) / 510, "{a} * {b}");
                // What qsg's image.rs had for CMYK before it took `nearest`.
                assert_eq!(ab, (a * b + 127) / 255, "{a} * {b}");
                let t = a * b;
                assert_eq!(qt(a, b), (t + (t >> 8) + 0x80) >> 8, "{a} * {b}");
                match ab - qt(a, b) {
                    0 => {}
                    1 => less.push((a, b)),
                    d => panic!("{a} * {b}: Qt's is {d} less"),
                }
            }
        }
        assert_eq!(less.len(), 24, "{less:?}");
        assert!(less.contains(&(253, 191)));
        assert_eq!((nearest(253, 191), qt(253, 191)), (190, 189));
    }
}
