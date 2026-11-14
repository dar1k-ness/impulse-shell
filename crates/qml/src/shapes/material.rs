//! The builders of the 35 Material shapes, written from the original's former `material-shapes.js`.

use super::geom::{
    Matrix, P, Polygon, Rounding, UNROUNDED, circle, from_num_vertices, from_vertices, p, r,
    rectangle, star,
};

pub const R15: Rounding = r(0.15, 0.0);
pub const R20: Rounding = r(0.2, 0.0);
pub const R30: Rounding = r(0.3, 0.0);
pub const R50: Rounding = r(0.5, 0.0);
pub const R100: Rounding = r(1.0, 0.0);

/// The exported matrices, in `material-shapes.js` order.
pub fn matrices() -> [(&'static str, Matrix); 13] {
    let rot = |d| Matrix::IDENTITY.rotate_z(d);
    [
        ("rotateNeg30", rot(-30.0)),
        ("rotateNeg45", rot(-45.0)),
        ("rotateNeg90", rot(-90.0)),
        ("rotateNeg135", rot(-135.0)),
        ("rotate30", rot(30.0)),
        ("rotate45", rot(45.0)),
        ("rotate60", rot(60.0)),
        ("rotate90", rot(90.0)),
        ("rotate120", rot(120.0)),
        ("rotate135", rot(135.0)),
        ("rotate180", rot(180.0)),
        ("rotate28th", rot(360.0 / 28.0)),
        ("rotateNeg16th", rot(-360.0 / 16.0)),
    ]
}

fn m(name: &str) -> Matrix {
    matrices().into_iter().find(|(n, _)| *n == name).unwrap().1
}

/// `doRepeat`.
pub fn do_repeat(
    points: &[(P, Rounding)],
    reps: f64,
    center: P,
    mirroring: bool,
) -> Vec<(P, Rounding)> {
    let mut out = Vec::new();
    if mirroring {
        let angles: Vec<f64> = points
            .iter()
            .map(|(o, _)| p(o.x - center.x, o.y - center.y).angle_degrees())
            .collect();
        let distances: Vec<f64> = points
            .iter()
            .map(|(o, _)| p(o.x - center.x, o.y - center.y).distance())
            .collect();
        let actual_reps = reps * 2.0;
        let section = 360.0 / actual_reps;
        let mut it = 0.0;
        while it < actual_reps {
            let even = it % 2.0 == 0.0;
            for index in 0..points.len() {
                let i = if even {
                    index
                } else {
                    points.len() - 1 - index
                };
                if i > 0 || even {
                    let base = angles[i];
                    let angle = it * section + if even { base } else { 2.0 * angles[0] - base };
                    let dist = distances[i];
                    let rad = angle * std::f64::consts::PI / 180.0;
                    out.push((
                        p(center.x + dist * rad.cos(), center.y + dist * rad.sin()),
                        points[i].1,
                    ));
                }
            }
            it += 1.0;
        }
    } else {
        let np = points.len();
        let mut i = 0usize;
        while (i as f64) < np as f64 * reps {
            let (o, rr) = points[i % np];
            let angle = ((i / np) as f64) * 360.0 / reps;
            out.push((o.rotate_degrees(angle, center), rr));
            i += 1;
        }
    }
    out
}

/// `customPolygon`.
pub fn custom_polygon(pnr: &[(P, Rounding)], reps: f64, center: P, mirroring: bool) -> Polygon {
    let pts = do_repeat(pnr, reps, center, mirroring);
    let mut vertices = Vec::with_capacity(pts.len() * 2);
    for (o, _) in &pts {
        vertices.push(o.x);
        vertices.push(o.y);
    }
    let per: Vec<Option<Rounding>> = pts.iter().map(|&(_, rr)| Some(rr)).collect();
    from_vertices(&vertices, UNROUNDED, Some(&per), center.x, center.y)
}

fn cp(pnr: &[(P, Rounding)], reps: f64) -> Polygon {
    custom_polygon(pnr, reps, p(0.5, 0.5), false)
}

const U: Rounding = UNROUNDED;

fn pr(x: f64, y: f64, rr: Rounding) -> (P, Rounding) {
    (p(x, y), rr)
}

pub fn build(name: &str) -> Option<Polygon> {
    Some(match name {
        "circle" => circle(10.0, 1.0, 0.0, 0.0).map(&m("rotate45")).normalized(),
        "square" => rectangle(1.0, 1.0, R30, None, 0.0, 0.0).normalized(),
        "slanted" => cp(
            &[
                pr(0.926, 0.970, r(0.189, 0.811)),
                pr(-0.021, 0.967, r(0.187, 0.057)),
            ],
            2.0,
        )
        .normalized(),
        "arch" => rectangle(
            1.0,
            1.0,
            U,
            Some(&[Some(R20), Some(R20), Some(R100), Some(R100)]),
            0.0,
            0.0,
        )
        .normalized(),
        "fan" => cp(
            &[
                pr(1.004, 1.000, r(0.148, 0.417)),
                pr(0.000, 1.000, r(0.151, 0.0)),
                pr(0.000, -0.003, r(0.148, 0.0)),
                pr(0.978, 0.020, r(0.803, 0.0)),
            ],
            1.0,
        )
        .normalized(),
        "arrow" => cp(
            &[
                pr(1.225, 1.060, r(0.211, 0.0)),
                pr(0.500, 0.892, r(0.313, 0.0)),
                pr(-0.216, 1.050, r(0.207, 0.0)),
                pr(0.499, -0.160, r(0.215, 1.000)),
            ],
            1.0,
        )
        .normalized(),
        "semiCircle" => rectangle(
            1.6,
            1.0,
            U,
            Some(&[Some(R20), Some(R20), Some(R100), Some(R100)]),
            0.0,
            0.0,
        )
        .normalized(),
        "oval" => circle(8.0, 1.0, 0.0, 0.0)
            .map(&m("rotateNeg90"))
            .map(&Matrix::IDENTITY.scale(1.0, 0.64, 1.0))
            .map(&m("rotate135"))
            .normalized(),
        "pill" => cp(
            &[
                pr(0.428, -0.001, r(0.426, 0.0)),
                pr(0.961, 0.039, r(0.426, 0.0)),
                pr(1.001, 0.428, U),
                pr(1.000, 0.609, r(1.000, 0.0)),
            ],
            2.0,
        )
        .map(&m("rotate180"))
        .normalized(),
        "triangle" => from_num_vertices(3.0, 1.0, 0.5, 0.5, R20, None)
            .map(&m("rotate30"))
            .normalized(),
        "diamond" => cp(
            &[
                pr(0.500, 1.096, r(0.151, 0.524)),
                pr(0.040, 0.500, r(0.159, 0.0)),
            ],
            2.0,
        )
        .normalized(),
        "clamShell" => cp(
            &[
                pr(0.829, 0.841, r(0.159, 0.0)),
                pr(0.171, 0.841, r(0.159, 0.0)),
                pr(-0.020, 0.500, r(0.140, 0.0)),
            ],
            2.0,
        )
        .normalized(),
        "pentagon" => cp(
            &[
                pr(0.828, 0.970, r(0.169, 0.0)),
                pr(0.172, 0.970, r(0.169, 0.0)),
                pr(-0.030, 0.365, r(0.164, 0.0)),
                pr(0.500, -0.009, r(0.172, 0.0)),
                pr(1.030, 0.365, r(0.164, 0.0)),
            ],
            1.0,
        )
        .normalized(),
        "gem" => cp(
            &[
                pr(1.005, 0.792, r(0.208, 0.0)),
                pr(0.5, 1.023, r(0.241, 0.778)),
                pr(-0.005, 0.792, r(0.208, 0.0)),
                pr(0.073, 0.258, r(0.228, 0.0)),
                pr(0.5, 0.000, r(0.241, 0.778)),
                pr(0.927, 0.258, r(0.228, 0.0)),
            ],
            1.0,
        )
        .normalized(),
        "sunny" => star(8.0, 1.0, 0.8, R15, None, 0.0, 0.0)
            .map(&m("rotate45"))
            .normalized(),
        "verySunny" => cp(
            &[
                pr(0.500, 1.080, r(0.085, 0.0)),
                pr(0.358, 0.843, r(0.085, 0.0)),
            ],
            8.0,
        )
        .map(&m("rotateNeg45"))
        .normalized(),
        "cookie4" => cp(
            &[
                pr(1.237, 1.236, r(0.258, 0.0)),
                pr(0.500, 0.918, r(0.233, 0.0)),
            ],
            4.0,
        )
        .normalized(),
        "cookie6" => cp(
            &[
                pr(0.723, 0.884, r(0.394, 0.0)),
                pr(0.500, 1.099, r(0.398, 0.0)),
            ],
            6.0,
        )
        .normalized(),
        "cookie7" => {
            let r28 = m("rotate28th");
            star(7.0, 1.0, 0.75, R50, None, 0.0, 0.0)
                .normalized()
                .map(&r28)
                .map(&r28)
                .map(&r28)
                .map(&r28)
                .map(&r28)
                .normalized()
        }
        "cookie9" => star(9.0, 1.0, 0.8, R50, None, 0.0, 0.0)
            .map(&m("rotate30"))
            .normalized(),
        "cookie12" => star(12.0, 1.0, 0.8, R50, None, 0.0, 0.0)
            .map(&m("rotate30"))
            .normalized(),
        "ghostish" => cp(
            &[
                pr(1.000, 1.140, r(0.254, 0.106)),
                pr(0.575, 0.906, r(0.253, 0.0)),
                pr(0.425, 0.906, r(0.253, 0.0)),
                pr(0.000, 1.140, r(0.254, 0.106)),
                pr(0.000, 0.000, r(1.0, 0.0)),
                pr(0.500, 0.000, r(1.0, 0.0)),
                pr(1.000, 0.000, r(1.0, 0.0)),
            ],
            1.0,
        )
        .normalized(),
        "clover4" => cp(
            &[
                pr(1.099, 0.725, r(0.476, 0.0)),
                pr(0.725, 1.099, r(0.476, 0.0)),
                pr(0.500, 0.926, U),
            ],
            4.0,
        )
        .normalized(),
        "clover8" => cp(&[pr(0.758, 1.101, r(0.209, 0.0)), pr(0.500, 0.964, U)], 8.0).normalized(),
        "burst" => cp(
            &[
                pr(0.592, 0.842, r(0.006, 0.0)),
                pr(0.500, 1.006, r(0.006, 0.0)),
            ],
            12.0,
        )
        .map(&m("rotateNeg30"))
        .map(&m("rotateNeg30"))
        .normalized(),
        "softBurst" => cp(
            &[
                pr(0.193, 0.277, r(0.053, 0.0)),
                pr(0.176, 0.055, r(0.053, 0.0)),
            ],
            10.0,
        )
        .map(&m("rotate180"))
        .normalized(),
        "boom" => cp(
            &[
                pr(0.457, 0.296, r(0.007, 0.0)),
                pr(0.500, -0.051, r(0.007, 0.0)),
            ],
            15.0,
        )
        .map(&m("rotate120"))
        .normalized(),
        "softBoom" => cp(
            &[
                pr(0.733, 0.454, U),
                pr(0.839, 0.437, r(0.532, 0.0)),
                pr(0.949, 0.449, r(0.439, 1.000)),
                pr(0.998, 0.478, r(0.174, 0.0)),
                pr(0.998, 0.522, r(0.174, 0.0)),
                pr(0.949, 0.551, r(0.439, 1.000)),
                pr(0.839, 0.563, r(0.532, 0.0)),
                pr(0.733, 0.546, U),
            ],
            16.0,
        )
        .map(&m("rotate45"))
        .map(&m("rotateNeg16th"))
        .normalized(),
        "flower" => cp(
            &[
                pr(0.370, 0.187, U),
                pr(0.416, 0.049, r(0.381, 0.0)),
                pr(0.479, 0.001, r(0.095, 0.0)),
                pr(0.521, 0.001, r(0.095, 0.0)),
                pr(0.584, 0.049, r(0.381, 0.0)),
                pr(0.630, 0.187, U),
            ],
            8.0,
        )
        .map(&m("rotate135"))
        .normalized(),
        "puffy" => cp(
            &[
                pr(1.003, 0.563, r(0.255, 0.0)),
                pr(0.940, 0.656, r(0.126, 0.0)),
                pr(0.881, 0.654, U),
                pr(0.926, 0.711, r(0.660, 0.0)),
                pr(0.914, 0.851, r(0.660, 0.0)),
                pr(0.777, 0.998, r(0.360, 0.0)),
                pr(0.722, 0.872, U),
                pr(0.717, 0.934, r(0.574, 0.0)),
                pr(0.670, 1.035, r(0.426, 0.0)),
                pr(0.545, 1.040, r(0.405, 0.0)),
                pr(0.500, 0.947, U),
                pr(0.500, 1.0 - 0.053, U),
                pr(1.0 - 0.545, 1.0 + 0.040, r(0.405, 0.0)),
                pr(1.0 - 0.670, 1.0 + 0.035, r(0.426, 0.0)),
                pr(1.0 - 0.717, 1.0 - 0.066, r(0.574, 0.0)),
                pr(1.0 - 0.722, 1.0 - 0.128, U),
                pr(1.0 - 0.777, 1.0 - 0.002, r(0.360, 0.0)),
                pr(1.0 - 0.914, 1.0 - 0.149, r(0.660, 0.0)),
                pr(1.0 - 0.926, 1.0 - 0.289, r(0.660, 0.0)),
                pr(1.0 - 0.881, 1.0 - 0.346, U),
                pr(1.0 - 0.940, 1.0 - 0.344, r(0.126, 0.0)),
                pr(1.0 - 1.003, 1.0 - 0.437, r(0.255, 0.0)),
            ],
            2.0,
        )
        .map(&Matrix::IDENTITY.scale(1.0, 0.742, 1.0))
        .normalized(),
        "puffyDiamond" => cp(
            &[
                pr(0.870, 0.130, r(0.146, 0.0)),
                pr(0.818, 0.357, U),
                pr(1.000, 0.332, r(0.853, 0.0)),
                pr(1.000, 1.0 - 0.332, r(0.853, 0.0)),
                pr(0.818, 1.0 - 0.357, U),
            ],
            4.0,
        )
        .map(&m("rotate90"))
        .normalized(),
        "pixelCircle" => cp(
            &[
                pr(1.000, 0.704, U),
                pr(0.926, 0.704, U),
                pr(0.926, 0.852, U),
                pr(0.843, 0.852, U),
                pr(0.843, 0.935, U),
                pr(0.704, 0.935, U),
                pr(0.704, 1.000, U),
                pr(0.500, 1.000, U),
                pr(1.0 - 0.704, 1.000, U),
                pr(1.0 - 0.704, 0.935, U),
                pr(1.0 - 0.843, 0.935, U),
                pr(1.0 - 0.843, 0.852, U),
                pr(1.0 - 0.926, 0.852, U),
                pr(1.0 - 0.926, 0.704, U),
                pr(0.0, 0.704, U),
            ],
            2.0,
        )
        .normalized(),
        "pixelTriangle" => cp(
            &[
                pr(0.888, 1.0 - 0.439, U),
                pr(0.789, 1.0 - 0.439, U),
                pr(0.789, 1.0 - 0.344, U),
                pr(0.675, 1.0 - 0.344, U),
                pr(0.674, 1.0 - 0.265, U),
                pr(0.560, 1.0 - 0.265, U),
                pr(0.560, 1.0 - 0.170, U),
                pr(0.421, 1.0 - 0.170, U),
                pr(0.421, 1.0 - 0.087, U),
                pr(0.287, 1.0 - 0.087, U),
                pr(0.287, 1.0 - 0.000, U),
                pr(0.113, 1.0 - 0.000, U),
                pr(0.110, 0.500, U),
                pr(0.113, 0.000, U),
                pr(0.287, 0.000, U),
                pr(0.287, 0.087, U),
                pr(0.421, 0.087, U),
                pr(0.421, 0.170, U),
                pr(0.560, 0.170, U),
                pr(0.560, 0.265, U),
                pr(0.674, 0.265, U),
                pr(0.675, 0.344, U),
                pr(0.789, 0.344, U),
                pr(0.789, 0.439, U),
                pr(0.888, 0.439, U),
            ],
            1.0,
        )
        .normalized(),
        "bun" => cp(
            &[
                pr(0.796, 0.500, U),
                pr(0.853, 0.518, R100),
                pr(0.992, 0.631, R100),
                pr(0.968, 1.000, R100),
                pr(0.032, 1.0 - 0.000, R100),
                pr(0.008, 1.0 - 0.369, R100),
                pr(0.147, 1.0 - 0.482, R100),
                pr(0.204, 1.0 - 0.500, U),
            ],
            2.0,
        )
        .normalized(),
        "heart" => cp(
            &[
                pr(0.782, 0.611, U),
                pr(0.499, 0.946, r(0.000, 0.0)),
                pr(0.2175, 0.611, U),
                pr(-0.064, 0.276, r(1.000, 0.0)),
                pr(0.208, -0.066, r(0.958, 0.0)),
                pr(0.500, 0.268, r(0.016, 0.0)),
                pr(0.792, -0.066, r(0.958, 0.0)),
                pr(1.064, 0.276, r(1.000, 0.0)),
            ],
            1.0,
        )
        .normalized(),
        _ => return None,
    })
}

/// `getX` exports and the builder each caches, in `material-shapes.js` order.
pub const GETTERS: [(&str, &str); 35] = [
    ("getCircle", "circle"),
    ("getSquare", "square"),
    ("getSlanted", "slanted"),
    ("getArch", "arch"),
    ("getFan", "fan"),
    ("getArrow", "arrow"),
    ("getSemiCircle", "semiCircle"),
    ("getOval", "oval"),
    ("getPill", "pill"),
    ("getTriangle", "triangle"),
    ("getDiamond", "diamond"),
    ("getClamShell", "clamShell"),
    ("getPentagon", "pentagon"),
    ("getGem", "gem"),
    ("getSunny", "sunny"),
    ("getVerySunny", "verySunny"),
    ("getCookie4Sided", "cookie4"),
    ("getCookie6Sided", "cookie6"),
    ("getCookie7Sided", "cookie7"),
    ("getCookie9Sided", "cookie9"),
    ("getCookie12Sided", "cookie12"),
    ("getGhostish", "ghostish"),
    ("getClover4Leaf", "clover4"),
    ("getClover8Leaf", "clover8"),
    ("getBurst", "burst"),
    ("getSoftBurst", "softBurst"),
    ("getBoom", "boom"),
    ("getSoftBoom", "softBoom"),
    ("getFlower", "flower"),
    ("getPuffy", "puffy"),
    ("getPuffyDiamond", "puffyDiamond"),
    ("getPixelCircle", "pixelCircle"),
    ("getPixelTriangle", "pixelTriangle"),
    ("getBun", "bun"),
    ("getHeart", "heart"),
];
