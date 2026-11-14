//! What a script reads for a property of a Qt type that nothing set and that Qt works out
//! from the object's other properties, where `qt_defaults.tsv` has one value per type.
//! The engine's own reads do not come here.

use crate::runtime::World;
use crate::value::Value;
use crate::valuetypes::{Vt, make_num};

/// The properties `script_value` knows.
const NAMES: &[&str] = &[
    "canvasSize",
    "tileSize",
    "canvasWindow",
    "sourceSize",
    "baseUrl",
    "transformOriginPoint",
    "contentWidth",
    "contentHeight",
    "nameFilters",
    "dashPattern",
];

impl World {
    pub(crate) fn script_value(&mut self, item: usize, name: &str) -> Option<Value> {
        // An object the type always has is a group of the item's own properties.
        if crate::qt_defaults::group(&self.items.get(item)?.types, name).is_some() {
            return Some(Value::Item(self.group(item, name)));
        }
        // QEasingCurve::toCubicSpline of a curve that is not a spline: no points.
        if name == "easing.bezierCurve"
            && (self.items.get(item)?.prop_info("easing")).is_some_and(|p| p.ty == "QEasingCurve")
        {
            return Some(Value::array(Vec::new()));
        }
        // QQuickText::effectiveHAlign and kin: what was asked for, where nothing mirrors it.
        if let Some(asked) = name.strip_prefix("effectiveH")
            && self.items.get(item)?.prop_info(name).is_some()
        {
            return self.read(item, &format!("h{asked}"));
        }
        if let Some(field) = name.strip_prefix("fontInfo.") {
            return self.font_info(item, field);
        }
        if !NAMES.contains(&name) {
            return None;
        }
        let ty = self.items.get(item)?.prop_info(name)?.ty.clone();
        // What this reads is read as the engine does.
        let script = std::mem::take(&mut self.script_read);
        let v = self.script_value_of(item, name, &ty);
        self.script_read = script;
        v
    }

    fn script_value_of(&mut self, item: usize, name: &str, ty: &str) -> Option<Value> {
        let is = |w: &World, t: &str| w.items[item].types.iter().any(|x| x == t);
        let num = |w: &mut World, k: &str| w.read(item, k).map_or(0.0, |v| v.to_number());
        Some(match (name, ty) {
            // QQuickCanvasItem: the canvas and its tiles are the item's size once it has
            // one, 1x1 before; the window is the whole canvas.
            ("canvasSize" | "tileSize", _) if is(self, "Canvas") => {
                let size = match [num(self, "width"), num(self, "height")] {
                    [0.0, 0.0] => [1.0, 1.0],
                    s => s,
                };
                make_num(Vt::of_type(ty)?, &size)
            }
            ("canvasWindow", _) if is(self, "Canvas") => {
                let size = [num(self, "width"), num(self, "height")];
                make_num(Vt::RectF, &[0.0, 0.0, size[0], size[1]])
            }
            // QQuickImageBase::sourceSize: a dimension nothing asked for is the pixmap's.
            ("sourceSize", "QSize") if is(self, "Image") => {
                let asked = ["width", "height"].map(|k| {
                    let field = format!("sourceSize.{k}");
                    // A binding that gave `undefined` was refused: nothing asked.
                    (self.explicit(item, &field))
                        .then(|| self.read(item, &field))
                        .flatten()
                        .filter(|v| !matches!(v, Value::Undefined))
                        .map(|v| v.to_number())
                });
                let size = match asked {
                    [None, None] => [num(self, "implicitWidth"), num(self, "implicitHeight")],
                    // `sourceSize.width: 8` is a write of the whole size, QSize(8, 0) with
                    // nothing loaded yet, and the height it wrote is asked for as well.
                    asked => asked.map(|v| v.unwrap_or(0.0)),
                };
                make_num(Vt::Size, &size)
            }
            // QQuickItemPrivate::computeTransformOrigin: the point of the item's rectangle
            // `transformOrigin` names.
            ("transformOriginPoint", "QPointF") => {
                let size = [num(self, "width"), num(self, "height")];
                self.script_read = true;
                let origin = self.read(item, "transformOrigin");
                self.script_read = false;
                let origin = origin.map_or(4.0, |v| v.to_number()) as usize;
                let at = |i: usize, len: f64| [0.0, len / 2.0, len][i.min(2)];
                make_num(
                    Vt::PointF,
                    &[at(origin % 3, size[0]), at(origin / 3, size[1])],
                )
            }
            // QQuickFlickable: -1 until a size of the content is set.
            ("contentWidth" | "contentHeight", _)
                if self.items[item].types.first().is_some_and(|t| {
                    matches!(t.as_str(), "Flickable" | "ScrollView")
                        || (name == "contentWidth" && matches!(t.as_str(), "ListView" | "GridView"))
                }) =>
            {
                Value::Num(-1.0)
            }
            // QQuickFolderListModel lists every name until told which.
            ("nameFilters", _) if is(self, "FolderListModel") => {
                Value::array(vec![Value::str("*")])
            }
            // QQuickShapePath: a dash of 4 and a gap of 2, used once the stroke is dashed.
            ("dashPattern", _) if is(self, "ShapePath") => {
                Value::array(vec![Value::Num(4.0), Value::Num(2.0)])
            }
            // QQuickText::baseUrl: the url of the document the item is written in.
            ("baseUrl", "QUrl") => {
                let doc = self.items[item].ctx.and_then(|c| self.ctx_url(c as usize));
                Value::str(&crate::qurl::base(doc.as_ref()).to_string())
            }
            _ => return None,
        })
    }

    /// Field `field` of `fontInfo` (QQuickFontInfo): of the font fontconfig picks for the
    /// item's `font`, its family and style; the rest as the font asks.
    fn font_info(&mut self, item: usize, field: &str) -> Option<Value> {
        self.items.get(item)?.prop_info("fontInfo")?;
        let script = std::mem::replace(&mut self.script_read, true);
        let mut font = |k: &str| self.read(item, &format!("font.{k}"));
        let v = match field {
            "family" | "styleName" => {
                let family = font("family")?.to_js_string();
                let weight = font("weight")?.to_number();
                let italic = font("italic")?.truthy();
                let [family, style] = crate::qtobj::font_match(&family, weight, italic)?;
                Some(Value::str(if field == "family" { &family } else { &style }))
            }
            "bold" | "weight" | "italic" | "pointSize" | "pixelSize" => font(field),
            _ => None,
        };
        self.script_read = script;
        v
    }

    /// What String() of group `g` gives where Qt's is a gadget or a value type: its
    /// properties as QVariant::toString gives them (QQmlValueTypeWrapper), a font as QFont
    /// writes itself.
    pub(crate) fn group_text(&mut self, g: usize) -> Option<String> {
        let (owner, prefix) = self.groups.get(&g)?.clone();
        let inst = self.items.get(owner)?;
        if let Some(text) = crate::qt_defaults::group(&inst.types, &prefix) {
            return text.map(str::to_string);
        }
        let ty = inst.prop_info(&prefix)?.ty.clone();
        let script = std::mem::replace(&mut self.script_read, true);
        let mut field = |k: &str| {
            let v = self.read(owner, &format!("{prefix}.{k}"));
            v.unwrap_or(Value::Undefined)
        };
        let text = match ty.as_str() {
            "QFont" => {
                let Value::Obj(mut font) = crate::qtobj::system_font_value() else {
                    unreachable!()
                };
                let family = Value::str(crate::colorscheme::system_font().qt_family());
                if let Some(Value::Obj(f)) = crate::valuetypes::with_field(
                    &font,
                    crate::valuetypes::Vt::FONT,
                    "family",
                    &family,
                ) {
                    font = f;
                }
                // The weight the style gives the type's font, where nothing set one.
                let weight = field("weight");
                if let Some(Value::Obj(f)) = crate::valuetypes::with_field(
                    &font,
                    crate::valuetypes::Vt::FONT,
                    "weight",
                    &weight,
                ) {
                    font = f;
                }
                for k in ["weight", "pointSize", "pixelSize"]
                    .into_iter()
                    .chain(crate::valuetypes::Vt::FONT.data().iter().copied())
                {
                    // A field nothing set in a control is the one set in the control it is in.
                    let name = format!("{prefix}.{k}");
                    let of = self.font_from(owner, &name).unwrap_or(owner);
                    if !self.explicit(of, &name) {
                        continue;
                    }
                    let v = self.font_held(of, &name);
                    let crate::value::Internal::ValueType(vt) = font.borrow().internal else {
                        unreachable!()
                    };
                    if let Some(Value::Obj(f)) =
                        crate::valuetypes::with_field(&font, vt, k, &v.unwrap_or(Value::Undefined))
                    {
                        font = f;
                    }
                }
                crate::valuetypes::display(&font.borrow())
            }
            "QEasingCurve" => {
                let ty = field("type").to_number();
                let name = crate::builtins::EASING_NAMES.get(ty as usize)?;
                let nums = ["amplitude", "overshoot", "period"]
                    .map(|k| crate::valuetypes::arg(field(k).to_number()));
                Some(format!("QEasingCurve({name}, {}, )", nums.join(", ")))
            }
            "QQuickIcon" => {
                let parts =
                    ["name", "source", "width", "height", "color", "cache"].map(|k| {
                        match (field(k), k) {
                            (Value::Undefined, "width" | "height") => "0".to_string(),
                            (Value::Undefined, _) => String::new(),
                            (v, _) => v.to_js_string(),
                        }
                    });
                Some(format!("QQuickIcon({})", parts.join(", ")))
            }
            _ => None,
        };
        self.script_read = script;
        text
    }
}
