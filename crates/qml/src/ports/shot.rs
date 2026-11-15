//! The work behind `Impulse/Screenshot` ([`crate::screenshot`]): capture through
//! wlr-screencopy instead of `grim`, crop
//! with `image` instead of `magick`, clipboard through `wl-clipboard-rs` instead of
//! `wl-copy`, image-search upload over `crate::http` instead of `curl | jq`. The annotation tool
//! (`swappy`/`satty`), `tesseract` and `xdg-open` still run as programs.

use super::Io;
use std::io::Write;
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard};

/// Region selector screenshots (`TempScreenshotProcess`) not yet handed to a crop job, and those
/// whose selector closed while the capture was still being written. Closing the selector
/// without a snip (Esc, click-away) deletes them; the original left them in the screenshot dir.
#[derive(Default)]
struct Temps {
    pending: std::collections::BTreeSet<String>,
    discarded: std::collections::BTreeSet<String>,
}

static TEMPS: Mutex<Temps> = Mutex::new(Temps {
    pending: std::collections::BTreeSet::new(),
    discarded: std::collections::BTreeSet::new(),
});

fn temps() -> MutexGuard<'static, Temps> {
    TEMPS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A capture into `path` is starting.
pub fn temp_started(path: &str) {
    let mut t = temps();
    t.discarded.remove(path);
    t.pending.insert(path.to_string());
}

/// The capture into `path` is written: dropped at once when its selector already closed.
pub fn temp_written(path: &str) {
    if temps().discarded.remove(path) {
        let _ = std::fs::remove_file(path);
    }
}

/// A crop job takes `path` over (and removes it when done).
pub fn temp_claimed(path: &str) {
    temps().pending.remove(path);
}

/// The selector closed: every unclaimed capture goes.
pub fn temps_discard() {
    let paths = std::mem::take(&mut temps().pending);
    for p in paths {
        let _ = std::fs::remove_file(&p);
        temps().discarded.insert(p);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum After {
    /// To the clipboard, and saved as `<dir>/screenshot-<date>.png` when a dir is set.
    Copy(Option<String>),
    /// Into `<tool> -f -`.
    Edit(String),
    /// Uploaded; `<base><url>` opened.
    Search { endpoint: String, base: String },
    /// `tesseract` with every installed language, text to the clipboard.
    Ocr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// `magick <path> -crop WxH+X+Y +repage …`, then `after`, then the file is removed.
    Crop {
        path: String,
        rect: (i64, i64, i64, i64),
        after: After,
    },
    /// `mkdir -p <dir> && grim -o <output> <path>`.
    Temp {
        dir: String,
        output: String,
        path: String,
    },
    /// `grim - | wl-copy`.
    FullCopy,
}

/// `-crop WxH+X+Y +repage`: the part of the rectangle inside the image (ImageMagick gives a
/// 1x1 image of its transparent white background for a rectangle entirely outside, with a
/// warning).
pub fn crop(img: &image::RgbaImage, (x, y, w, h): (i64, i64, i64, i64)) -> image::RgbaImage {
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    let (x0, y0) = (x.clamp(0, iw), y.clamp(0, ih));
    let (x1, y1) = (
        if w <= 0 { iw } else { (x + w).clamp(0, iw) },
        if h <= 0 { ih } else { (y + h).clamp(0, ih) },
    );
    if x1 <= x0 || y1 <= y0 {
        return image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 0]));
    }
    image::imageops::crop_imm(
        img,
        x0 as u32,
        y0 as u32,
        (x1 - x0) as u32,
        (y1 - y0) as u32,
    )
    .to_image()
}

/// `curl -sF files[]=@<file> <endpoint> | jq -r '.files[0].url'`.
pub fn upload(endpoint: &str, file: &str) -> String {
    let Ok(data) = std::fs::read(file) else {
        return String::new();
    };
    let name = std::path::Path::new(file)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let boundary = format!("------------------------{:016x}", rand_u64());
    let mut body = Vec::with_capacity(data.len() + 256);
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"files[]\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    body.extend_from_slice(&data);
    let _ = write!(body, "\r\n--{boundary}--\r\n");
    // Through crate::http: curl's proxy rules and happy eyeballs (plain ureq stalled for
    // minutes on an unreachable IPv6 route), and an error status still yields its body.
    let headers = [(
        "Content-Type".to_string(),
        format!("multipart/form-data; boundary={boundary}"),
    )];
    let Ok(r) = crate::http::send("POST", endpoint, &headers, Some(&body), Default::default())
    else {
        return String::new();
    };
    let Ok(text) = r.text() else {
        return String::new();
    };
    jq_files0_url(&text)
}

/// The image-search page for an uploaded screenshot. The original opened `{base}` or `{base}null` when
/// the upload failed (a search for nothing); `None` then, so the job fails instead.
pub fn search_url(base: &str, hosted: &str) -> Option<String> {
    (!hosted.is_empty() && hosted != "null").then(|| format!("{base}{hosted}"))
}

/// `jq -r '.files[0].url'` of a response ("" when it isn't JSON, `null` when absent).
pub fn jq_files0_url(text: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return String::new();
    };
    match v
        .get("files")
        .and_then(|f| f.get(0))
        .and_then(|f| f.get("url"))
    {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => "null".into(),
        Some(o) => o.to_string(),
    }
}

fn rand_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    h.finish()
}

/// `tesseract --list-langs | awk 'NR>1{print $1}' | tr '\n' '+' | sed 's/\+$/\n/'`.
pub fn langs_from(list: &str) -> String {
    list.lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next())
        .collect::<Vec<_>>()
        .join("+")
}

pub fn tesseract_langs(stop: &Arc<crate::svc::Stop>) -> Option<String> {
    let o = crate::child::run(
        workpool::userenv::command("tesseract")
            .arg("--list-langs")
            .stdin(Stdio::null()),
        stop,
    )
    .ok()?;
    Some(langs_from(&String::from_utf8_lossy(&o.out)))
}

fn ocr(path: &str, stop: &Arc<crate::svc::Stop>) -> Vec<u8> {
    let langs = tesseract_langs(stop).unwrap_or_default();
    let mut c = workpool::userenv::command("tesseract");
    c.args([path, "stdout", "-l"]);
    if !langs.is_empty() {
        c.arg(langs);
    }
    crate::child::run(c.stdin(Stdio::null()), stop)
        .map(|o| o.out)
        .unwrap_or_default()
}

/// The capture into `path`: the original's `/tmp/impulse-shell/media/screenshot` is shared, so its
/// directory must be ours (made private) and the file is renamed into place, never written
/// through a symlink planted there.
pub fn save_temp(path: &str, png: &[u8]) -> bool {
    crate::privfs::Dir::parent_of(path.as_ref(), true)
        .is_ok_and(|(d, name)| d.write(name, png).is_ok())
}

impl Job {
    pub fn run(self, io: &Io) -> i32 {
        match self {
            Job::Temp { dir, output, path } => {
                if crate::privfs::Dir::open(dir.as_ref(), true).is_err() {
                    return 1;
                }
                let code = match super::wl::capture_or_stop(Some(&output), &io.stop) {
                    Ok(img) => !save_temp(&path, &super::wl::png(&img)) as i32,
                    Err(_) if io.stopped() => 143,
                    Err(e) => {
                        elog::eprintln!("[screenshot] {e}");
                        1
                    }
                };
                temp_written(&path);
                code
            }
            Job::FullCopy => match super::wl::capture_or_stop(None, &io.stop) {
                Ok(img) => match super::wl::copy(super::wl::png(&img), Some("image/png"), &io.stop)
                {
                    Ok(_) => 0,
                    Err(_) if io.stopped() => 143,
                    Err(_) => 1,
                },
                Err(_) if io.stopped() => 143,
                Err(e) => {
                    elog::eprintln!("[screenshot] {e}");
                    1
                }
            },
            Job::Crop { path, rect, after } => {
                // The temporary screenshot goes on every outcome (the original's `&& rm` chain left it
                // behind after a failed upload, tesseract or crop).
                struct Temp<'a>(&'a str);
                impl Drop for Temp<'_> {
                    fn drop(&mut self) {
                        let _ = std::fs::remove_file(self.0);
                    }
                }
                let _temp = Temp(&path);
                // Sniff the format like magick: the temporary screenshot has no extension.
                let Ok(img) = std::fs::read(&path)
                    .map_err(|_| ())
                    .and_then(|d| imgtools::limit::image(&d).map_err(|_| ()))
                else {
                    return 1;
                };
                let png = super::wl::png(&crop(&img.to_rgba8(), rect));
                match after {
                    After::Copy(dir) => {
                        if let Some(d) = dir {
                            if std::fs::create_dir_all(&d).is_err() {
                                return 1;
                            }
                            let p = format!("{d}/screenshot-{}.png", super::date_stamp());
                            if std::fs::write(&p, &png).is_err() {
                                return 1;
                            }
                        }
                        if super::wl::copy(png, Some("image/png"), &io.stop).is_err() {
                            return if io.stopped() { 143 } else { 1 };
                        }
                        0
                    }
                    After::Edit(tool) => {
                        let Ok(mut ch) = crate::child::spawn(
                            workpool::userenv::command(&tool)
                                .args(["-f", "-"])
                                .stdin(Stdio::piped())
                                .stdout(Stdio::null())
                                .stderr(Stdio::null()),
                        ) else {
                            return 127;
                        };
                        if let Some(mut i) = ch.child_mut().stdin.take() {
                            let _ = i.write_all(&png);
                        }
                        if !ch.wait().is_ok_and(|s| s.success()) {
                            return 1;
                        }
                        0
                    }
                    After::Search { endpoint, base } => {
                        if std::fs::write(&path, &png).is_err() {
                            return 1;
                        }
                        let Some(url) = search_url(&base, &upload(&endpoint, &path)) else {
                            return 1;
                        };
                        match super::status(
                            workpool::userenv::command("xdg-open").arg(url),
                            &io.stop,
                        ) {
                            Some(s) if s.success() => 0,
                            _ if io.stopped() => 143,
                            _ => 1,
                        }
                    }
                    After::Ocr => {
                        if std::fs::write(&path, &png).is_err() {
                            return 1;
                        }
                        let text = ocr(&path, &io.stop);
                        if io.stopped() {
                            return 143;
                        }
                        if super::wl::copy(text, None, &io.stop).is_err() {
                            return if io.stopped() { 143 } else { 1 };
                        }
                        0
                    }
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod upload_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};

    /// One-request server answering `status` with `body`; returns its URL.
    pub(crate) fn serve(status: u16, body: &'static str) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/upload", l.local_addr().unwrap());
        std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length: ") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut b = vec![0; len];
            let _ = r.read_exact(&mut b);
            let mut s = s;
            let _ = write!(
                s,
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        url
    }

    #[test]
    fn upload_reads_the_reply_like_curl_and_jq() {
        let f = std::env::temp_dir().join(format!("shot-upload-{}.png", std::process::id()));
        std::fs::write(&f, b"png").unwrap();
        let f = f.to_str().unwrap();
        let ok = r#"{"files":[{"url":"https://host/x.png"}]}"#;
        assert_eq!(upload(&serve(200, ok), f), "https://host/x.png");
        // curl without -f hands an error reply's body to jq too.
        assert_eq!(upload(&serve(413, ok), f), "https://host/x.png");
        assert_eq!(upload(&serve(500, "<h1>down</h1>"), f), "");
        assert_eq!(upload(&serve(200, "{}"), f), "null");
        assert_eq!(upload("http://127.0.0.1:9/upload", f), "");
        let _ = std::fs::remove_file(f);
    }

    #[test]
    fn failed_upload_opens_no_search_page() {
        let base = "https://lens.google.com/uploadbyurl?url=";
        assert_eq!(search_url(base, ""), None);
        assert_eq!(search_url(base, "null"), None);
        assert_eq!(
            search_url(base, "https://host/x.png").as_deref(),
            Some("https://lens.google.com/uploadbyurl?url=https://host/x.png")
        );
    }
}
