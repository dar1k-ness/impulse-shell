//! Every way a texture is stretched over a scene, against Qt Quick through OpenGL
//! (`support/qt_gl.rs`, llvmpipe), pixel by pixel on both of our paths: the CPU raster and
//! ours through OpenGL. Qt samples all of them with the video card's one linear filter, and
//! the CPU raster repeats llvmpipe's arithmetic (`qsg::layer::linear`).
//!
//! Each scene carries how many pixels of each path may differ from Qt's at all. It is 0 where
//! ours is Qt's. Where it is not, the number is what was measured, so it cannot grow, and the
//! scene says what is missing; whoever closes it lowers the number.

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const W: usize = 160;
const H: usize = 80;
const BG: [u8; 4] = [0x20, 0x20, 0x20, 0xff];
const IMPORTS: &str = "import QtQuick\nimport QtQuick.Effects\nimport Qt5Compat.GraphicalEffects\n";

/// For a scene Qt itself draws differently from run to run: counted and printed, not held.
const NOT_HELD: usize = usize::MAX;

/// `write_png(.., 10, 6)` as a one-frame GIF (its 60 colours fit a palette).
#[rustfmt::skip]
const GIF: &[u8] = &[
    71, 73, 70, 56, 55, 97, 10, 0, 6, 0, 133, 0, 0, 204, 212, 255, 153, 212, 255, 229,
    170, 255, 178, 170, 255, 229, 212, 0, 178, 212, 0, 204, 170, 0, 153, 170, 0, 102, 212, 255,
    127, 170, 255, 127, 212, 0, 102, 170, 0, 204, 127, 255, 153, 127, 255, 229, 127, 0, 178, 127,
    0, 102, 127, 255, 127, 127, 0, 51, 212, 255, 0, 212, 255, 76, 170, 255, 25, 170, 255, 76,
    212, 0, 25, 212, 0, 51, 170, 0, 0, 170, 0, 51, 127, 255, 0, 127, 255, 76, 127, 0,
    25, 127, 0, 229, 85, 255, 178, 85, 255, 204, 85, 0, 153, 85, 0, 127, 85, 255, 102, 85,
    0, 76, 85, 255, 25, 85, 255, 51, 85, 0, 0, 85, 0, 204, 42, 255, 153, 42, 255, 229,
    0, 255, 178, 0, 255, 229, 42, 0, 178, 42, 0, 204, 0, 0, 153, 0, 0, 102, 42, 255,
    127, 0, 255, 127, 42, 0, 102, 0, 0, 51, 42, 255, 0, 42, 255, 76, 0, 255, 25, 0,
    255, 76, 42, 0, 25, 42, 0, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 44, 0, 0, 0, 0, 10, 0, 6, 0, 0, 8, 70, 0, 119, 220,
    208, 97, 99, 70, 140, 23, 43, 92, 168, 168, 145, 131, 6, 14, 24, 50, 82, 180, 64, 193,
    226, 68, 9, 19, 36, 70, 136, 8, 241, 1, 132, 135, 13, 29, 52, 112, 128, 16, 161, 193,
    3, 6, 14, 50, 84, 192, 64, 97, 65, 130, 3, 3, 12, 8, 152, 112, 65, 130, 5, 4,
    10, 2, 20, 0, 64, 32, 32, 0, 59,
];

/// A file whose every pixel differs from its neighbours, so a scaled copy shows how it was scaled.
fn write_png(dir: &std::path::Path, name: &str, w: usize, h: usize) -> String {
    let mut c = Canvas::new(w, h, [0, 0, 0, 255]);
    for (n, p) in c.px.iter_mut().enumerate() {
        let (x, y) = (n % w, n / w);
        *p = [
            (x * 255 / w) as u8,
            (y * 255 / h) as u8,
            ((x + y) % 2 * 255) as u8,
            255,
        ];
    }
    let file = dir.join(name);
    c.save_png(file.to_str().unwrap()).unwrap();
    file.to_str().unwrap().to_string()
}

/// A scene and how many pixels of our CPU raster and of ours through OpenGL may differ from
/// Qt's.
struct Scene {
    name: &'static str,
    body: String,
    cpu: usize,
    gpu: usize,
}

fn scenes(dir: &std::path::Path) -> Vec<Scene> {
    let small = write_png(dir, "small.png", 10, 6);
    let big = write_png(dir, "big.png", 100, 30);
    let [p16, p20, p32, p65] = [16, 20, 32, 65].map(|n| write_png(dir, &format!("{n}.png"), n, n));
    let icon = "x: 4; y: 4; width: 20; height: 20";
    let gif = dir.join("small.gif");
    std::fs::write(&gif, GIF).unwrap();
    let gif = gif.to_str().unwrap();
    // The source of an effect, off to the side.
    let src = format!("Image {{ id: s; x: 120; y: 60; source: \"{small}\" }}\n");
    let at = "x: 4; y: 4; width: 26; height: 16";
    let scene = |name, body: String, cpu, gpu| Scene {
        name,
        body,
        cpu,
        gpu,
    };
    vec![
        // ---- Image: one linear filter, Qt's.
        scene(
            "Image stretched",
            format!("Image {{ {at}; source: \"{small}\" }}"),
            0,
            0,
        ),
        scene(
            "Image, PreserveAspectFit",
            format!(
                "Image {{ x: 4; y: 4; width: 40; height: 40; fillMode: Image.PreserveAspectFit; \
                 source: \"{small}\" }}"
            ),
            0,
            0,
        ),
        scene(
            "Image, PreserveAspectCrop",
            format!(
                "Image {{ x: 4; y: 4; width: 40; height: 40; clip: true; \
                 fillMode: Image.PreserveAspectCrop; source: \"{small}\" }}"
            ),
            0,
            0,
        ),
        scene(
            "Image, smooth: false",
            format!("Image {{ {at}; smooth: false; source: \"{small}\" }}"),
            0,
            0,
        ),
        scene(
            "Image shrunk",
            format!("Image {{ x: 4; y: 4; width: 37; height: 11; source: \"{big}\" }}"),
            0,
            0,
        ),
        scene(
            "AnimatedImage stretched",
            format!("AnimatedImage {{ {at}; playing: false; source: \"{gif}\" }}"),
            0,
            0,
        ),
        // ---- `mipmap: true`: a texture of its own with the chain of halved levels, and the
        // video card's choice among them. Shrunk more than a little, two levels are blended.
        scene(
            "Image shrunk, mipmap: true",
            format!(
                "Image {{ x: 4; y: 4; width: 37; height: 11; mipmap: true; source: \"{big}\" }}"
            ),
            0,
            0,
        ),
        scene(
            "Image shrunk, mipmap: true, smooth: false",
            format!(
                "Image {{ x: 4; y: 4; width: 37; height: 11; mipmap: true; smooth: false; \
                 source: \"{big}\" }}"
            ),
            0,
            0,
        ),
        // Half the size is the second level alone, a little under the size the first two,
        // a ninth the fourth and the fifth; one side shrunk and the other stretched goes by
        // the side that shrinks more, and a cropped picture by what is shown of it.
        scene(
            "Image 32x32 at 16, mipmap: true",
            format!(
                "Image {{ x: 4; y: 4; width: 16; height: 16; mipmap: true; source: \"{p32}\" }}"
            ),
            0,
            0,
        ),
        scene(
            "Image 20x20 at 19, mipmap: true",
            format!(
                "Image {{ x: 4; y: 4; width: 19; height: 19; mipmap: true; source: \"{p20}\" }}"
            ),
            0,
            0,
        ),
        scene(
            "Image shrunk to a ninth, mipmap: true",
            format!(
                "Image {{ x: 4; y: 4; width: 11; height: 3; mipmap: true; source: \"{big}\" }}"
            ),
            0,
            0,
        ),
        scene(
            "Image shrunk across and stretched down, mipmap: true",
            format!(
                "Image {{ x: 4; y: 4; width: 30; height: 40; mipmap: true; source: \"{big}\" }}"
            ),
            0,
            0,
        ),
        scene(
            "Image shrunk and cropped, mipmap: true",
            format!(
                "Image {{ x: 4; y: 4; width: 24; height: 20; mipmap: true; \
                 fillMode: Image.PreserveAspectCrop; clip: true; source: \"{big}\" }}"
            ),
            0,
            0,
        ),
        // The two `mipmap: true` of qml/ at the sizes the running shell has there.
        // OverviewWindow.qml:130: a window's icon, 65.394 wide of a 65x65 texture on a
        // 1920x1080 screen. SysTrayMenuEntry.qml:105 (Quickshell's `IconImage`: an `Image`
        // that fits, 20 wide): a named icon is a 20x20 texture, and a menu's `icon-data`
        // picture has the size the application gave it, 16x16 from CopyQ's menu.
        scene(
            "Image 65x65 at 65.394, mipmap: true",
            format!(
                "Image {{ x: 4; y: 4; width: 65.394; height: 65.394; mipmap: true; \
                 source: \"{p65}\" }}"
            ),
            0,
            0,
        ),
        scene(
            "Image 20x20 at 20, mipmap: true",
            format!("Image {{ {icon}; mipmap: true; source: \"{p20}\" }}"),
            0,
            0,
        ),
        scene(
            "Image 16x16 at 20, mipmap: true",
            format!(
                "Image {{ {icon}; mipmap: true; fillMode: Image.PreserveAspectFit; \
                 source: \"{p16}\" }}"
            ),
            0,
            0,
        ),
        // No application of the guest sends a picture larger than the entry; one that does
        // is shrunk through the levels.
        scene(
            "Image 32x32 at 20, mipmap: true",
            format!("Image {{ {icon}; mipmap: true; source: \"{p32}\" }}"),
            0,
            0,
        ),
        // The CPU raster gives the row whose centre is on the edge to the wrong side.
        scene(
            "Image stretched, edges on pixel centres",
            format!("Image {{ x: 4.5; y: 4.5; width: 26; height: 16; source: \"{small}\" }}"),
            52,
            0,
        ),
        scene(
            "Image 1:1, edges on pixel centres",
            format!("Image {{ x: 4.5; y: 4.5; source: \"{small}\" }}"),
            20,
            0,
        ),
        // BorderImage is not implemented: draws nothing.
        scene(
            "BorderImage",
            format!(
                "BorderImage {{ x: 4; y: 4; width: 60; height: 40; \
                 border {{ left: 3; right: 3; top: 2; bottom: 2 }} source: \"{small}\" }}"
            ),
            2400,
            2400,
        ),
        // ---- Layers.
        // `layer.textureSize` is not implemented: the layer has the item's size.
        scene(
            "layer.textureSize",
            format!(
                "Image {{ x: 4; y: 4; source: \"{small}\"; layer.enabled: true; \
                 layer.smooth: true; layer.textureSize: Qt.size(26, 16) }}"
            ),
            60,
            60,
        ),
        // The CPU raster blends a scaled layer's edge with what is outside the texture.
        scene(
            "layer under scale, layer.smooth: true",
            format!(
                "Item {{ x: 4; y: 4; width: 10; height: 6; scale: 2.6; \
                 transformOrigin: Item.TopLeft; layer.enabled: true; layer.smooth: true; \
                 Image {{ source: \"{small}\" }} }}"
            ),
            258,
            0,
        ),
        // Qt reads a layer that is not smooth from the nearest texel; ours reads it linearly.
        scene(
            "layer under scale, not smooth",
            format!(
                "Item {{ x: 4; y: 4; width: 10; height: 6; scale: 2.6; \
                 transformOrigin: Item.TopLeft; layer.enabled: true; \
                 Image {{ source: \"{small}\" }} }}"
            ),
            502,
            400,
        ),
        // A ShaderEffectSource that is an item of the scene draws nothing.
        scene(
            "ShaderEffectSource as an item",
            format!("{src}ShaderEffectSource {{ {at}; sourceItem: s; smooth: true }}"),
            416,
            416,
        ),
        // ---- Effects larger than their source.
        scene(
            "MultiEffect, no effect on",
            format!("{src}MultiEffect {{ {at}; source: s }}"),
            0,
            0,
        ),
        // The blurred picture is laid out differently from Qt's.
        scene(
            "MultiEffect, blur",
            format!("{src}MultiEffect {{ {at}; source: s; blurEnabled: true; blur: 0.5 }}"),
            903,
            145,
        ),
        scene(
            "ColorOverlay",
            format!("{src}ColorOverlay {{ {at}; source: s; color: \"#80ff0000\" }}"),
            0,
            0,
        ),
        scene(
            "Desaturate",
            format!("{src}Desaturate {{ {at}; source: s; desaturation: 0.5 }}"),
            0,
            0,
        ),
        scene(
            "OpacityMask",
            format!(
                "{src}Rectangle {{ id: m; x: 120; y: 40; width: 10; height: 6; radius: 3 }}\n\
                 OpacityMask {{ {at}; source: s; maskSource: m }}"
            ),
            0,
            0,
        ),
        scene(
            "GaussianBlur",
            format!("{src}GaussianBlur {{ {at}; source: s; radius: 4; samples: 9 }}"),
            0,
            0,
        ),
        scene(
            "DropShadow",
            format!(
                "{src}DropShadow {{ {at}; source: s; radius: 4; samples: 9; \
                 horizontalOffset: 2; color: \"#80000000\" }}"
            ),
            0,
            0,
        ),
        // FastBlur is not implemented: draws nothing.
        scene(
            "FastBlur",
            format!("{src}FastBlur {{ {at}; source: s; radius: 8 }}"),
            416,
            416,
        ),
        // ---- Glyphs under a scale: Qt stretches the glyph cache's texture, ours draws the
        // glyphs of the larger size. Qt's natively rendered glyphs come out differently from
        // run to run (382 to 386 pixels of ink).
        scene(
            "native text under scale",
            "Text { x: 4; y: 4; text: \"Agy\"; color: \"white\"; font.pixelSize: 14; \
             renderType: Text.NativeRendering; scale: 1.5; transformOrigin: Item.TopLeft }"
                .into(),
            NOT_HELD,
            NOT_HELD,
        ),
        scene(
            "distance field text under scale",
            "Text { x: 4; y: 4; text: \"Agy\"; color: \"white\"; font.pixelSize: 14; \
             scale: 1.5; transformOrigin: Item.TopLeft }"
                .into(),
            539,
            445,
        ),
        // ---- A gradient's texture.
        scene(
            "horizontal gradient",
            "Rectangle { x: 4; y: 4; width: 60; height: 40; gradient: Gradient { \
             orientation: Gradient.Horizontal; \
             GradientStop { position: 0; color: \"red\" } \
             GradientStop { position: 1; color: \"#8000ff00\" } } }"
                .into(),
            0,
            0,
        ),
    ]
}

/// `body` in a window of ours, as the CPU raster paints it or as ours draws it through OpenGL.
/// On a thread of its own: the painter's caches are per thread and keyed by item, and every
/// scene here is a new runtime with the same items.
fn ours(body: &str, gpu: bool) -> Canvas {
    let body = body.to_string();
    std::thread::spawn(move || paint(&body, gpu))
        .join()
        .expect("the scene is painted")
}

fn paint(body: &str, gpu: bool) -> Canvas {
    let source = format!(
        "{IMPORTS}import Rustshell\nFloatingWindow {{\n    width: {W}; height: {H}; \
         color: \"#202020\"\n{body}\n}}\n"
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [W as f64, H as f64]);
    }
    if !gpu {
        let mut cpu = Canvas::new(W, H, BG);
        scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut cpu));
        return cpu;
    }
    let (mut gpu, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut surface = gpu.pbuffer(W as i32, H as i32).expect("pbuffer");
    let mut frame = shell::gpu::Frame::new(W, H);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut frame));
    gpu.render(&mut surface, &mut frame, || {});
    gpu.read_surface(&surface)
}

/// How many pixels differ and the largest difference of a channel.
fn differ(ours: &Canvas, qt: &Canvas) -> (usize, u8) {
    assert_eq!((ours.w, ours.h), (qt.w, qt.h));
    let far = |(a, b): (&[u8; 4], &[u8; 4])| (0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap();
    let off = ours.px.iter().zip(&qt.px).map(far).filter(|d| *d > 0);
    (off.clone().count(), off.max().unwrap_or(0))
}

#[test]
fn stretched_textures_are_qt_quicks() {
    if !qt_gl::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut wrong = Vec::new();
    for s in scenes(dir.path()) {
        let source = format!(
            "{IMPORTS}Rectangle {{\n    width: {W}; height: {H}; color: \"#202020\"\n{}\n}}\n",
            s.body
        );
        let (_, qt) = qt_gl::run(&source);
        assert!(
            qt.px.iter().any(|p| *p != BG),
            "{}: Qt draws nothing",
            s.name
        );
        for (path, gpu, most) in [("CPU raster", false, s.cpu), ("OpenGL", true, s.gpu)] {
            let (n, max) = differ(&ours(&s.body, gpu), &qt);
            println!("{}, {path}: {n} pixels differ, by {max} at most", s.name);
            if n > most {
                wrong.push(format!(
                    "{}, {path}: {n} pixels differ from Qt's (by {max} at most), {most} allowed",
                    s.name
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
