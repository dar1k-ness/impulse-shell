//! QMovie: the frames of an animated GIF or WebP (QMovie::supportedFormats() of Qt 6.12 with
//! qt6-imageformats; its SVG entries have one frame here), read one at a time as
//! QImageReader does, and a picture that does not move as the one frame QMovie plays it as. One canvas lives between frames and each frame's rectangle is drawn on
//! it (QGifHandler's `lastImage`, QWebpHandler's `m_composited`); no frame is kept here.

use qsg::raster::Canvas;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

type Px = [u8; 4];

/// An animated file open for reading.
pub struct Movie {
    path: String,
    /// The canvas every frame is composited on.
    size: (usize, usize),
    /// QImageReader::setScaledSize: the size a frame is handed out at.
    scaled: (usize, usize),
    /// Milliseconds each frame shows for.
    delays: Vec<f64>,
    kind: Kind,
    play: Option<Play>,
    /// The frame `play` reads next.
    next: usize,
    /// The frame handed out last has no alpha channel and yet pixels whose alpha is not 255.
    raw: bool,
}

/// What the file's blocks say of each frame, read before any pixel.
enum Kind {
    Gif(GifScan),
    Webp(WebpScan),
    /// A picture that does not move, and its frame until it is asked for.
    Still(Option<Canvas>),
}

enum Play {
    Gif(Box<GifPlay>),
    Webp(Box<WebpPlay>),
}

impl Movie {
    /// `path` as a movie whose frames are scaled to `source_size` (QQuickAnimatedImage hands
    /// `sourceSize` to QMovie::setScaledSize); `None` for a file QImageReader does not read
    /// and a canvas over its allocation limit, refused from the header before a pixel is
    /// decoded. A file of one frame is a movie too: Qt plays it and stops. So is a picture
    /// that does not move: QMovie shows a format that is no animation for 1000 ms, a WebP
    /// without frames for none. An SVG is not opened here.
    pub fn open(path: &str, source_size: (i32, i32)) -> Option<Movie> {
        let mut file = BufReader::new(File::open(path).ok()?);
        let head = file.fill_buf().ok()?;
        let webp = head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP");
        let moving = if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
            let scan = scan_gif(&mut file)?;
            let delays = scan.frames.iter().map(|f| f.delay).collect();
            Some((scan.size, delays, Kind::Gif(scan)))
        } else if webp {
            scan_webp(&mut file).map(|(size, delays, scan)| (size, delays, Kind::Webp(scan)))
        } else {
            None
        };
        let (size, delays, kind) = match moving {
            Some(m) if !m.1.is_empty() => m,
            // A WebP with no frames is one picture; a GIF with none is nothing.
            Some((_, _, Kind::Gif(_))) => return None,
            _ => {
                let mut size = (0, 0);
                let frame = still(path, source_size, &mut size)?;
                let delay = if webp { 0.0 } else { 1000.0 };
                (size, vec![delay], Kind::Still(Some(frame)))
            }
        };
        if delays.is_empty() || size.0 == 0 || size.1 == 0 {
            return None;
        }
        let scaled = scaled_size(size, source_size);
        qsg::image::check_size(size.0, size.1).ok()?;
        qsg::image::check_size(scaled.0, scaled.1).ok()?;
        Some(Movie {
            path: path.to_string(),
            size,
            scaled,
            delays,
            kind,
            play: None,
            next: 0,
            raw: false,
        })
    }

    /// Whether the frame handed out last is a Format_RGB32 picture with pixels whose alpha
    /// byte is not 255 (QGifHandler leaves 0x00ffffff where it clears an image that has no
    /// transparent colour, and 0 where it drew nothing). Qt Quick hands such a picture to the
    /// graphics card as it is and draws it without blending while nothing makes it
    /// translucent: the window gets those pixels, alpha and all.
    pub fn raw(&self) -> bool {
        self.raw
    }

    pub fn frame_count(&self) -> usize {
        self.delays.len()
    }

    /// QMovie::nextFrameDelay() of each frame, in milliseconds.
    pub fn delays(&self) -> &[f64] {
        &self.delays
    }

    /// QImageReader::loopCount(): the times the file asks to be played again after the
    /// first, -1 for ever. A GIF with no NETSCAPE extension asks for none.
    pub fn loops(&self) -> i32 {
        match &self.kind {
            Kind::Gif(scan) => match scan.loops {
                None => 0,
                Some(0) => -1,
                Some(n) => n as i32,
            },
            Kind::Webp(scan) => scan.loops as i32 - 1,
            Kind::Still(..) => 0,
        }
    }

    /// QMovie::frameCount() is QImageReader::imageCount(), which a format that is no
    /// animation answers with 1 while it can read and 0 once its picture is read, until
    /// QMovie looked for a second frame.
    pub fn counts_late(&self) -> bool {
        matches!(self.kind, Kind::Still(..)) && self.delays[0] != 0.0
    }

    /// Frame `k`, composited and premultiplied. The frames up to `k` are read in order, from
    /// the start of the file when `k` is not after the last one read (QMovie rewinds its
    /// device the same way to loop). `None` when the file gives no such frame.
    pub fn frame(&mut self, k: usize) -> Option<Canvas> {
        if k >= self.delays.len() {
            return None;
        }
        if let Kind::Still(frame) = &mut self.kind {
            return match frame.take() {
                Some(c) => Some(c),
                None => still(
                    &self.path,
                    (self.scaled.0 as i32, self.scaled.1 as i32),
                    &mut (0, 0),
                ),
            };
        }
        if self.play.is_none() || k < self.next {
            self.play = None;
            self.next = 0;
            let file = BufReader::new(File::open(&self.path).ok()?);
            self.play = Some(match &self.kind {
                Kind::Gif(_) => Play::Gif(Box::new(GifPlay::new(file)?)),
                Kind::Webp(scan) => Play::Webp(Box::new(WebpPlay::new(file, self.size, scan))),
                Kind::Still(..) => return None,
            });
        }
        loop {
            let last = self.next == k;
            let (size, scaled) = (self.size, self.scaled);
            let out = match (self.play.as_mut()?, &self.kind) {
                (Play::Gif(g), Kind::Gif(scan)) => g.read(size, scan, self.next).map(|()| {
                    last.then(|| {
                        let frame = scale(&g.canvas, scaled, !g.opaque);
                        let raw = g.opaque && g.holes && frame.px.iter().any(|p| p[3] != 255);
                        (frame, raw)
                    })
                }),
                // QWebpHandler's canvas is ARGB32 whether or not the file has alpha.
                (Play::Webp(p), Kind::Webp(scan)) => p.read(scan, self.next).map(|()| {
                    last.then(|| match scaled == size {
                        true => (p.shown(), false),
                        // Scaled from the pixels the canvas has, not from premultiplied ones.
                        false => {
                            let (w, h) = size;
                            let frame =
                                qsg::image::scale_straight(&p.canvas, w, h, scaled.0, scaled.1);
                            (frame, false)
                        }
                    })
                }),
                _ => None,
            };
            let Some(out) = out else {
                self.play = None;
                return None;
            };
            self.next += 1;
            if let Some((frame, raw)) = out {
                self.raw = raw;
                return Some(frame);
            }
        }
    }
}

/// The picture of a file that does not move at `source_size`, as QImageReader::read gives it
/// with that scaled size; `size` becomes the file's own.
fn still(path: &str, source_size: (i32, i32), size: &mut (usize, usize)) -> Option<Canvas> {
    let data = std::fs::read(path).ok()?;
    let sized = |w, h| {
        *size = (w, h);
        scaled_size((w, h), source_size)
    };
    Some(qsg::image::decode_scaled(&data, sized).ok()?.0)
}

/// QImageReader::read: a handler that cannot scale (neither QGifHandler nor QWebpHandler has
/// the ScaledSize option) reads the whole canvas, which is then scaled smoothly.
/// The first image of the GIF `path` as QImageReader reads it for an Image, at the size
/// `sized` makes of the file's, and whether it has no alpha channel and yet pixels whose
/// alpha is not 255 (`Movie::raw`). `None` for what is no GIF or cannot be read.
pub fn gif_first(
    path: &str,
    sized: impl Fn(usize, usize) -> (usize, usize),
) -> Option<(Canvas, bool)> {
    let mut file = BufReader::new(File::open(path).ok()?);
    let head = file.fill_buf().ok()?;
    if !head.starts_with(b"GIF87a") && !head.starts_with(b"GIF89a") {
        return None;
    }
    let scan = scan_gif(&mut file)?;
    let size = scan.size;
    if scan.frames.is_empty() || size.0 == 0 || size.1 == 0 {
        return None;
    }
    let to = sized(size.0, size.1);
    qsg::image::check_size(size.0, size.1).ok()?;
    qsg::image::check_size(to.0, to.1).ok()?;
    let mut play = GifPlay::new(BufReader::new(File::open(path).ok()?))?;
    play.read(size, &scan, 0)?;
    let frame = scale(&play.canvas, to, !play.opaque);
    let raw = play.opaque && play.holes && frame.px.iter().any(|p| p[3] != 255);
    Some((frame, raw))
}

fn scale(canvas: &Canvas, to: (usize, usize), alpha: bool) -> Canvas {
    if to == (canvas.w, canvas.h) {
        Canvas::from_px(canvas.w, canvas.h, canvas.px.clone())
    } else {
        qsg::image::scale_decoded(canvas, to.0, to.1, alpha)
    }
}

/// QImageReader::read with a scaled size: both sides given scale to exactly that size, the
/// aspect ratio ignored; one side given keeps the ratio (rounded, at least 1).
fn scaled_size(size: (usize, usize), ss: (i32, i32)) -> (usize, usize) {
    let other = |given: i32, of: usize, to: usize| {
        ((to as f64 * (given as f64 / of as f64)).round() as usize).max(1)
    };
    match (ss.0 > 0, ss.1 > 0) {
        (true, true) => (ss.0 as usize, ss.1 as usize),
        (true, false) => (ss.0 as usize, other(ss.0, size.0, size.1)),
        (false, true) => (other(ss.1, size.1, size.0), ss.1 as usize),
        (false, false) => size,
    }
}

/// What the blocks before a GIF image say of it.
#[derive(Clone, Copy)]
struct GifFrame {
    delay: f64,
    trans: Option<u8>,
    dispose: u8,
}

#[derive(Default)]
struct GifScan {
    size: (usize, usize),
    /// The background colour index: a GIF with a global colour table has one.
    bg: Option<u8>,
    /// The loop count of the NETSCAPE extension, when the file has one.
    loops: Option<usize>,
    frames: Vec<GifFrame>,
}

const RESTORE_BACKGROUND: u8 = 2;
const RESTORE_IMAGE: u8 = 3;

/// QGIFFormat::scan: the images of a GIF counted from its blocks, no pixel decoded, with what
/// QGIFFormat::decode keeps for each from the Graphic Control Extensions. An image with no
/// extension of its own keeps the delay and the transparent index of the one before
/// (QGifHandler's `nextDelay` starts at 100 ms); a delay under 2/100 s plays at 100 ms.
fn scan_gif(r: &mut BufReader<File>) -> Option<GifScan> {
    let mut b = [0u8; 13];
    r.read_exact(&mut b).ok()?;
    let le = |lo: u8, hi: u8| usize::from(lo) | usize::from(hi) << 8;
    let global = b[10] & 0x80 != 0;
    let mut scan = GifScan {
        size: (le(b[6], b[7]), le(b[8], b[9])),
        bg: global.then_some(b[11]),
        loops: None,
        frames: Vec::new(),
    };
    if global {
        r.seek_relative(3 * (2 << (b[10] & 7))).ok()?;
    }
    let mut next = GifFrame {
        delay: 100.0,
        trans: None,
        dispose: 0,
    };
    // A file that ends or breaks mid-way has the images counted so far, as Qt's scan does.
    let mut blocks = |scan: &mut GifScan| -> Option<bool> {
        let mut c = [0u8; 1];
        r.read_exact(&mut c).ok()?;
        match c[0] {
            b',' => {
                let mut d = [0u8; 9];
                r.read_exact(&mut d).ok()?;
                if scan.frames.is_empty() {
                    if scan.size.0 == 0 {
                        scan.size.0 = le(d[0], d[1]) + le(d[4], d[5]);
                    }
                    if scan.size.1 == 0 {
                        scan.size.1 = le(d[2], d[3]) + le(d[6], d[7]);
                    }
                }
                scan.frames.push(next);
                next.dispose = 0;
                if d[8] & 0x80 != 0 {
                    r.seek_relative(3 * (2 << (d[8] & 7))).ok()?;
                }
                r.read_exact(&mut c).ok()?;
                if c[0] > 12 {
                    return None;
                }
                skip_blocks(r)?;
            }
            b'!' => {
                r.read_exact(&mut c).ok()?;
                if c[0] == 0xf9 {
                    r.read_exact(&mut c).ok()?;
                    let n = usize::from(c[0]);
                    let mut ext = [0u8; 255];
                    r.read_exact(&mut ext[..n]).ok()?;
                    let dispose = (ext[0] >> 2) & 7;
                    let cs = if n >= 3 { le(ext[1], ext[2]) } else { 1 };
                    next = GifFrame {
                        delay: (if cs < 2 { 10 } else { cs } * 10) as f64,
                        trans: (ext[0] & 1 != 0).then_some(ext[3]),
                        dispose: if dispose <= RESTORE_IMAGE { dispose } else { 0 },
                    };
                    if n == 0 {
                        return Some(true);
                    }
                } else if c[0] == 0xff {
                    r.read_exact(&mut c).ok()?;
                    let mut ext = [0u8; 255];
                    r.read_exact(&mut ext[..usize::from(c[0])]).ok()?;
                    if ext.starts_with(b"NETSCAPE") {
                        r.read_exact(&mut c).ok()?;
                        let n = usize::from(c[0]);
                        let mut ext = [0u8; 255];
                        r.read_exact(&mut ext[..n]).ok()?;
                        if n == 0 {
                            return Some(true);
                        }
                        scan.loops = Some(le(ext[1], ext[2]));
                    }
                }
                skip_blocks(r)?;
            }
            _ => return Some(false),
        }
        Some(true)
    };
    while blocks(&mut scan) == Some(true) {}
    Some(scan)
}

fn skip_blocks(r: &mut BufReader<File>) -> Option<()> {
    let mut n = [0u8; 1];
    loop {
        r.read_exact(&mut n).ok()?;
        if n[0] == 0 {
            return Some(());
        }
        r.seek_relative(i64::from(n[0])).ok()?;
    }
}

/// QGIFFormat::decode's state between frames.
struct GifPlay {
    dec: gif::Decoder<BufReader<File>>,
    /// QGifHandler's `lastImage`, kept as the pixels a frame is handed out in: premultiplied
    /// ones of a Format_ARGB32 image, and the four bytes of each pixel of a Format_RGB32 one.
    canvas: Canvas,
    /// Format_RGB32: the first image has no transparent index (QGIFFormat picks the format
    /// once, at the first image). What reads the picture takes every pixel for opaque; the
    /// alpha bytes are still what QGIFFormat wrote, 0 where it cleared.
    opaque: bool,
    /// The canvas may hold a pixel whose alpha byte is not 255: one that was cleared, that no
    /// image drew, or whose colour index is past the colour table.
    holes: bool,
    prev: Option<Disposal>,
    /// The pixels under a frame that asks for them back (RestoreImage).
    backing: Vec<Px>,
    row: Vec<u8>,
}

/// QGIFFormat::disposePrevious: what becomes of a frame's rectangle before the next one.
struct Disposal {
    method: u8,
    /// Inclusive `left, top, right, bottom`, clipped as Qt clips them.
    rect: [usize; 4],
    /// RestoreBackground's fill; `None` is the canvas's first pixel.
    fill: Option<Px>,
}

impl GifPlay {
    fn new(file: BufReader<File>) -> Option<GifPlay> {
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::Indexed);
        Some(GifPlay {
            dec: opts.read_info(file).ok()?,
            canvas: Canvas::from_px(0, 0, Vec::new()),
            opaque: true,
            holes: false,
            prev: None,
            backing: Vec::new(),
            row: Vec::new(),
        })
    }

    /// Draws image `n` of the file on the canvas, as QGIFFormat::decode does.
    fn read(&mut self, (sw, sh): (usize, usize), scan: &GifScan, n: usize) -> Option<()> {
        let meta = *scan.frames.get(n)?;
        let info = self.dec.next_frame_info().ok()??;
        let (left, top) = (usize::from(info.left), usize::from(info.top));
        let (width, height) = (usize::from(info.width), usize::from(info.height));
        let interlaced = info.interlaced;
        let local = info.palette.clone();
        if n == 0 {
            self.opaque = meta.trans.is_none();
            // The new image is set to zero bytes.
            self.canvas = Canvas::from_px(sw, sh, vec![[0; 4]; sw * sh]);
            self.holes = false;
        }
        // Q_TRANSPARENT (0x00ffffff): white with an alpha of 0 where the format has no alpha.
        let clear: Px = if self.opaque {
            [255, 255, 255, 0]
        } else {
            [0; 4]
        };
        // QGIFFormat::color: an index past the colour table is transparent, and so is the
        // transparent index itself.
        let mut colors = [clear; 256];
        let table = local.as_deref().or(self.dec.global_palette());
        let known = table.map_or(0, |t| t.len() / 3);
        for (c, rgb) in colors.iter_mut().zip(table.unwrap_or_default().chunks(3)) {
            *c = [rgb[0], rgb[1], rgb[2], 255];
        }
        if let (Some(t), false) = (meta.trans, self.opaque) {
            colors[usize::from(t)] = [0; 4];
        }
        let px = &mut self.canvas.px;
        if let Some(d) = self.prev.take() {
            let [l, t, r, b] = d.rect;
            // With no background colour known the fill is the picture's first pixel.
            let fill = d.fill.unwrap_or(px[0]);
            self.holes |= d.method == RESTORE_BACKGROUND && fill[3] != 255;
            for y in t..=b {
                let line = &mut px[y * sw + l..=y * sw + r];
                match d.method {
                    RESTORE_BACKGROUND => line.fill(fill),
                    _ => {
                        let at = (y - t) * (r - l + 1);
                        line.copy_from_slice(&self.backing[at..at + line.len()]);
                    }
                }
            }
        }
        if n == 0 && (left != 0 || top != 0 || width < sw || height < sh) {
            // A first image smaller than the screen: the rest is the transparent colour or
            // the background colour.
            match (meta.trans, scan.bg) {
                (Some(t), _) => px.fill(colors[usize::from(t)]),
                (None, Some(bg)) => px.fill(colors[usize::from(bg)]),
                (None, None) => {}
            }
            self.holes |= px[0][3] != 255;
        }
        // QGIFFormat's `right`/`bottom` and the clipping of disposePrevious.
        let edge = |at: usize, len: usize, screen: usize| {
            let far = ((at + len).min(screen) as i64 - 1).max(0) as usize;
            (at.min(screen - 1), far.min(screen - 1))
        };
        let ((l, r), (t, b)) = (edge(left, width, sw), edge(top, height, sh));
        if meta.dispose == RESTORE_IMAGE && r >= l && b >= t {
            self.backing.clear();
            for y in t..=b {
                self.backing.extend_from_slice(&px[y * sw + l..=y * sw + r]);
            }
        }
        if r >= l && b >= t && meta.dispose >= RESTORE_BACKGROUND {
            let fill = match (meta.trans, scan.bg) {
                (Some(_), _) => Some(clear),
                (None, Some(bg)) => Some(colors[usize::from(bg)]),
                (None, None) => None,
            };
            self.prev = Some(Disposal {
                method: meta.dispose,
                rect: [l, t, r, b],
                fill,
            });
        }
        // The first image draws every pixel, a later one leaves the transparent ones.
        let skip = meta.trans.filter(|_| n > 0);
        self.row.resize(width, 0);
        let rows: Box<dyn Iterator<Item = usize>> = if interlaced {
            let pass = |from: usize, step: usize| (from..height).step_by(step);
            Box::new(
                pass(0, 8)
                    .chain(pass(4, 8))
                    .chain(pass(2, 4))
                    .chain(pass(1, 2)),
            )
        } else {
            Box::new(0..height)
        };
        let mut highest = 0;
        for y in rows {
            // A file that ends inside an image shows the rows it has.
            if width == 0 || !self.dec.fill_buffer(&mut self.row).ok()? {
                self.holes = true;
                break;
            }
            if top + y >= sh || left >= sw {
                continue;
            }
            let line = &mut px[(top + y) * sw + left..(top + y) * sw + (left + width).min(sw)];
            for (p, i) in line.iter_mut().zip(&self.row) {
                highest = highest.max(*i);
                if Some(*i) != skip {
                    *p = colors[usize::from(*i)];
                }
            }
        }
        self.holes |= usize::from(highest) >= known;
        Some(())
    }
}

/// One ANMF chunk of an animated WebP.
struct WebpFrame {
    /// Where the frame's image chunks are in the file, and how many bytes.
    at: u64,
    len: usize,
    /// `left, top, width, height` on the canvas.
    rect: [usize; 4],
    /// Blended over the canvas; else it replaces its rectangle.
    blend: bool,
    /// Its rectangle is cleared before the next frame.
    dispose: bool,
}

#[derive(Default)]
struct WebpScan {
    /// The file's alpha flag (WebPBitstreamFeatures::has_alpha).
    alpha: bool,
    /// The ANIM chunk's background colour.
    bg: Px,
    /// The ANIM chunk's loop count: 0 plays for ever.
    loops: usize,
    frames: Vec<WebpFrame>,
}

/// WebPDemux: the canvas, the flags and the frames of an animated WebP from its chunks, no
/// pixel decoded. `None` for a file with no animation flag and for a frame outside the
/// canvas, which the demuxer refuses.
fn scan_webp(r: &mut BufReader<File>) -> Option<((usize, usize), Vec<f64>, WebpScan)> {
    let end = r.get_ref().metadata().ok()?.len();
    let u24 = |b: &[u8]| usize::from(b[0]) | usize::from(b[1]) << 8 | usize::from(b[2]) << 16;
    let (mut size, mut delays, mut scan) = ((0, 0), Vec::new(), WebpScan::default());
    let mut at = 12u64;
    let mut b = [0u8; 24];
    loop {
        r.seek(SeekFrom::Start(at)).ok()?;
        if r.read_exact(&mut b[..8]).is_err() {
            break;
        }
        let len = u64::from(u32::from_le_bytes([b[4], b[5], b[6], b[7]]));
        if at + 8 + len > end {
            break;
        }
        match &b[..4] {
            b"VP8X" if len >= 10 => {
                r.read_exact(&mut b[8..18]).ok()?;
                if b[8] & 0x02 == 0 {
                    return None;
                }
                scan.alpha = b[8] & 0x10 != 0;
                size = (u24(&b[12..15]) + 1, u24(&b[15..18]) + 1);
            }
            b"ANIM" if len >= 6 => {
                r.read_exact(&mut b[8..14]).ok()?;
                scan.bg = [b[10], b[9], b[8], b[11]];
                scan.loops = usize::from(b[12]) | usize::from(b[13]) << 8;
            }
            b"ANMF" if len >= 16 => {
                r.read_exact(&mut b[8..24]).ok()?;
                let rect = [
                    2 * u24(&b[8..11]),
                    2 * u24(&b[11..14]),
                    u24(&b[14..17]) + 1,
                    u24(&b[17..20]) + 1,
                ];
                if rect[0] + rect[2] > size.0 || rect[1] + rect[3] > size.1 {
                    return None;
                }
                delays.push(u24(&b[20..23]) as f64);
                scan.frames.push(WebpFrame {
                    at: at + 24,
                    len: (len - 16) as usize,
                    rect,
                    blend: b[23] & 2 == 0,
                    dispose: b[23] & 1 != 0,
                });
            }
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    Some((size, delays, scan))
}

/// qPremultiply.
fn premultiply([r, g, b, a]: Px) -> Px {
    let mul = |c: u8| mul255::qt(u32::from(c), u32::from(a)) as u8;
    [mul(r), mul(g), mul(b), a]
}

/// A pixel as QPainter blends it on a Format_ARGB32 image: 16 bits a channel, premultiplied.
type Px64 = [u32; 4];

/// convertARGB32ToRGBA64PM (the AVX2 one, eight pixels at a time): a pixel among eight
/// opaque ones (`opaque`) is widened only, any other is multiplied by its alpha.
fn fetch64(p: Px, opaque: bool) -> Px64 {
    let a = u32::from(p[3]) * 257;
    let mul = |c: u8| {
        let t = (u32::from(c) * 257 * a) >> 16;
        t + (t >> 15)
    };
    match opaque {
        true => p.map(|c| u32::from(c) * 257),
        false => [mul(p[0]), mul(p[1]), mul(p[2]), a],
    }
}

/// storeARGB32FromRGBA64PM (the SSE4 one): back to 8 bits a channel, not premultiplied. It
/// divides by a translucent alpha in single floats, with the reciprocal `rcpps` gives.
fn store64(p: Px64) -> Px {
    let div_257 = |v: u32| (((v + 128) - ((v + 128) >> 8)) >> 8) as u8;
    match p[3] {
        0 => [0; 4],
        0xffff => p.map(div_257),
        a => {
            let a = a as f32;
            let ia = rcp(a);
            let ia = ((ia + ia) - ia * (ia * a)) * 255.0;
            let div = |c: u32| (c as f32 * ia).round_ties_even().clamp(0.0, 255.0) as u8;
            [div(p[0]), div(p[1]), div(p[2]), div_257(p[3])]
        }
    }
}

/// `1 / a` as the `rcpps` instruction of this CPU approximates it.
fn rcp(a: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rcp_ss, _mm_set_ss};
        // SAFETY: every x86-64 CPU has SSE.
        let a = unsafe { _mm_set_ss(a) };
        // SAFETY: as above.
        let r = unsafe { _mm_rcp_ss(a) };
        // SAFETY: as above.
        unsafe { _mm_cvtss_f32(r) }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        1.0 / a
    }
}

/// comp_func_SourceOver_rgb64 for a source that is neither opaque nor transparent:
/// `s + d * (65535 - alpha of s) / 65535`.
fn over64(s: Px64, d: Px64) -> Px64 {
    let ia = 65535 - s[3];
    let mul = |v: u32| {
        let x = v * ia;
        (x + (x >> 16) + 0x8000) >> 16
    };
    [0, 1, 2, 3].map(|c| (s[c] + mul(d[c])) & 0xffff)
}

/// QWebpHandler::read's state between frames.
struct WebpPlay {
    file: BufReader<File>,
    w: usize,
    /// QWebpHandler's `m_composited`: Format_ARGB32, not premultiplied.
    canvas: Vec<Px>,
    /// The rectangle of the frame before, when it is cleared before this one is drawn.
    clear: Option<[usize; 4]>,
}

/// The pixels QPainter hands a blend function at once (qdrawhelper's BufferSize), and those
/// the AVX2 conversion looks at together.
const SPAN: usize = 2048;
const GROUP: usize = 8;

impl WebpPlay {
    fn new(file: BufReader<File>, (w, h): (usize, usize), scan: &WebpScan) -> WebpPlay {
        // A file with alpha starts transparent, one without at the background colour.
        let fill = if scan.alpha { [0; 4] } else { scan.bg };
        WebpPlay {
            file,
            w,
            canvas: vec![fill; w * h],
            clear: None,
        }
    }

    /// The canvas as a frame: premultiplied, as a QImage becomes when it is scaled or shown.
    fn shown(&self) -> Canvas {
        let px = self.canvas.iter().map(|p| match p[3] {
            255 => *p,
            _ => premultiply(*p),
        });
        Canvas::from_px(self.w, self.canvas.len() / self.w, px.collect())
    }

    /// Draws frame `n` of the file on the canvas, as QWebpHandler::read does.
    fn read(&mut self, scan: &WebpScan, n: usize) -> Option<()> {
        let f = scan.frames.get(n)?;
        let [left, top, width, height] = f.rect;
        let mut chunks = vec![0u8; f.len];
        self.file.seek(SeekFrom::Start(f.at)).ok()?;
        self.file.read_exact(&mut chunks).ok()?;
        let still = std::io::Cursor::new(webp_still(&chunks, (width, height))?);
        let mut dec = image_webp::WebPDecoder::new(still).ok()?;
        if dec.dimensions() != (width as u32, height as u32) {
            return None;
        }
        // WebPIterator::has_alpha: the frame has an ALPH chunk or a lossless image with alpha.
        let alpha = dec.has_alpha();
        let mut px = vec![0u8; dec.output_buffer_size()?];
        dec.read_image(&mut px).ok()?;
        let sw = self.w;
        let cleared = self.clear.take();
        if let Some([l, t, w, h]) = cleared {
            for y in t..t + h {
                self.canvas[y * sw + l..y * sw + l + w].fill([0; 4]);
            }
        }
        let n = if alpha { 4 } else { 3 };
        let src = |s: &[u8]| -> Px { [s[0], s[1], s[2], if n == 4 { s[3] } else { 255 }] };
        for (y, row) in px.chunks_exact(width * n).enumerate() {
            let line = &mut self.canvas[(top + y) * sw + left..(top + y) * sw + left + width];
            if !scan.alpha && cleared.is_some() {
                // In a file without the alpha flag QWebpHandler leaves its painter in
                // CompositionMode_Clear after it cleared the frame before: this frame's
                // rectangle is cleared too, not drawn.
                line.fill([0; 4]);
            } else if !scan.alpha {
                // The frame decodes to Format_RGB32: every pixel is opaque.
                for (d, s) in line.iter_mut().zip(row.chunks_exact(n)) {
                    *d = [s[0], s[1], s[2], 255];
                }
            } else if !f.blend {
                // CompositionMode_Source.
                for (d, s) in line.iter_mut().zip(row.chunks_exact(n)) {
                    let s = src(s);
                    *d = if s[3] == 255 {
                        s
                    } else {
                        store64(fetch64(s, false))
                    };
                }
            } else {
                // CompositionMode_SourceOver: every pixel of the rectangle is read from the
                // canvas and written back, drawn over or not.
                for (line, row) in line.chunks_mut(SPAN).zip(row.chunks(SPAN * n)) {
                    for (line, row) in line.chunks_mut(GROUP).zip(row.chunks(GROUP * n)) {
                        let opaque = line.iter().all(|d| d[3] == 255);
                        for (d, s) in line.iter_mut().zip(row.chunks_exact(n)) {
                            let s = src(s);
                            *d = match s[3] {
                                255 => s,
                                0 => store64(fetch64(*d, opaque)),
                                _ => store64(over64(fetch64(s, false), fetch64(*d, opaque))),
                            };
                        }
                    }
                }
            }
        }
        self.clear = (alpha && f.dispose).then_some(f.rect);
        Some(())
    }
}

/// The image chunks of an ANMF frame (`VP8 `, `VP8L`, or `ALPH` and `VP8 `) as a WebP file
/// of their own.
fn webp_still(chunks: &[u8], (w, h): (usize, usize)) -> Option<Vec<u8>> {
    let (mut alph, mut image) = (None, None);
    let mut at = 0;
    while let Some(head) = chunks.get(at..at + 8) {
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let chunk = chunks.get(at..at + 8 + len)?;
        match &head[..4] {
            b"ALPH" => alph = Some(chunk),
            b"VP8 " | b"VP8L" => {
                image = Some(chunk);
                break;
            }
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    let image = image?;
    let mut body = b"WEBP".to_vec();
    let mut push = |chunk: &[u8]| {
        body.extend(chunk);
        body.resize(body.len() + (chunk.len() & 1), 0);
    };
    if let Some(alph) = alph.filter(|_| image.starts_with(b"VP8 ")) {
        let (w, h) = ((w - 1).to_le_bytes(), (h - 1).to_le_bytes());
        push(&[b"VP8X", &[10, 0, 0, 0, 0x10, 0, 0, 0][..], &w[..3], &h[..3]].concat());
        push(alph);
    }
    push(image);
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    Some(out)
}
