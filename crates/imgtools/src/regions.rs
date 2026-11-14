//! Port of `scripts/images/find_regions.py` (OpenCV ximgproc selective search + NMS), in the
//! mode RegionSelection.qml calls it (`--hyprctl --image P --max-width W --max-height H`) and
//! no other: the script's remaining options (`--k`, `--sigma`, `--resize-factor`, ...) keep
//! their defaults here and are usage errors on the command line, since nothing in the shell
//! can pass them and a result is only promised equal to the script's for what is tested.

#[path = "ss_cv.rs"]
pub mod ss_cv;
#[path = "ss_graph.rs"]
pub mod ss_graph;
#[path = "ss_search.rs"]
pub mod ss_search;
#[path = "ss_sort.rs"]
pub mod ss_sort;

/// What the shell asks for: the image and the size a region must stay under.
#[derive(Clone, Debug, Default)]
pub struct RegionArgs {
    pub image: String,
    pub max_width: i64,
    pub max_height: i64,
}

/// The script's `--min-width` and `--min-height` defaults, the only ones the shell runs it
/// with.
const MIN_WIDTH: i64 = 200;
const MIN_HEIGHT: i64 = 100;

/// One region as `{x, y, width, height}`, in the script's output order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

fn iou(a: &Region, b: &Region) -> f64 {
    let xa = a.x.max(b.x);
    let ya = a.y.max(b.y);
    let xb = (a.x + a.width).min(b.x + b.width);
    let yb = (a.y + a.height).min(b.y + b.height);
    let inter = 0.max(xb - xa) * 0.max(yb - ya);
    let union = a.width * a.height + b.width * b.height - inter;
    if union > 0 {
        inter as f64 / union as f64
    } else {
        0.0
    }
}

fn non_max_suppression(mut regions: Vec<Region>, th: f64) -> Vec<Region> {
    regions.sort_by_key(|r| std::cmp::Reverse(r.width * r.height));
    let mut keep = Vec::new();
    while !regions.is_empty() {
        let cur = regions.remove(0);
        regions.retain(|r| iou(&cur, r) < th);
        keep.push(cur);
    }
    keep
}

/// The script's regions as it computes them on a CPU where OpenCV runs its AVX2 filters
/// (`fma`) or on one where it does not. `None` where the script exits 1 (image not loadable,
/// or the resized image is empty).
pub fn find_regions_as(a: &RegionArgs, fma: bool) -> Option<Vec<Region>> {
    let img = crate::cv::imread_bgr(&a.image)?;
    let (px, w, h) = ss_cv::resize_area_tenth(&img.px, img.w, img.h)?;
    let (ow, oh) = (img.w, img.h);
    let rects = ss_search::selective_search_fast(&px, w, h, fma);
    let rects: Vec<Region> = rects
        .iter()
        .map(|r| Region {
            x: r.x as i64,
            y: r.y as i64,
            width: r.w as i64,
            height: r.h as i64,
        })
        .collect();
    Some(postprocess(&rects, ow as i64, oh as i64, a))
}

/// The script's steps after `ss.process()`: scale back, drop the full-image rect, size filters,
/// NMS (IoU 0.7).
fn postprocess(rects: &[Region], ow: i64, oh: i64, a: &RegionArgs) -> Vec<Region> {
    let mut regions = Vec::new();
    for r in rects {
        let x = (r.x as f64 / ss_cv::RESIZE_FACTOR) as i64;
        let y = (r.y as f64 / ss_cv::RESIZE_FACTOR) as i64;
        let rw = (r.width as f64 / ss_cv::RESIZE_FACTOR) as i64;
        let rh = (r.height as f64 / ss_cv::RESIZE_FACTOR) as i64;
        if rw == ow && rh == oh && x == 0 && y == 0 {
            continue;
        }
        if rw > MIN_WIDTH && rh > MIN_HEIGHT && rw < a.max_width && rh < a.max_height {
            regions.push(Region {
                x,
                y,
                width: rw,
                height: rh,
            });
        }
    }
    non_max_suppression(regions, 0.7)
}

/// `json.dumps` of the `--hyprctl` list (no trailing newline).
pub fn to_hyprctl_json(rs: &[Region]) -> String {
    let items: Vec<String> = rs
        .iter()
        .map(|r| {
            format!(
                "{{\"at\": [{}, {}], \"size\": [{}, {}]}}",
                r.x, r.y, r.width, r.height
            )
        })
        .collect();
    format!("[{}]", items.join(", "))
}

/// Whether argparse reads `s` as a value: not as an option, which is what it takes a word
/// that starts with `-` for unless it is a negative number (`-5`, `-.5`, `-1.5`).
fn is_value(s: &str) -> bool {
    let Some(n) = s.strip_prefix('-') else {
        return true;
    };
    let digits = |d: &str| d.bytes().all(|b| b.is_ascii_digit());
    match n.split_once('.') {
        None => digits(n),
        Some((i, f)) => digits(i) && digits(f) && !f.is_empty(),
    }
}

/// The call RegionSelection.qml makes, `--hyprctl --image P --max-width W --max-height H` in
/// any order; `None` for anything else.
fn parse(args: &[String]) -> Option<RegionArgs> {
    let (mut image, mut max_width, mut max_height, mut hyprctl) = (None, None, None, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "--hyprctl" {
            hyprctl = true;
            continue;
        }
        let v = it.next().filter(|v| is_value(v))?;
        match arg.as_str() {
            "--image" => image = Some(v.clone()),
            "--max-width" => max_width = Some(v.parse().ok()?),
            "--max-height" => max_height = Some(v.parse().ok()?),
            _ => return None,
        }
    }
    hyprctl.then_some(RegionArgs {
        image: image?,
        max_width: max_width?,
        max_height: max_height?,
    })
}

/// The script's stdout and exit code for the arguments RegionSelection.qml passes it. Exit 2
/// with empty stdout, the script's answer to a usage error, for a value it would not take and
/// for every other call: an option of the script that is not ported, an abbreviated or
/// `--opt=value` spelling, one of the four arguments missing.
pub fn cli(args: &[String]) -> (String, i32) {
    cli_as(args, ss_cv::filter_fma())
}

/// [`cli`] for the CPU kind `fma`, see [`find_regions_as`].
pub fn cli_as(args: &[String], fma: bool) -> (String, i32) {
    let Some(a) = parse(args) else {
        return (String::new(), 2);
    };
    match crate::pool::scoped(|| find_regions_as(&a, fma)) {
        None => (String::new(), 1),
        Some(rs) => (to_hyprctl_json(&rs) + "\n", 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i64, y: i64, width: i64, height: i64) -> Region {
        Region {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn hyprctl_json_format() {
        assert_eq!(to_hyprctl_json(&[]), "[]");
        assert_eq!(
            to_hyprctl_json(&[r(0, 10, 200, 300), r(-1, 2, 3, 4)]),
            r#"[{"at": [0, 10], "size": [200, 300]}, {"at": [-1, 2], "size": [3, 4]}]"#
        );
    }

    #[test]
    fn nms_keeps_larger_and_stable_order() {
        // Equal areas keep input order (Python's stable sort); IoU >= 0.7 is suppressed.
        let a = r(0, 0, 100, 100);
        let b = r(5, 5, 100, 100); // IoU 0.82 with a
        let c = r(300, 0, 100, 100);
        let d = r(0, 0, 100, 70); // IoU exactly 0.7 with a
        let e = r(0, 0, 100, 71);
        assert_eq!(non_max_suppression(vec![c, a, b, d], 0.7), vec![c, a]);
        assert_eq!(non_max_suppression(vec![d, b, a], 0.7), vec![b, d]);
        assert_eq!(non_max_suppression(vec![e, a], 0.7), vec![a]);
        assert_eq!(non_max_suppression(vec![], 0.7), vec![]);
    }

    #[test]
    fn filters_are_strict_and_scale_truncates() {
        let a = RegionArgs {
            max_width: 500,
            max_height: 400,
            ..Default::default()
        };
        // Rects of the image resized by 0.1; the comments give them scaled back.
        let rects = [
            r(0, 0, 20, 20),   // 200x200: width not > 200
            r(10, 10, 30, 20), // 300x200: kept
            r(10, 10, 50, 20), // 500x200: width not < 500
            r(20, 20, 40, 40), // 400x400: height not < 400
            r(70, 30, 30, 10), // 300x100: height not > 100
            r(30, 0, 40, 30),  // 400x300: kept, larger so first
        ];
        assert_eq!(
            postprocess(&rects, 1000, 1000, &a),
            vec![r(300, 0, 400, 300), r(100, 100, 300, 200)]
        );
    }

    #[test]
    fn a_word_after_an_option_is_a_value_as_argparse_reads_it() {
        for v in ["", "960", "/tmp/a.png", "-", "-5", "-.5", "-1.5", "a-b"] {
            assert!(is_value(v), "{v:?}");
        }
        for v in [
            "--hyprctl",
            "-i",
            "-1e5",
            "-5.",
            "-inf",
            "-/tmp/a.png",
            "--5",
        ] {
            assert!(!is_value(v), "{v:?}");
        }
    }

    #[test]
    fn full_image_rect_is_excluded() {
        let a = RegionArgs {
            max_width: 2000,
            max_height: 2000,
            ..Default::default()
        };
        let rects = [r(0, 0, 100, 80), r(1, 0, 100, 80), r(0, 0, 50, 40)];
        assert_eq!(
            postprocess(&rects, 1000, 800, &a),
            vec![r(10, 0, 1000, 800), r(0, 0, 500, 400)]
        );
    }
}
