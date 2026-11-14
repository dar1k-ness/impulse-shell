//! Celebi quantizer exactly as `materialyoucolor.quantize.QuantizeCelebi`.

unsafe extern "C" {
    fn colorgen_quantize_celebi(
        rgb: *const u32,
        n: usize,
        max_colors: i32,
        out_argb: *mut u32,
        out_count: *mut u32,
        cap: usize,
    ) -> usize;
}

/// `pixels` are `[r, g, b, ..]` slices; alpha is ignored like the python binding does.
/// Returns `(argb, population)` sorted by argb (the C++ `std::map` order python sees).
pub fn celebi<'a>(pixels: impl IntoIterator<Item = &'a [u8]>, max_colors: i32) -> Vec<(u32, u32)> {
    let rgb: Vec<u32> = pixels
        .into_iter()
        .map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
        .collect();
    let cap = 256;
    let mut argb = vec![0u32; cap];
    let mut count = vec![0u32; cap];
    // SAFETY: `rgb` holds `rgb.len()` readable values and `argb`/`count` `cap` writable ones;
    // celebi_ffi.cc reads only `n` inputs, writes at most `cap` outputs and keeps no pointer.
    let n = unsafe {
        colorgen_quantize_celebi(
            rgb.as_ptr(),
            rgb.len(),
            max_colors,
            argb.as_mut_ptr(),
            count.as_mut_ptr(),
            cap,
        )
    };
    argb.into_iter().zip(count).take(n.min(cap)).collect()
}
