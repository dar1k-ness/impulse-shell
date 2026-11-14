//! QImage::scaled to an empty size gives a null image (ColorQuantizer `rescaleSize` in (0, 1)).

use qsg::image::{bilinear_scale, fit_long_side, smooth_scale};
use qsg::raster::Canvas;

#[test]
fn fit_long_side_below_one_is_empty() {
    assert_eq!(fit_long_side(400, 300, 0.5), (0, 0));
    assert_eq!(fit_long_side(300, 400, 0.99), (0, 0));
    assert_eq!(fit_long_side(400, 1, 10.0), (10, 1));
}

#[test]
fn scaling_to_an_empty_size_is_empty() {
    let c = Canvas::new(4, 4, [255, 0, 0, 255]);
    for (w, h) in [(0, 1), (1, 0), (0, 0)] {
        let s = smooth_scale(&c, w, h, false);
        assert_eq!((s.w, s.h, s.px.len()), (w, h, 0));
        let b = bilinear_scale(&c, w, h);
        assert_eq!((b.w, b.h, b.px.len()), (w, h, 0));
    }
}
