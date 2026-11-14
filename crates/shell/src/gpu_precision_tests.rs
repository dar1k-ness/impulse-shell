//! GPU precision tests on any GL (llvmpipe in CI): `emulate` rewrites each fragment shader so
//! its mediump/lowp inputs (gl_FragCoord, which GLSL ES 1.00 declares mediump, and float
//! varyings/uniforms without highp) carry fp16 values, as on GPUs that run mediump at half
//! precision (AMD GCN like the RX 580, most mobile GPUs). llvmpipe computes everything at fp32,
//! so without this a shader that only breaks at fp16 passes here.
//!
//! RI_REQUIRE_GL=1 (CI) fails instead of skipping when no GL context can be made.

use super::*;
use std::borrow::Cow;
use std::cell::Cell;

// 0 off; 1 mediump is fp16 and highp exists; 2 mediump is fp16 and there is no highp.
thread_local!(static FP16: Cell<u8> = const { Cell::new(0) });

/// Shaders compiled while this is alive (on this thread) get fp16 mediump inputs.
struct Fp16;

impl Fp16 {
    /// A GPU whose fragment shaders have highp (GL_FRAGMENT_PRECISION_HIGH) and run mediump
    /// at half precision: the RX 580.
    fn on() -> Fp16 {
        FP16.set(1);
        Fp16
    }

    /// A GPU without highp in fragment shaders (Mali-400, Adreno 2xx): FS_PRECISION's `HP`
    /// is mediump there, so every float input of a fragment shader is fp16.
    fn no_highp() -> Fp16 {
        FP16.set(2);
        Fp16
    }
}

impl Drop for Fp16 {
    fn drop(&mut self) {
        FP16.set(0);
    }
}

/// Round-to-nearest-even into IEEE binary16 (subnormals kept, overflow clamped to 65504),
/// computed at highp.
const QUANTIZE: &str = "
highp float _fp16_rne(highp float r) { highp float f = floor(r); highp float d = r - f;
 if (d > 0.5) return f + 1.0; if (d < 0.5) return f; return f + mod(f, 2.0); }
highp float _fp16(highp float x) { highp float a = abs(x);
 if (a < 6.103515625e-05) return sign(x) * _fp16_rne(a / 5.9604644775390625e-08) * 5.9604644775390625e-08;
 highp float e = floor(log2(a)); if (exp2(e) > a) e -= 1.0; if (exp2(e + 1.0) <= a) e += 1.0;
 highp float st = exp2(e - 10.0); return sign(x) * min(_fp16_rne(a / st) * st, 65504.0); }
highp vec2 _fp16(highp vec2 v) { return vec2(_fp16(v.x), _fp16(v.y)); }
highp vec3 _fp16(highp vec3 v) { return vec3(_fp16(v.x), _fp16(v.y), _fp16(v.z)); }
highp vec4 _fp16(highp vec4 v) { return vec4(_fp16(v.x), _fp16(v.y), _fp16(v.z), _fp16(v.w)); }
";

/// `fs` with its reduced-precision float inputs quantized to fp16 at the start of `main`, when
/// `Fp16` is on; unchanged otherwise. Every other use of such an input reads a highp global
/// holding the quantized value. Arithmetic stays at the driver's precision: the inputs are where
/// the clock bug's precision was lost (pixel coordinates past 1024 and 1/2048 steps in [0, 1]).
/// Under `Fp16::no_highp` the default precision and `HP` are mediump, as FS_PRECISION makes
/// them on such a GPU.
pub(super) fn emulate(fs: &str) -> Cow<'_, str> {
    let hp = match FP16.get() {
        0 => return Cow::Borrowed(fs),
        1 => "highp",
        _ => "mediump",
    };
    let main = fs.find("void main").expect("fragment shader without main");
    let mut default = hp;
    let mut decls = Vec::new();
    // (type, name, array length)
    let mut inputs = vec![("vec4", "gl_FragCoord".to_string(), None)];
    let mut at = 0;
    for stmt in fs[..main].split_inclusive(';') {
        let span = at..at + stmt.len();
        at = span.end;
        let w: Vec<&str> = stmt.trim_end_matches(';').split_whitespace().collect();
        match w.as_slice() {
            ["precision", p, "float"] => default = p,
            ["varying" | "uniform", rest @ ..] if !rest.is_empty() => {
                let (prec, rest) = match rest[0] {
                    "HP" => (hp, &rest[1..]),
                    p @ ("lowp" | "mediump" | "highp") => (p, &rest[1..]),
                    _ => (default, rest),
                };
                let ty = rest[0];
                if prec == "highp" || !matches!(ty, "float" | "vec2" | "vec3" | "vec4") {
                    continue;
                }
                decls.push(span);
                let names = rest[1..].join(" ");
                for n in names.split(',').map(str::trim) {
                    match n.split_once('[') {
                        Some((n, len)) => {
                            let len: usize =
                                len.trim_end_matches(']').parse().expect("array length");
                            inputs.push((ty, n.to_string(), Some(len)));
                        }
                        None => inputs.push((ty, n.to_string(), None)),
                    }
                }
            }
            _ => {}
        }
    }
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::new();
    let mut i = 0;
    while i < fs.len() {
        if let Some(d) = decls.iter().find(|d| d.start == i) {
            out.push_str(&fs[d.clone()]);
            i = d.end;
            continue;
        }
        let c = fs[i..].chars().next().unwrap();
        let prev = fs[..i].chars().next_back();
        if ident(c) && !prev.is_some_and(|p| ident(p) || p == '.') {
            let end = fs[i..].find(|c| !ident(c)).map_or(fs.len(), |e| i + e);
            let w = &fs[i..end];
            if inputs.iter().any(|(_, n, _)| n == w) {
                out.push_str("_mp_");
            }
            out.push_str(w);
            i = end;
        } else {
            out.push(c);
            i += c.len_utf8();
        }
    }
    let globals: String = inputs
        .iter()
        .map(|(ty, n, len)| match len {
            Some(len) => format!("highp {ty} _mp_{n}[{len}];\n"),
            None => format!("highp {ty} _mp_{n};\n"),
        })
        .collect();
    let init: String = inputs
        .iter()
        .map(|(_, n, len)| match len {
            Some(len) => format!(
                " for (int _mp_i = 0; _mp_i < {len}; _mp_i++) _mp_{n}[_mp_i] = _fp16({n}[_mp_i]);"
            ),
            None => format!(" _mp_{n} = _fp16({n});"),
        })
        .collect();
    let main = out.find("void main").unwrap();
    let body = main + out[main..].find('{').unwrap() + 1;
    out.insert_str(body, &init);
    out.insert_str(0, &format!("{QUANTIZE}{globals}"));
    Cow::Owned(out)
}

fn gpu() -> Option<Gpu> {
    let g = Gpu::headless("surfaceless").map(|g| g.0).ok();
    match &g {
        Some(g) => elog::eprintln!("precision test on {}", g.renderer),
        None if std::env::var_os("RI_REQUIRE_GL").is_some() => {
            panic!("no surfaceless EGL/GLES2 context (RI_REQUIRE_GL is set)")
        }
        None => elog::eprintln!("precision test skipped: no surfaceless EGL/GLES2 context"),
    }
    g
}

/// QHD width and a height past 1024: fp16 steps are 1 px in [1024, 2048) and 2 px above.
const W: i32 = 2560;
const H: i32 = 1100;

/// Every texel distinct: x in red and the low green bits, y in blue and the high green bits.
fn pattern() -> Vec<u8> {
    let mut px = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            px.extend_from_slice(&[x as u8, ((x >> 8) | ((y >> 8) << 4)) as u8, y as u8, 255]);
        }
    }
    px
}

unsafe fn upload(g: &Gpu, px: &[u8]) -> glow::Texture {
    // SAFETY: `g`'s context is current, as this fn requires.
    let (t, ..) = unsafe { g.target(W, H) }.expect("target");
    // SAFETY: `g`'s context is current, as this fn requires.
    unsafe { g.gl.bind_texture(glow::TEXTURE_2D, Some(t)) };
    // SAFETY: `g`'s context is current, as this fn requires.
    unsafe {
        g.gl.tex_sub_image_2d(
            glow::TEXTURE_2D,
            0,
            0,
            0,
            W,
            H,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(px)),
        )
    };
    t
}

/// A cleared W x H target, bound for drawing.
unsafe fn canvas(g: &Gpu) -> glow::Framebuffer {
    // SAFETY: `g`'s context is current, as this fn requires.
    let (_, fb, ..) = unsafe { g.target(W, H) }.expect("target");
    // SAFETY: `g`'s context is current, as this fn requires.
    unsafe { g.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb)) };
    // SAFETY: `g`'s context is current, as this fn requires.
    unsafe { g.gl.viewport(0, 0, W, H) };
    // SAFETY: `g`'s context is current, as this fn requires.
    unsafe { g.gl.disable(glow::SCISSOR_TEST) };
    // SAFETY: `g`'s context is current, as this fn requires.
    unsafe { g.gl.clear_color(0.0, 0.0, 0.0, 0.0) };
    // SAFETY: `g`'s context is current, as this fn requires.
    unsafe { g.gl.clear(glow::COLOR_BUFFER_BIT) };
    fb
}

unsafe fn read(g: &Gpu, fb: glow::Framebuffer) -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    // SAFETY: `g`'s context is current, as this fn requires.
    unsafe { g.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb)) };
    // SAFETY: `g`'s context is current, as this fn requires.
    unsafe {
        g.gl.read_pixels(
            0,
            0,
            W,
            H,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut px)),
        )
    };
    px
}

/// The first pixel that differs, as "(x, y): got [..] want [..]", and how many differ.
fn mismatch(got: &[u8], want: &[u8]) -> Option<(String, usize)> {
    let bad: Vec<usize> = (0..got.len() / 4)
        .filter(|&i| got[i * 4..i * 4 + 4] != want[i * 4..i * 4 + 4])
        .collect();
    let &i = bad.first()?;
    let (x, y) = (i as i32 % W, i as i32 / W);
    Some((
        format!(
            "({x}, {y}): got {:?} want {:?}",
            &got[i * 4..i * 4 + 4],
            &want[i * 4..i * 4 + 4]
        ),
        bad.len(),
    ))
}

/// The previous frame through `draw_fade` at full opacity; `shaders` replaces FADE_VS/FADE_FS.
fn fade(g: &mut Gpu, shaders: Option<(&str, &str)>) -> Option<(String, usize)> {
    let want = pattern();
    if let Some((vs, fs)) = shaders {
        // SAFETY: `gpu()` made `g`'s context current on this thread.
        let p = unsafe { program(&g.gl, vs, fs, &["p"]) }.expect("fade program");
        let u = ["a", "o"].map(|n| {
            // SAFETY: `gpu()` made `g`'s context current on this thread.
            unsafe { g.gl.get_uniform_location(p, n) }
        });
        g.fade_prog = Some((p, u));
        // The pre-fix shaders divide by the window size `s`, which `draw_fade` does not set.
        // SAFETY: `gpu()` made `g`'s context current on this thread.
        unsafe { g.use_prog(p) };
        // SAFETY: `gpu()` made `g`'s context current on this thread.
        let s = unsafe { g.gl.get_uniform_location(p, "s") };
        // SAFETY: `gpu()` made `g`'s context current on this thread.
        unsafe { g.gl.uniform_2_f32(s.as_ref(), W as f32, H as f32) };
    }
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    let src = unsafe { upload(g, &want) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    let fb = unsafe { canvas(g) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { g.draw_fade(src, W, H, 1.0, None) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    mismatch(&unsafe { read(g, fb) }, &want)
}

/// An image drawn 1:1 through the textured-quad program (`fs` replaces TEX_FS).
fn image(g: &mut Gpu, fs: Option<&str>) -> Option<(String, usize)> {
    let want = pattern();
    let p = match fs {
        // SAFETY: `gpu()` made `g`'s context current on this thread.
        Some(fs) => unsafe { program(&g.gl, TEX_VS, fs, &["p", "t"]) }.expect("tex program"),
        None => g.tex,
    };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    let src = unsafe { upload(g, &want) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    let fb = unsafe { canvas(g) };
    let gl = &g.gl;
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.use_program(Some(p)) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.disable(glow::BLEND) };
    let u = |n| {
        // SAFETY: `gpu()` made `g`'s context current on this thread.
        unsafe { gl.get_uniform_location(p, n) }
    };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.uniform_2_f32(u("s").as_ref(), W as f32, H as f32) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.uniform_1_f32(u("m").as_ref(), 0.0) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.uniform_1_f32(u("o").as_ref(), 1.0) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.uniform_4_f32(u("k").as_ref(), 0.0, 0.0, 0.0, 0.0) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.uniform_1_i32(u("x").as_ref(), 0) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.uniform_1_i32(u("y").as_ref(), 1) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.active_texture(glow::TEXTURE0) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.bind_texture(glow::TEXTURE_2D, Some(src)) };
    // y-down scene coordinates: the top row samples the texture's last row, so the read-back
    // (bottom row first) lines up with the upload.
    let mut v = Vec::new();
    quad(&mut v, 0.0, 0.0, W as f32, H as f32, [0.0, 1.0, 1.0, -1.0]);
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.bind_buffer(glow::ARRAY_BUFFER, Some(g.vbo)) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    let off = unsafe { g.stream(as_bytes(&v)) };
    let gl = &g.gl;
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.enable_vertex_attrib_array(0) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.enable_vertex_attrib_array(1) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, off) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, off + 8) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    unsafe { gl.draw_arrays(glow::TRIANGLES, 0, 6) };
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    mismatch(&unsafe { read(g, fb) }, &want)
}

/// `frame_compare`'s program built with `fs` instead of DIFF_FS.
fn diff_program(g: &mut Gpu, fs: &str) {
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    let p = unsafe { program(&g.gl, DIFF_VS, fs, &["p"]) }.expect("diff program");
    let u = ["a", "b", "s", "m"].map(|n| {
        // SAFETY: `gpu()` made `g`'s context current on this thread.
        unsafe { g.gl.get_uniform_location(p, n) }
    });
    g.diff = Some((p, u));
}

/// The copy `frame_compare` keeps of a frame, against the frame; `fs` replaces DIFF_FS.
fn frame_copy(g: &mut Gpu, fs: Option<&str>) -> Option<(String, usize)> {
    let want = pattern();
    if let Some(fs) = fs {
        diff_program(g, fs);
    }
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    let src = unsafe { upload(g, &want) };
    let mut copy = None;
    assert_eq!(g.frame_compare(src, W, H, (W, H), &mut copy), Some(true));
    let fb = copy.as_ref().expect("frame copy").chain[0].1;
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    let got = unsafe { read(g, fb) };
    g.free_frame_copy(copy);
    mismatch(&got, &want)
}

/// Pixels whose column or row the fp16 gl_FragCoord cannot hold: odd ones past 1024. Not the
/// last column and row: a copy that asks for the one after them is given them (CLAMP_TO_EDGE).
const ODD_PAST_1024: [(i32, i32); 5] = [(1025, 3), (2047, 600), (W - 3, 7), (3, 1025), (4, H - 3)];

/// Per pixel of `at`: whether `frame_changed` reports a frame changed in that pixel alone, by
/// one step of one channel. `fs` replaces DIFF_FS.
fn sees_changes(g: &mut Gpu, fs: Option<&str>, at: &[(i32, i32)]) -> Vec<bool> {
    sees_changes_in(g, fs, (W, H), 1, at)
}

/// `sees_changes` for a preview of `view` pixels (bilinear, so a texel weighs less than one
/// in it) and a change of `step` in the channel.
fn sees_changes_in(
    g: &mut Gpu,
    fs: Option<&str>,
    view: (i32, i32),
    step: u8,
    at: &[(i32, i32)],
) -> Vec<bool> {
    let mut px = pattern();
    if let Some(fs) = fs {
        diff_program(g, fs);
    }
    // SAFETY: `gpu()` made `g`'s context current on this thread.
    let src = unsafe { upload(g, &px) };
    at.iter()
        .map(|&(x, y)| {
            let mut copy = None;
            assert!(g.frame_changed(src, W, H, view, &mut copy), "first frame");
            assert!(!g.frame_changed(src, W, H, view, &mut copy), "same frame");
            let i = ((y * W + x) * 4) as usize;
            px[i] ^= step;
            // SAFETY: `gpu()` made `g`'s context current on this thread.
            unsafe { g.gl.bind_texture(glow::TEXTURE_2D, Some(src)) };
            // SAFETY: `gpu()` made `g`'s context current on this thread.
            unsafe {
                g.gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    x,
                    y,
                    1,
                    1,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&px[i..i + 4])),
                )
            };
            let seen = g.frame_changed(src, W, H, view, &mut copy);
            g.free_frame_copy(copy);
            seen
        })
        .collect()
}

/// FADE_FS before the fixes: mediump `gl_FragCoord / s`, then highp math on the still-mediump
/// gl_FragCoord. Both garbled the bar's clock and tray past x = 1024 during a theme fade on the
/// owner's RX 580 and both were exact on llvmpipe.
/// Their sampler is lowp, the default they had before FS_PRECISION named one.
const FADE_FS_MEDIUMP: &str =
    "precision mediump float; uniform lowp sampler2D a; uniform vec2 s; uniform float o;
void main() { gl_FragColor = texture2D(a, gl_FragCoord.xy / s) * o; }";
const FADE_FS_FRAGCOORD: &str =
    "precision highp float; uniform lowp sampler2D a; uniform vec2 s; uniform float o;
void main() { gl_FragColor = texture2D(a, (floor(gl_FragCoord.xy) + 0.5) / s) * o; }";
/// DIFF_FS before the fix: the target pixel from gl_FragCoord. Its copy of a frame left out the
/// odd columns and rows past 1024 at fp16, so a frame that changed only there compared unchanged.
const DIFF_FS_FRAGCOORD: &str = "uniform sampler2D a; uniform sampler2D b;
uniform vec2 s; uniform float m;
void main() { vec2 f = floor(gl_FragCoord.xy);
 if (m > 1.5) { gl_FragColor = texture2D(a, (f + 0.5) / s); return; }
 float d = 0.0;
 for (int i = 0; i < 8; i++) for (int j = 0; j < 8; j++) {
  vec2 q = f * 8.0 + vec2(float(i), float(j));
  if (q.x < s.x && q.y < s.y) { vec2 c = (q + 0.5) / s; vec4 x = texture2D(a, c);
   if (m > 0.5) { vec4 e = abs(x - texture2D(b, c)); d = max(d, max(max(e.r, e.g), max(e.b, e.a))); }
   else d = max(d, x.r); } }
 gl_FragColor = vec4(d > 0.001 ? 1.0 : 0.0); }";
/// TEX_FS before it moved to highp: its mediump `u` can't address single texels of a texture
/// wider than 2048 (fp16 steps of 1/2048 in [0.5, 1)).
fn tex_fs_mediump() -> String {
    format!("precision mediump float; {TEX_FS}")
}

#[test]
fn emulation_quantizes_reduced_precision_inputs() {
    let _on = Fp16::on();
    let fs = emulate(
        "precision mediump float; varying vec2 u; varying highp vec2 h; uniform float o;
uniform sampler2D x; void main() { gl_FragColor = texture2D(x, u + h) * o + gl_FragCoord.x; }",
    );
    assert!(
        fs.contains("texture2D(x, _mp_u + h) * _mp_o + _mp_gl_FragCoord.x"),
        "{fs}"
    );
    assert!(
        fs.contains("varying vec2 u;") && fs.contains("_mp_u = _fp16(u);"),
        "{fs}"
    );
    assert!(!fs.contains("_mp_h") && !fs.contains("_mp_x"), "{fs}");
    drop(_on);
    assert!(matches!(emulate(FADE_FS), Cow::Borrowed(_)));
    // Without highp the default precision and `HP` are mediump too; arrays go element by
    // element.
    let src = "uniform vec2 w[3];\nvarying HP vec2 q; uniform sampler2D x;
void main() { gl_FragColor = texture2D(x, q + w[1].x); }";
    {
        let _on = Fp16::on();
        let fs = emulate(src);
        assert!(!fs.contains("_mp_q =") && !fs.contains("_mp_w"), "{fs}");
    }
    let _on = Fp16::no_highp();
    let fs = emulate(src);
    assert!(fs.contains("texture2D(x, _mp_q + _mp_w[1].x)"), "{fs}");
    assert!(
        fs.contains("highp vec2 _mp_w[3];")
            && fs.contains("_mp_i < 3; _mp_i++) _mp_w[_mp_i] = _fp16(w[_mp_i]);"),
        "{fs}"
    );
}

#[test]
fn fade_is_pixel_exact_at_fp16_mediump() {
    let _on = Fp16::on();
    let Some(mut g) = gpu() else { return };
    if let Some((at, n)) = fade(&mut g, None) {
        panic!("FADE_FS at fp16 mediump: {n} pixels differ, first at {at}");
    }
}

/// The emulation is what catches the bug: the pre-fix shaders pass on plain llvmpipe and fail
/// with fp16 mediump inputs.
#[test]
fn fade_before_fix_fails_at_fp16_mediump() {
    for fs in [FADE_FS_MEDIUMP, FADE_FS_FRAGCOORD] {
        let Some(mut g) = gpu() else { return };
        if !g.renderer.contains("llvmpipe") {
            elog::eprintln!("fp32 baseline skipped on {}", g.renderer);
        } else if let Some((at, n)) = fade(&mut g, Some((DIFF_VS, fs))) {
            panic!("pre-fix fade differs even at fp32 ({n} pixels, first at {at}): {fs}");
        }
        let _on = Fp16::on();
        let mut g = gpu().unwrap();
        let (at, n) = fade(&mut g, Some((DIFF_VS, fs))).expect("pre-fix fade passed at fp16");
        elog::eprintln!("pre-fix fade at fp16: {n} pixels differ, first at {at}");
    }
}

#[test]
fn frame_copy_is_pixel_exact_at_fp16_mediump() {
    let _on = Fp16::on();
    let Some(mut g) = gpu() else { return };
    if let Some((at, n)) = frame_copy(&mut g, None) {
        panic!("DIFF_FS copy at fp16 mediump: {n} pixels differ, first at {at}");
    }
}

#[test]
fn frame_compare_sees_odd_columns_and_rows_past_1024_at_fp16_mediump() {
    let _on = Fp16::on();
    let Some(mut g) = gpu() else { return };
    let seen = sees_changes(&mut g, None, &ODD_PAST_1024);
    assert_eq!(seen, [true; 5], "changes at {ODD_PAST_1024:?}");
}

/// The pre-fix shader is exact on plain llvmpipe and, with fp16 mediump inputs, copies a frame
/// wrong and takes the frames changed in the pixels it left out for unchanged ones.
#[test]
fn frame_compare_before_fix_fails_at_fp16_mediump() {
    let fs = DIFF_FS_FRAGCOORD;
    let Some(mut g) = gpu() else { return };
    if !g.renderer.contains("llvmpipe") {
        elog::eprintln!("fp32 baseline skipped on {}", g.renderer);
    } else {
        if let Some((at, n)) = frame_copy(&mut g, Some(fs)) {
            panic!("pre-fix copy differs even at fp32 ({n} pixels, first at {at})");
        }
        let seen = sees_changes(&mut gpu().unwrap(), Some(fs), &ODD_PAST_1024);
        assert_eq!(seen, [true; 5], "pre-fix compare at fp32");
    }
    let _on = Fp16::on();
    let (at, n) = frame_copy(&mut gpu().unwrap(), Some(fs)).expect("pre-fix copy passed at fp16");
    elog::eprintln!("pre-fix copy at fp16: {n} pixels differ, first at {at}");
    let seen = sees_changes(&mut gpu().unwrap(), Some(fs), &ODD_PAST_1024);
    assert_eq!(seen, [false; 5], "pre-fix compare at fp16");
}

/// A preview smaller than the frame (the usual case: a window thumbnail of a screen capture)
/// compares the frame as the preview samples it. Half size puts preview column 1025 on frame
/// columns 2050 and 2051; 2047 x 1099 has odd preview columns and rows past 1024 of its own.
#[test]
fn scaled_frame_compare_sees_changes_past_1024_at_fp16_mediump() {
    let _on = Fp16::on();
    let Some(mut g) = gpu() else { return };
    for (view, at) in [
        ((W / 2, H / 2), HALF_VIEW_PAST_1024),
        ((2047, 1099), ODD_PAST_1024),
    ] {
        let seen = sees_changes_in(&mut g, None, view, 128, &at);
        assert_eq!(seen, [true; 5], "view {view:?}, changes at {at:?}");
    }
}

/// The pre-fix shader sees them at fp32 and, with fp16 mediump inputs, leaves the odd preview
/// columns past 1024 out of both copies: a frame that changed only under them compares equal.
#[test]
fn scaled_frame_compare_before_fix_fails_at_fp16_mediump() {
    let (fs, view) = (DIFF_FS_FRAGCOORD, (W / 2, H / 2));
    let Some(mut g) = gpu() else { return };
    if !g.renderer.contains("llvmpipe") {
        elog::eprintln!("fp32 baseline skipped on {}", g.renderer);
    } else {
        let seen = sees_changes_in(&mut g, Some(fs), view, 128, &HALF_VIEW_PAST_1024);
        assert_eq!(seen, [true; 5], "pre-fix scaled compare at fp32");
    }
    let _on = Fp16::on();
    let seen = sees_changes_in(
        &mut gpu().unwrap(),
        Some(fs),
        view,
        128,
        &HALF_VIEW_PAST_1024,
    );
    assert_eq!(seen, [false; 5], "pre-fix scaled compare at fp16");
}

/// Frame pixels under the odd columns past 1024 of a half-size preview (column 1025 covers
/// frame columns 2050 and 2051).
const HALF_VIEW_PAST_1024: [(i32, i32); 5] =
    [(2050, 3), (2051, 600), (2054, 1099), (2302, 0), (W - 6, 7)];

/// A picture with every texel distinct (`pattern`'s colours) and one whose alpha varies from
/// texel to texel (premultiplied), `w` x `h`.
fn pictures(w: i32, h: i32) -> (Rc<Canvas>, Rc<Canvas>) {
    let (mut pic, mut veil) = (
        Canvas::new(w as usize, h as usize, [0, 0, 0, 255]),
        Canvas::new(w as usize, h as usize, [0, 0, 0, 0]),
    );
    for (i, (p, v)) in pic.px.iter_mut().zip(&mut veil.px).enumerate() {
        let (x, y) = (i % w as usize, i / w as usize);
        *p = [x as u8, ((x >> 8) | ((y >> 8) << 4)) as u8, y as u8, 255];
        *v = [0, 0, 0, (x * 37 + y * 11) as u8 | 1];
    }
    (Rc::new(pic), Rc::new(veil))
}

/// One draw per effect program and mode, over a window 2560 px wide: TEX_FS's overlay,
/// desaturation and mask modes, EFFECT_FS without and with its BLUR_FS levels and with its
/// shadow, the blur shaders with baked taps (GaussianBlur, DropShadow) and with the tap loop
/// (of 241 taps and of the lock screen's 201), RSHADOW_FS.
const SCENES: [&str; 11] = [
    "overlay",
    "desaturate",
    "mask",
    "effect",
    "effect blur",
    "effect shadow",
    "blur",
    "drop shadow",
    "looped blur",
    "lock screen blur",
    "rectangular shadow",
];

/// More taps than BAKED_TAPS: the shader with the loop.
fn looped_kernel() -> Gauss {
    let (taps, spread, sum) = qsg::blur::kernel(120.0, 241, None);
    assert!(taps.len() > BAKED_TAPS);
    Gauss {
        taps,
        spread,
        sum,
        per: [W as f32, 8.0],
        band: [0.0, 1.0],
        shadow: None,
        first: None,
    }
}

/// The lock screen's wallpaper blur (Background.qml: radius 100, 201 samples), also drawn by
/// the loop.
fn lock_kernel() -> Gauss {
    let (taps, spread, sum) = qsg::blur::kernel(100.0, 201, None);
    assert!(taps.len() > BAKED_TAPS);
    Gauss {
        taps,
        spread,
        sum,
        per: [W as f32, 8.0],
        band: [0.0, 1.0],
        shadow: None,
        first: None,
    }
}

fn scene(name: &str) -> Frame {
    // The loop reads 241 or 201 taps per fragment (each quantized under `Fp16::no_highp`): a
    // strip.
    let (w, h) = if name == "looped blur" || name == "lock screen blur" {
        (W, 8)
    } else {
        (W, H)
    };
    let (pic, veil) = pictures(w, h);
    let (wf, hf) = (w as f64, h as f64);
    let rect = [0.0, 0.0, wf, hf];
    let layer = |c: &Rc<Canvas>| {
        let mut f = Frame::new(w as usize, h as usize);
        f.image(c.clone(), rect, [0.0, 0.0, 1.0, 1.0], 1.0, false);
        f
    };
    let gauss = |radius, samples, shadow| {
        let (taps, spread, sum) = qsg::blur::kernel(radius, samples, None);
        Gauss {
            taps,
            spread,
            sum,
            per: [w as f32, h as f32],
            band: [0.0, 1.0],
            shadow,
            first: None,
        }
    };
    let size = (w as usize, h as usize);
    let effect = [1.0, 1.0, 1.0, 0.0, 0.1, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
    let mut fr = Frame::new(w as usize, h as usize);
    match name {
        "overlay" => fr.tint(
            layer(&pic),
            [wf, hf],
            [255, 64, 0, 128].map(|k| k * 257),
            None,
            true,
            rect,
            1.0,
        ),
        "desaturate" => fr.tint(layer(&pic), [wf, hf], [0; 4], Some(0.5), true, rect, 1.0),
        "mask" => fr.masked(layer(&pic), layer(&veil), rect, false, 1.0, 0),
        "effect" => fr.effect(layer(&pic), None, true, rect, effect, 1.0, None, None),
        "effect blur" => {
            let blur = Blur {
                sizes: qsg_blur_levels(size),
                dpr: 1.0,
                multiplier: 1.0,
                weights: [0.2, 0.5, 0.3, 0.0, 0.0, 0.0],
            };
            fr.effect(layer(&pic), None, true, rect, effect, 1.0, Some(blur), None)
        }
        "effect shadow" => {
            // The default blurMax of 32 has three BlurItems.
            let mut levels = qsg_blur_levels(size);
            levels.push((levels[1].0 * 0.5, levels[1].1 * 0.5));
            let p = qsg::multieffect::Params {
                shadow_enabled: true,
                shadow_horizontal_offset: 7.0,
                shadow_vertical_offset: 5.0,
                shadow_scale: 1.1,
                shadow_color: [40, 80, 120, 255].map(|k| k * 257),
                ..Default::default()
            };
            let shadow = qsg::multieffect::shadow(&p, wf, hf);
            assert!(shadow.is_some());
            let blur = Blur {
                sizes: levels,
                dpr: 1.0,
                multiplier: 1.0,
                weights: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            };
            fr.effect(
                layer(&veil),
                None,
                true,
                rect,
                effect,
                1.0,
                Some(blur),
                shadow,
            )
        }
        "blur" => fr.gauss(layer(&pic), gauss(8.0, 17, None), size, rect, None, 1.0),
        "drop shadow" => {
            let shadow = Some(([0.1, 0.2, 0.3, 0.5], 1.0));
            fr.gauss(layer(&veil), gauss(8.0, 17, shadow), size, rect, None, 1.0)
        }
        "looped blur" => fr.gauss(layer(&pic), looped_kernel(), size, rect, None, 1.0),
        "lock screen blur" => fr.gauss(layer(&pic), lock_kernel(), size, rect, None, 1.0),
        "rectangular shadow" => {
            // Item size, reduced half size, blur and the corner radii
            // (`qsg::shadow::shader`): edges at 20.5 and 2539.5 px.
            let u = [
                w as f32,
                h as f32,
                w as f32 * 0.5 - 20.5,
                h as f32 * 0.5 - 20.5,
                3.0,
            ];
            let radii = [12.0, 9.0, 6.0, 3.0];
            fr.rshadow(rect, u, radii, [0.2, 0.4, 0.6, 0.8], 1.0)
        }
        _ => unreachable!("{name}"),
    }
    fr
}

/// MultiEffect's first two BlurItem sizes for a source of `size` (`qsg::multieffect::blur_sizes`).
fn qsg_blur_levels(size: (usize, usize)) -> Vec<(f64, f64)> {
    let (w, h) = (
        (size.0 as f64 / 16.0).ceil() * 8.0,
        (size.1 as f64 / 16.0).ceil() * 8.0,
    );
    vec![(w, h), (w * 0.5, h * 0.5)]
}

/// `scene(name)` as a new context draws it (its programs built under the `Fp16` alive now).
fn draw(name: &str) -> Option<Canvas> {
    let mut g = gpu()?;
    if name == "looped blur" || name == "lock screen blur" {
        assert!(
            lock_kernel().looped(g.max_frag_uniforms)
                && looped_kernel().looped(g.max_frag_uniforms),
            "{} fragment uniform vectors: the loop is not used",
            g.max_frag_uniforms
        );
    }
    let mut fr = scene(name);
    let mut s = g.pbuffer(fr.w as i32, fr.h as i32).expect("pbuffer");
    s.reswap();
    g.render(&mut s, &mut fr, || {});
    let shot = g.read_surface(&s);
    assert!(shot.px.iter().any(|p| p[3] > 0), "{name}: nothing drawn");
    Some(shot)
}

/// How many pixels differ, and the first one that does.
fn differ(a: &Canvas, b: &Canvas) -> (usize, String) {
    assert_eq!((a.w, a.h), (b.w, b.h));
    let mut bad = (0..a.px.len()).filter(|&i| a.px[i] != b.px[i]);
    let first = bad.next().map_or(String::new(), |i| {
        format!(
            "({}, {}): {:?} for {:?}",
            i % a.w,
            i / a.w,
            a.px[i],
            b.px[i]
        )
    });
    (bad.count() + !first.is_empty() as usize, first)
}

/// Every effect shader draws the same pixels on a GPU whose mediump is fp16 (and that has
/// highp, like the RX 580) as at fp32, in a window past 2048 px: none reads a mediump input
/// or gl_FragCoord.
#[test]
fn effects_are_pixel_exact_at_fp16_mediump() {
    for name in SCENES {
        let Some(plain) = draw(name) else { return };
        let _on = Fp16::on();
        let fp16 = draw(name).unwrap();
        let (n, first) = differ(&fp16, &plain);
        assert_eq!(
            n, 0,
            "{name} at fp16 mediump: {n} pixels differ, first at {first}"
        );
    }
}

/// What the test above would miss without the emulation. On a GPU with no highp in fragment
/// shaders `HP` is mediump and texture coordinates are fp16: they cannot address single
/// texels of a texture wider than 2048 (steps of 1/2048 in [0.5, 1)), and RSHADOW_FS's item
/// coordinates step by 2 px past 2048. No shader can: the sampler takes the coordinate at
/// the precision the GPU has. Qt's own shaders ask for highp there (qsb starts the GLSL ES
/// 1.00 variant of a fragment shader with `precision highp float;`).
#[test]
fn effects_lose_pixels_past_2048_without_highp() {
    for name in SCENES {
        let Some(plain) = draw(name) else { return };
        let _on = Fp16::no_highp();
        let fp16 = draw(name).unwrap();
        let (n, first) = differ(&fp16, &plain);
        elog::eprintln!("{name} without highp: {n} pixels differ, first at {first}");
        assert!(n > 0, "{name} is exact without highp");
    }
}

#[test]
fn image_is_pixel_exact_at_fp16_mediump() {
    let _on = Fp16::on();
    let Some(mut g) = gpu() else { return };
    if let Some((at, n)) = image(&mut g, None) {
        panic!("TEX_FS at fp16 mediump: {n} pixels differ, first at {at}");
    }
}

#[test]
fn image_before_fix_fails_at_fp16_mediump() {
    let fs = tex_fs_mediump();
    let _on = Fp16::on();
    let Some(mut g) = gpu() else { return };
    let (at, n) = image(&mut g, Some(&fs)).expect("mediump TEX_FS passed at fp16");
    elog::eprintln!("mediump TEX_FS at fp16: {n} pixels differ, first at {at}");
}
