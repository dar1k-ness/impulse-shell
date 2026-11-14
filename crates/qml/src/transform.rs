//! Where an item's points land in its parent, as QQuickItemPrivate::itemToParentTransform
//! composes it in Qt 6.12: the item's position, then its `transform` list from the last entry
//! to the first (each a QQuickTransform::applyTo on a QMatrix4x4, read back with
//! QMatrix4x4::toTransform), then its `scale` and `rotation` about `transformOrigin`.
//! Drawing, pointer hits, `mapToItem` and its siblings, `contains` and `childAt` all go
//! through the one matrix.

use crate::runtime::{Sym, World};
use crate::value::Value;
use crate::valuetypes::{GENERAL, Mat4, Vt};

/// A QTransform: `m[row][column]` is `m11..m33`, and a point maps as a row vector, so
/// `x' = m11 x + m21 y + m31`, `y' = m12 x + m22 y + m32`, `w' = m13 x + m23 y + m33`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub m: [[f64; 3]; 3],
}

impl Default for Xf {
    fn default() -> Self {
        Xf::IDENTITY
    }
}

impl Xf {
    pub const IDENTITY: Xf = Xf {
        m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    };

    pub fn translation(dx: f64, dy: f64) -> Xf {
        let mut t = Xf::IDENTITY;
        t.translate(dx, dy);
        t
    }

    /// QTransform::translate: the points move by (dx, dy) before the rest applies.
    #[allow(clippy::needless_range_loop)]
    pub fn translate(&mut self, dx: f64, dy: f64) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let m = &mut self.m;
        for c in 0..3 {
            m[2][c] += dx * m[0][c] + dy * m[1][c];
        }
    }

    /// QTransform::scale.
    #[allow(clippy::needless_range_loop)]
    pub fn scale(&mut self, sx: f64, sy: f64) {
        if sx == 1.0 && sy == 1.0 {
            return;
        }
        let m = &mut self.m;
        for c in 0..3 {
            m[0][c] *= sx;
            m[1][c] *= sy;
        }
    }

    /// QTransform::rotate about the z axis, in degrees; the quarter turns are exact.
    #[allow(clippy::needless_range_loop)]
    pub fn rotate(&mut self, a: f64) {
        if a == 0.0 {
            return;
        }
        let (sina, cosa) = if a == 90.0 || a == -270.0 {
            (1.0, 0.0)
        } else if a == 270.0 || a == -90.0 {
            (-1.0, 0.0)
        } else if a == 180.0 {
            (0.0, -1.0)
        } else {
            let b = a.to_radians();
            (b.sin(), b.cos())
        };
        let m = &mut self.m;
        for c in 0..3 {
            let (r0, r1) = (m[0][c], m[1][c]);
            m[0][c] = cosa * r0 + sina * r1;
            m[1][c] = -sina * r0 + cosa * r1;
        }
    }

    /// QTransform::operator*: `self`, then `o`.
    pub fn then(&self, o: &Xf) -> Xf {
        let mut m = [[0.0; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = (0..3).map(|k| self.m[r][k] * o.m[k][c]).sum();
            }
        }
        Xf { m }
    }

    /// QTransform::map(QPointF).
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        let m = &self.m;
        let (px, py) = (
            m[0][0] * x + m[1][0] * y + m[2][0],
            m[0][1] * x + m[1][1] * y + m[2][1],
        );
        if self.is_affine() {
            return (px, py);
        }
        let w = 1.0 / (m[0][2] * x + m[1][2] * y + m[2][2]);
        (px * w, py * w)
    }

    /// QTransform::mapRect of `[x, y, width, height]`: the bounding box of the four corners.
    pub fn map_rect(&self, r: [f64; 4]) -> [f64; 4] {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in [
            (r[0], r[1]),
            (r[0] + r[2], r[1]),
            (r[0] + r[2], r[1] + r[3]),
            (r[0], r[1] + r[3]),
        ] {
            let (px, py) = self.map(x, y);
            x0 = x0.min(px);
            y0 = y0.min(py);
            x1 = x1.max(px);
            y1 = y1.max(py);
        }
        [x0, y0, x1 - x0, y1 - y0]
    }

    pub fn is_affine(&self) -> bool {
        self.m[0][2] == 0.0 && self.m[1][2] == 0.0 && self.m[2][2] == 1.0
    }

    /// The offset, when the transform only moves points.
    pub fn as_translation(&self) -> Option<(f64, f64)> {
        let m = &self.m;
        (self.is_affine() && m[0][0] == 1.0 && m[0][1] == 0.0 && m[1][0] == 0.0 && m[1][1] == 1.0)
            .then_some((m[2][0], m[2][1]))
    }

    /// `[sx, sy, dx, dy]` when the transform scales along the axes by positive factors and
    /// moves: rectangles stay upright rectangles.
    pub fn as_scale(&self) -> Option<[f64; 4]> {
        let m = &self.m;
        (self.is_affine() && m[0][1] == 0.0 && m[1][0] == 0.0 && m[0][0] > 0.0 && m[1][1] > 0.0)
            .then_some([m[0][0], m[1][1], m[2][0], m[2][1]])
    }

    /// `[cos, sin, cx, cy]` when the transform is a turn about the point `(cx, cy)` and
    /// nothing else.
    pub fn as_rotation(&self) -> Option<[f64; 4]> {
        let [c, s, c2, d, e, f] = self.affine()?;
        let near = |a: f64, b: f64| (a - b).abs() < 1e-12;
        if !(near(c, d) && near(s, -c2) && near(c * c + s * s, 1.0)) || near(c, 1.0) {
            return None;
        }
        // The point the turn leaves in place: (1 - c) cx + s cy = e, -s cx + (1 - c) cy = f.
        let det = 2.0 * (1.0 - c);
        Some([
            c,
            s,
            ((1.0 - c) * e - s * f) / det,
            (s * e + (1.0 - c) * f) / det,
        ])
    }

    /// `[a, b, c, d, e, f]` of an affine transform: `x' = a x + c y + e`, `y' = b x + d y + f`.
    pub fn affine(&self) -> Option<[f64; 6]> {
        let m = &self.m;
        self.is_affine()
            .then_some([m[0][0], m[0][1], m[1][0], m[1][1], m[2][0], m[2][1]])
    }

    pub fn determinant(&self) -> f64 {
        let m = &self.m;
        m[0][0] * (m[2][2] * m[1][1] - m[2][1] * m[1][2])
            - m[1][0] * (m[2][2] * m[0][1] - m[2][1] * m[0][2])
            + m[2][0] * (m[1][2] * m[0][1] - m[1][1] * m[0][2])
    }

    /// The inverse; `None` where QTransform::inverted finds none (a determinant within
    /// qFuzzyIsNull of zero).
    pub fn inverse(&self) -> Option<Xf> {
        let det = self.determinant();
        if det.abs() <= 1e-12 || !det.is_finite() {
            return None;
        }
        let m = &self.m;
        let cof = |r0: usize, r1: usize, c0: usize, c1: usize| {
            m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
        };
        let adj = [
            [cof(1, 2, 1, 2), -cof(0, 2, 1, 2), cof(0, 1, 1, 2)],
            [-cof(1, 2, 0, 2), cof(0, 2, 0, 2), -cof(0, 1, 0, 2)],
            [cof(1, 2, 0, 1), -cof(0, 2, 0, 1), cof(0, 1, 0, 1)],
        ];
        Some(Xf {
            m: adj.map(|r| r.map(|v| v / det)),
        })
    }

    /// QTransform::inverted: the identity when there is no inverse.
    pub fn inverted(&self) -> Xf {
        self.inverse().unwrap_or(Xf::IDENTITY)
    }

    /// QMatrix4x4(const QTransform &).
    fn to_mat4(self) -> Mat4 {
        let t = self.m.map(|r| r.map(|v| v as f32));
        let mut m = Mat4::identity();
        m.m[0] = [t[0][0], t[0][1], 0.0, t[0][2]];
        m.m[1] = [t[1][0], t[1][1], 0.0, t[1][2]];
        m.m[3] = [t[2][0], t[2][1], 0.0, t[2][2]];
        m.flags = GENERAL;
        m
    }

    /// QMatrix4x4::toTransform(): the matrix without its z row and column.
    fn from_mat4(m: &Mat4) -> Xf {
        let row = |c: usize| [m.m[c][0], m.m[c][1], m.m[c][3]].map(f64::from);
        Xf {
            m: [row(0), row(1), row(3)],
        }
    }
}

/// The QML types that are a QQuickTransform.
pub const TYPES: [&str; 5] = ["Translate", "Scale", "Rotation", "Matrix4x4", "Shear"];

impl World {
    pub fn is_transform(&self, i: usize) -> bool {
        self.items
            .get(i)
            .is_some_and(|it| it.types.iter().any(|t| TYPES.iter().any(|n| t == n)))
    }

    /// A number a transform is made of. `name` is the C++ property: one declared in QML under
    /// its name has a slot of its own (`World::declared_sym`) and is not read here.
    fn xf_num(&mut self, i: usize, name: &str, dflt: f64) -> f64 {
        match self.read(i, name) {
            Some(Value::Undefined | Value::Null) | None => dflt,
            Some(v) => v.to_number(),
        }
    }

    /// A QVector3D property: each component from its grouped assignment (`origin.x: 5`) if
    /// there is one, else from the whole value, else `dflt`.
    fn xf_vec3(&mut self, i: usize, name: &str, dflt: [f64; 3]) -> [f32; 3] {
        let stored = self.items[i].props.get(name).map(|p| p.value.clone());
        if let Some(r) = &mut self.reading {
            r.push((i, Sym::new(name)));
        }
        let whole = match stored {
            Some(Value::Obj(o))
                if matches!(
                    crate::valuetypes::kind(&Value::Obj(o.clone())),
                    Some(Vt::Vector3D)
                ) =>
            {
                let n = crate::valuetypes::nums(&o.borrow(), Vt::Vector3D);
                [n[0], n[1], n[2]]
            }
            _ => dflt,
        };
        let grouped = self.has_grouped(i, name);
        let mut out = [0.0f32; 3];
        for (k, f) in ["x", "y", "z"].iter().enumerate() {
            let key = format!("{name}.{f}");
            let own = grouped && self.items[i].props.contains_key(&key);
            out[k] = if own {
                self.xf_num(i, &key, whole[k]) as f32
            } else {
                whole[k] as f32
            };
        }
        out
    }

    /// `origin` of a Scale, Rotation or Shear and `axis` of a Rotation whose fields were set
    /// one by one (`origin.x: 5`): the QVector3D they make, as Qt reads the property.
    pub(crate) fn grouped_vec3(&mut self, i: usize, name: &str) -> Option<Value> {
        let inst = self.items.get(i)?;
        let ty = |n: &str| inst.types.iter().any(|t| t == n);
        let dflt = match name {
            "origin" if ty("Scale") || ty("Rotation") || ty("Shear") => [0.0; 3],
            "axis" if ty("Rotation") => [0.0, 0.0, 1.0],
            _ => return None,
        };
        if !self.has_grouped(i, name) {
            return None;
        }
        let v = self.xf_vec3(i, name, dflt);
        let n = [v[0] as f64, v[1] as f64, v[2] as f64];
        // The stored value is the one script holds: a write to one of its fields reaches the
        // property through it, so it is replaced only when a field's own value moved on.
        if let Some(Value::Obj(o)) = self.items[i].props.get(name).map(|p| &p.value)
            && matches!(o.borrow().internal, crate::value::Internal::ValueType(_))
            && crate::valuetypes::nums(&o.borrow(), Vt::Vector3D)[..3] == n
        {
            return Some(Value::Obj(o.clone()));
        }
        let v = crate::valuetypes::make_num(Vt::Vector3D, &n);
        if let Value::Obj(o) = &v {
            o.borrow_mut().owner = Some((i, std::rc::Rc::from(name)));
        }
        self.items[i]
            .props
            .entry_sym(Sym::new(name))
            .or_insert_with(|| crate::runtime::Prop {
                ty: Sym::new("vector3d"),
                ..Default::default()
            })
            .value = v.clone();
        Some(v)
    }

    /// QQuickTransform::applyTo of the transform object `t`.
    fn apply_transform(&mut self, t: usize, m: &mut Mat4) {
        let Some(kind) = self.items.get(t).and_then(|it| {
            TYPES
                .iter()
                .copied()
                .find(|n| it.types.iter().any(|ty| ty == n))
        }) else {
            return;
        };
        match kind {
            "Translate" => {
                let (x, y) = (self.xf_num(t, "x", 0.0), self.xf_num(t, "y", 0.0));
                m.translate(x as f32, y as f32, 0.0);
            }
            "Scale" => {
                let o = self.xf_vec3(t, "origin", [0.0; 3]);
                let s = ["xScale", "yScale", "zScale"].map(|n| self.xf_num(t, n, 1.0) as f32);
                m.translate(o[0], o[1], o[2]);
                m.scale(s[0], s[1], s[2]);
                m.translate(-o[0], -o[1], -o[2]);
            }
            "Rotation" => {
                let angle = self.xf_num(t, "angle", 0.0);
                let axis = self.xf_vec3(t, "axis", [0.0, 0.0, 1.0]);
                if angle == 0.0 || axis == [0.0; 3] {
                    return;
                }
                let o = self.xf_vec3(t, "origin", [0.0; 3]);
                m.translate(o[0], o[1], o[2]);
                m.projected_rotate(angle as f32, axis[0], axis[1], axis[2], 1024.0);
                m.translate(-o[0], -o[1], -o[2]);
            }
            "Shear" => {
                let [xf, yf, xa, ya] =
                    ["xFactor", "yFactor", "xAngle", "yAngle"].map(|n| self.xf_num(t, n, 0.0));
                if xf == 0.0 && yf == 0.0 && xa == 0.0 && ya == 0.0 {
                    return;
                }
                let o = self.xf_vec3(t, "origin", [0.0; 3]);
                let mut sh = Mat4::identity();
                sh.m[1][0] = (xf + xa.to_radians().tan()) as f32;
                sh.m[0][1] = (yf + ya.to_radians().tan()) as f32;
                sh.flags = GENERAL;
                m.translate(o[0], o[1], o[2]);
                *m = m.mul(&sh);
                m.translate(-o[0], -o[1], -o[2]);
            }
            _ => {
                if let Some(Value::Obj(o)) = self.read(t, "matrix")
                    && let Some(vt @ Vt::Matrix4x4 { flags }) =
                        crate::valuetypes::kind(&Value::Obj(o.clone()))
                {
                    let n = crate::valuetypes::nums(&o.borrow(), vt);
                    *m = m.mul(&Mat4::from_rows(&n, flags));
                }
            }
        }
    }

    /// Whether `item` has QQuickItem's `transform` list: a QQuickItem with no QML property
    /// of the name.
    pub(crate) fn has_transform_list(&self, item: usize) -> bool {
        let it = &self.items[item];
        !crate::types::is_non_item(&it.types)
            && it.props.get("transform").is_none_or(|p| p.seq == 0)
    }

    /// What a write of `v` leaves in the `transform` list, a QQmlListProperty of
    /// QQuickTransform that is cleared and then appended to: one transform is a list of it,
    /// a list keeps its transforms, and anything else leaves the list empty. An object that
    /// is no transform is refused with Qt's warning; one listed twice stays once, at its
    /// last place (QQuickTransform::appendToItem). `undefined` changes nothing (`None`): a
    /// script assigning it has thrown already.
    pub(crate) fn transform_list(&mut self, v: Value) -> Option<Value> {
        let elems = match &v {
            Value::Undefined => return None,
            Value::Item(_) => vec![v.clone()],
            Value::Obj(o) => o
                .try_borrow()
                .ok()
                .and_then(|b| b.array.clone())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut keep: Vec<usize> = Vec::new();
        let mut same = matches!(v, Value::Obj(_));
        for e in &elems {
            match e {
                Value::Item(t) if self.is_transform(*t) => {
                    if let Some(at) = keep.iter().position(|k| k == t) {
                        keep.remove(at);
                        same = false;
                    }
                    keep.push(*t);
                }
                Value::Item(t) => {
                    same = false;
                    let what = self.items[*t]
                        .types
                        .last()
                        .map_or("QObject", |t| t.as_str());
                    tracing::warn!(
                        target: "ri::qml",
                        "Cannot append {what} to a QML list of QQuickTransform*"
                    );
                }
                _ => same = false,
            }
        }
        // The list itself, changed in place (`transform.push(t)`), stays the same object.
        if same && keep.len() == elems.len() {
            return Some(v);
        }
        Some(Value::array(keep.into_iter().map(Value::Item).collect()))
    }

    /// The objects in `item`'s `transform` list.
    pub fn transforms(&mut self, item: usize) -> Vec<usize> {
        // No QQuickItem, or a QML property of the name: not the transform list.
        if self.items[item].props.get("transform").is_none() {
            return Vec::new();
        }
        let l = match self.read(item, "transform") {
            Some(Value::Item(i)) => vec![i],
            Some(Value::Obj(o)) => match &o.borrow().array {
                Some(a) => a
                    .iter()
                    .filter_map(|v| match v {
                        Value::Item(i) => Some(*i),
                        _ => None,
                    })
                    .collect(),
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        let l: Vec<usize> = l
            .into_iter()
            .filter(|&t| !self.dead.contains(&t) && self.is_transform(t))
            .collect();
        // QQuickTransformPrivate::items: a change of the transform redraws the items using it.
        for &t in &l {
            let users = self.transform_users.entry(t).or_default();
            if !users.contains(&item) {
                users.push(item);
            }
        }
        l
    }

    /// The `scale` that transforms `item`: QQuickItem's, whatever QML declares under the name.
    pub fn xf_scale(&mut self, item: usize) -> f64 {
        self.xf_num(item, "scale", 1.0)
    }

    /// `item`'s own transform: what its `transform` list, `scale` and `rotation` do to a
    /// point of the item, before its `x` and `y` place it in the parent. `None` when they do
    /// nothing. Reads no notifier.
    pub fn item_transform(&mut self, item: usize) -> Option<Xf> {
        self.transform_with(item, false)
    }

    /// `item_transform` as the item's QSGTransformNode has it: the node's matrix is a
    /// QMatrix4x4, of floats (QQuickWindowPrivate::updateDirtyNode), so a `scale` that is 1
    /// as a float (CookieClock's `1.4 - 0.4 * shown`) leaves the subtree as it is, and a
    /// `rotation` of -180 is as exact a half turn as 180 (QMatrix4x4::rotate; QTransform::rotate
    /// takes its sine and cosine).
    pub fn node_transform(&mut self, item: usize) -> Option<Xf> {
        self.transform_with(item, true)
    }

    fn transform_with(&mut self, item: usize, node: bool) -> Option<Xf> {
        if self.items.get(item).is_none() || crate::types::is_non_item(&self.items[item].types) {
            return None;
        }
        let reading = self.reading.take();
        let list = self.transforms(item);
        let rot = self.xf_num(item, "rotation", 0.0);
        let rot = if rot.is_finite() { rot } else { 0.0 };
        let sc = self.xf_scale(item);
        let (rot, sc) = match node {
            true if rot as f32 == -180.0 => (180.0, sc as f32 as f64),
            true => (rot, sc as f32 as f64),
            false => (rot, sc),
        };
        let mut t = Xf::IDENTITY;
        let mut any = false;
        if !list.is_empty() {
            let mut m = t.to_mat4();
            for &tr in list.iter().rev() {
                self.apply_transform(tr, &mut m);
            }
            t = Xf::from_mat4(&m);
            any = t != Xf::IDENTITY;
        }
        // Whole turns leave the points where they are.
        if sc != 1.0 || rot % 360.0 != 0.0 {
            let (w, h) = (
                self.xf_num(item, "width", 0.0),
                self.xf_num(item, "height", 0.0),
            );
            let to = self.xf_num(item, "transformOrigin", 4.0) as i32;
            let f = |k: i32| [0.0, 0.5, 1.0][k.clamp(0, 2) as usize];
            let (ox, oy) = (w * f(to % 3), h * f(to / 3));
            t.translate(ox, oy);
            t.scale(sc, sc);
            t.rotate(rot);
            t.translate(-ox, -oy);
            any = true;
        }
        self.reading = reading;
        any.then_some(t)
    }

    /// The matrix of `item`'s transform node with the item at (x, y) in its parent, as
    /// QQuickWindowPrivate::updateDirtyNode builds it: what `item_transform` composes in
    /// double, here each a QMatrix4x4 call in float after the move to (x, y). The scene graph
    /// multiplies these, and their bits reach the pixels of distance-field text.
    pub fn item_node_matrix(&mut self, item: usize, x: f64, y: f64) -> Mat4 {
        let mut m = Mat4::identity();
        if x != 0.0 || y != 0.0 {
            m.translate(x as f32, y as f32, 0.0);
        }
        if self.items.get(item).is_none() || crate::types::is_non_item(&self.items[item].types) {
            return m;
        }
        let reading = self.reading.take();
        for &tr in self.transforms(item).iter().rev() {
            self.apply_transform(tr, &mut m);
        }
        let rot = self.xf_num(item, "rotation", 0.0);
        let rot = if rot.is_finite() { rot } else { 0.0 };
        let sc = self.xf_scale(item);
        if sc != 1.0 || rot != 0.0 {
            let (w, h) = (
                self.xf_num(item, "width", 0.0),
                self.xf_num(item, "height", 0.0),
            );
            let to = self.xf_num(item, "transformOrigin", 4.0) as i32;
            let f = |k: i32| [0.0, 0.5, 1.0][k.clamp(0, 2) as usize];
            let (ox, oy) = (w * f(to % 3), h * f(to / 3));
            m.translate(ox as f32, oy as f32, 0.0);
            if sc != 1.0 {
                m.scale(sc as f32, sc as f32, 1.0);
            }
            if rot != 0.0 {
                m.rotate(rot as f32, 0.0, 0.0, 1.0);
            }
            m.translate(-ox as f32, -oy as f32, 0.0);
        }
        self.reading = reading;
        m
    }

    /// The combined matrix of `item`'s transform node under `base`, the combined matrix above
    /// the item `top`: as QSGBatchRenderer's Updater::visitTransformNode, each node from `top`
    /// down to `item` that is not the identity multiplies in, in float.
    pub fn combined_node_matrix(&mut self, item: usize, top: usize, base: Mat4) -> Mat4 {
        let mut chain = Vec::new();
        let mut cur = Some(item);
        while let Some(i) = cur {
            let (x, y, next) = self.parent_step(i);
            chain.push((i, x, y));
            if i == top {
                break;
            }
            cur = next;
        }
        let mut m = base;
        for (i, x, y) in chain.into_iter().rev() {
            let n = self.item_node_matrix(i, x, y);
            if n.flags != crate::valuetypes::IDENTITY {
                m = m.mul(&n);
            }
        }
        m
    }

    /// QQuickItemPrivate::itemToWindowTransform: `item`'s points in the scene.
    pub fn item_to_scene(&mut self, item: usize) -> Xf {
        let mut t = Xf::IDENTITY;
        let mut cur = Some(item);
        while let Some(i) = cur {
            if self.dead.contains(&i) {
                break;
            }
            if let Some(own) = self.item_transform(i) {
                t = t.then(&own);
            }
            let (x, y, next) = self.parent_step(i);
            t = t.then(&Xf::translation(x, y));
            cur = next;
        }
        t
    }

    /// QQuickItem::mapToItem: `from`'s points in `to` (in the scene without one): out to
    /// the scene, then through `to`'s inverse (the identity where it has none).
    pub fn map_xf(&mut self, from: usize, to: Option<usize>) -> Xf {
        let t = self.item_to_scene(from);
        match to {
            Some(o) if !self.dead.contains(&o) => t.then(&self.item_to_scene(o).inverted()),
            _ => t,
        }
    }

    /// QQuickItem::contains: a point is in an item from its left and top edge up to, not
    /// including, its right and bottom one. With an item as `containmentMask` the mask
    /// answers instead, for the point moved back by the mask's position. A Flickable's
    /// contentItem holds what the Flickable does (QQuickFlickableContentItem::contains).
    pub fn item_contains(&mut self, item: usize, (mut x, mut y): (f64, f64)) -> bool {
        if let Some(f) = self.flickable_of(item) {
            let (cx, cy) = self.scene_pos(item, Some(f));
            return self.item_contains(f, (x + cx, y + cy));
        }
        let reading = self.reading.take();
        let mut cur = item;
        for _ in 0..64 {
            let mask = match self.items[cur].props.contains_key("containmentMask") {
                true => self.read(cur, "containmentMask"),
                false => None,
            };
            match mask {
                Some(Value::Item(m))
                    if m != cur
                        && !self.dead.contains(&m)
                        && self
                            .items
                            .get(m)
                            .is_some_and(|i| !crate::types::is_non_item(&i.types)) =>
                {
                    x -= self.xf_num(m, "x", 0.0);
                    y -= self.xf_num(m, "y", 0.0);
                    cur = m;
                }
                _ => break,
            }
        }
        let (w, h) = (
            self.xf_num(cur, "width", 0.0),
            self.xf_num(cur, "height", 0.0),
        );
        self.reading = reading;
        x >= 0.0 && y >= 0.0 && x < w && y < h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: (f64, f64), b: (f64, f64)) {
        assert!(
            (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9,
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn a_turn_about_a_point_gives_its_angle_and_point() {
        let mut t = Xf::IDENTITY;
        t.translate(30.0, 12.0);
        t.rotate(40.0);
        t.translate(-30.0, -12.0);
        let [c, s, cx, cy] = t.as_rotation().unwrap();
        let (sin, cos) = 40f64.to_radians().sin_cos();
        near((c, s), (cos, sin));
        near((cx, cy), (30.0, 12.0));
        t.scale(2.0, 2.0);
        assert_eq!(t.as_rotation(), None);
        assert_eq!(Xf::translation(3.0, 4.0).as_rotation(), None);
    }

    #[test]
    fn operations_apply_before_what_is_there() {
        let mut t = Xf::IDENTITY;
        t.translate(10.0, 20.0);
        t.scale(2.0, 3.0);
        t.rotate(90.0);
        t.translate(1.0, 0.0);
        // (0,0) moves to (1,0), turns to (0,1), scales to (0,3), lands at (10,23).
        near(t.map(0.0, 0.0), (10.0, 23.0));
        near(t.inverted().map(10.0, 23.0), (0.0, 0.0));
        assert_eq!(t.map_rect([0.0, 0.0, 1.0, 1.0]), [8.0, 23.0, 2.0, 3.0]);
    }

    /// QQuickTransform::update: a change of a transform draws again every item whose list
    /// has it, and no item that had it before.
    #[test]
    fn a_change_of_a_transform_draws_its_items_again() {
        let mut rt = crate::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
Item {
    property alias a: a
    property alias b: b
    property alias c: c
    property alias t: t
    Translate { id: t; x: 1 }
    Item { id: a; transform: t }
    Item { id: b; transform: [t, t] }
    Item { id: c; transform: t }
}"#,
            )
            .unwrap();
        rt.settle();
        let item = |rt: &mut crate::runtime::Runtime, n: &str| match rt.get(root, n) {
            Value::Item(i) => i,
            v => panic!("{n} is {v:?}"),
        };
        let [a, b, c, t] = ["a", "b", "c", "t"].map(|n| item(&mut rt, n));
        // A list holds a transform once (QQuickTransform::appendToItem): `[t, t]` moves by 1.
        for i in [a, b, c] {
            let got = rt.world.borrow_mut().item_transform(i);
            assert_eq!(got, Some(Xf::translation(1.0, 0.0)));
        }
        rt.set(c, "transform", Value::array(Vec::new()));
        rt.settle();
        assert_eq!(rt.world.borrow_mut().item_transform(c), None);
        rt.world.borrow_mut().touched.clear();
        rt.set(t, "x", Value::Num(5.0));
        rt.settle();
        let touched = std::mem::take(&mut rt.world.borrow_mut().touched);
        assert!(touched.contains(&a) && touched.contains(&b), "{touched:?}");
        assert!(!touched.contains(&c), "{touched:?}");
    }

    #[test]
    fn no_inverse_is_the_identity() {
        let mut t = Xf::IDENTITY;
        t.scale(0.0, 1.0);
        assert!(t.inverse().is_none());
        assert_eq!(t.inverted(), Xf::IDENTITY);
    }
}
