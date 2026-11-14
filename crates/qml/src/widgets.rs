//! Compiled-in widget types of the widget layout: what a layout entry's `type` may name, where
//! the widget may be placed, its size limits, and the typed schema its settings are checked
//! against. A shell ships its registry as a `widgets.toml` manifest next to its root QML; each
//! entry names the shell's own QML component, which is compiled with the rest of the shell, so
//! configuration selects and parameterizes widgets but never adds code.
//!
//! User settings arrive as raw TOML overrides (the layout model persists them untouched).
//! [`WidgetType::resolve_settings`] turns them into one typed value per declared setting:
//! a missing, mistyped or out-of-range override falls back to the declared default and is
//! reported, so a hand-edited config never breaks the widget.
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Surfaces a widget can live on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Panel,
    Desktop,
    Sidebar,
}

/// Width and height in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

/// Size constraints; an unbounded maximum is `f64::INFINITY`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeLimits {
    pub min: Size,
    pub max: Size,
    pub default: Size,
}

impl SizeLimits {
    pub fn clamp(&self, s: Size) -> Size {
        Size {
            w: s.w.clamp(self.min.w, self.max.w),
            h: s.h.clamp(self.min.h, self.max.h),
        }
    }

    fn valid(&self) -> bool {
        let ok = |min: f64, d: f64, max: f64| min >= 0.0 && min <= d && d <= max;
        ok(self.min.w, self.default.w, self.max.w) && ok(self.min.h, self.default.h, self.max.h)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingKind {
    Bool,
    Int {
        min: Option<i64>,
        max: Option<i64>,
    },
    Real {
        min: Option<f64>,
        max: Option<f64>,
    },
    Str,
    /// A QColor string: `#rgb`, `#rrggbb`, `#aarrggbb`, `#rrrgggbbb`, `#rrrrggggbbbb` or an SVG name.
    Color,
    Choice(Vec<String>),
}

impl SettingKind {
    pub fn name(&self) -> &'static str {
        match self {
            SettingKind::Bool => "bool",
            SettingKind::Int { .. } => "int",
            SettingKind::Real { .. } => "real",
            SettingKind::Str => "string",
            SettingKind::Color => "color",
            SettingKind::Choice(_) => "choice",
        }
    }

    /// The typed value of `v`, or why it is not one.
    pub fn check(&self, key: &str, v: &toml::Value) -> Result<SettingValue, SettingError> {
        let wrong = || SettingError::WrongType {
            key: key.to_string(),
            expected: self.name(),
        };
        let range = || SettingError::OutOfRange {
            key: key.to_string(),
        };
        match (self, v) {
            (SettingKind::Bool, toml::Value::Boolean(b)) => Ok(SettingValue::Bool(*b)),
            (SettingKind::Int { min, max }, toml::Value::Integer(i)) => {
                if min.is_some_and(|m| *i < m) || max.is_some_and(|m| *i > m) {
                    return Err(range());
                }
                Ok(SettingValue::Int(*i))
            }
            (SettingKind::Real { min, max }, toml::Value::Integer(_) | toml::Value::Float(_)) => {
                let x = match v {
                    toml::Value::Integer(i) => *i as f64,
                    toml::Value::Float(f) => *f,
                    _ => unreachable!(),
                };
                if !x.is_finite() || min.is_some_and(|m| x < m) || max.is_some_and(|m| x > m) {
                    return Err(range());
                }
                Ok(SettingValue::Real(x))
            }
            (SettingKind::Str, toml::Value::String(s)) => Ok(SettingValue::Str(s.clone())),
            (SettingKind::Color, toml::Value::String(s)) => {
                if crate::builtins::parse_color(s) == crate::color::Color::INVALID {
                    return Err(SettingError::BadColor {
                        key: key.to_string(),
                        value: s.clone(),
                    });
                }
                Ok(SettingValue::Color(s.trim().to_string()))
            }
            (SettingKind::Choice(opts), toml::Value::String(s)) => {
                if !opts.contains(s) {
                    return Err(SettingError::NotAChoice {
                        key: key.to_string(),
                        value: s.clone(),
                    });
                }
                Ok(SettingValue::Choice(s.clone()))
            }
            _ => Err(wrong()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(String),
    Color(String),
    Choice(String),
}

impl SettingValue {
    /// The QML value written to the widget's property of the same name; a color string
    /// converts like any string assigned to a `color` property.
    pub fn to_qml(&self) -> crate::value::Value {
        use crate::value::Value;
        match self {
            SettingValue::Bool(b) => Value::Bool(*b),
            SettingValue::Int(i) => Value::Num(*i as f64),
            SettingValue::Real(x) => Value::Num(*x),
            SettingValue::Str(s) | SettingValue::Color(s) | SettingValue::Choice(s) => {
                Value::str(s)
            }
        }
    }
}

/// The TOML value a QML setting value stands for, for the schema to check (the inverse of
/// [`SettingValue::to_qml`]): a bool, a string (a color reads as its name) or a number, an
/// integer when it has no fraction (JS has one number type). `None` for anything else (null,
/// undefined, an array, an object, an item): no setting kind holds it, so it is a schema error
/// ([`WidgetType::resolve_qml`]), never a value substituted for it.
pub fn qml_to_toml(v: &crate::value::Value) -> Option<toml::Value> {
    use crate::value::Value;
    // Every integral f64 in [-2^63, 2^63) converts to i64 exactly.
    const I64_END: f64 = 9_223_372_036_854_775_808.0;
    Some(match v {
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Num(n) if n.fract() == 0.0 && (-I64_END..I64_END).contains(n) => {
            toml::Value::Integer(*n as i64)
        }
        Value::Num(n) => toml::Value::Float(*n),
        Value::Str(s) => toml::Value::String(s.to_string()),
        Value::Color(_) => toml::Value::String(v.to_js_string()),
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingSpec {
    pub key: String,
    pub label: String,
    pub kind: SettingKind,
    pub default: SettingValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingError {
    Unknown { key: String },
    WrongType { key: String, expected: &'static str },
    OutOfRange { key: String },
    BadColor { key: String, value: String },
    NotAChoice { key: String, value: String },
}

impl fmt::Display for SettingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingError::Unknown { key } => write!(f, "unknown setting \"{key}\""),
            SettingError::WrongType { key, expected } => {
                write!(f, "setting \"{key}\" must be a {expected}")
            }
            SettingError::OutOfRange { key } => write!(f, "setting \"{key}\" is out of range"),
            SettingError::BadColor { key, value } => {
                write!(f, "setting \"{key}\": \"{value}\" is not a color")
            }
            SettingError::NotAChoice { key, value } => {
                write!(
                    f,
                    "setting \"{key}\": \"{value}\" is not one of the choices"
                )
            }
        }
    }
}

/// Settings after validation: one value per declared setting, in declaration order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    pub values: Vec<(String, SettingValue)>,
    pub errors: Vec<SettingError>,
}

impl Resolved {
    pub fn get(&self, key: &str) -> Option<&SettingValue> {
        self.values.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The values as the object a container publishes to QML (`settings`): every declared
    /// setting by key ([`SettingValue::to_qml`]).
    pub fn to_qml(&self) -> crate::value::Value {
        let o = crate::eval::new_obj();
        for (k, v) in &self.values {
            crate::eval::set_own(&o, k, v.to_qml());
        }
        crate::value::Value::Obj(o)
    }

    /// Logs the errors as those of layout widget `id` (a container publishing its settings).
    pub fn report(&self, id: &str, log: &mut impl Extend<String>) {
        log.extend(
            self.errors
                .iter()
                .map(|e| format!("layout widget \"{id}\": {e}")),
        );
    }

    /// The errors as the array of messages a container publishes to QML (`settingErrors`).
    pub fn errors_qml(&self) -> crate::value::Value {
        use crate::value::Value;
        Value::array(
            self.errors
                .iter()
                .map(|e| Value::str(&e.to_string()))
                .collect(),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WidgetType {
    /// Stable id a layout entry's `type` names.
    pub id: String,
    pub name: String,
    /// Material Symbols name, like the rest of the shell's icons.
    pub icon: String,
    pub placements: Vec<Placement>,
    pub size: SizeLimits,
    pub settings: Vec<SettingSpec>,
    /// The shell's QML component that renders the widget.
    pub component: PathBuf,
    /// In a panel, shares one group background with the adjacent `pill` widgets of its zone
    /// (the original's `BarGroup`).
    pub pill: bool,
    /// The config flag that turns the widget on and off, as a dotted path into the original's
    /// `Config.options` (`background.widgets.clock.enable`): the widget shows while it is not
    /// `false`. `None`: always shown.
    pub enabled_by: Option<String>,
}

impl WidgetType {
    pub fn allows(&self, p: Placement) -> bool {
        self.placements.contains(&p)
    }

    pub fn setting(&self, key: &str) -> Option<&SettingSpec> {
        self.settings.iter().find(|s| s.key == key)
    }

    pub fn resolve_settings(&self, overrides: &toml::Table) -> Resolved {
        let mut r = Resolved::default();
        for (k, _) in overrides.iter().filter(|(k, _)| self.setting(k).is_none()) {
            r.errors.push(SettingError::Unknown { key: k.clone() });
        }
        for s in &self.settings {
            let v = match overrides.get(&s.key).map(|v| s.kind.check(&s.key, v)) {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    r.errors.push(e);
                    s.default.clone()
                }
                None => s.default.clone(),
            };
            r.values.push((s.key.clone(), v));
        }
        r
    }

    /// [`Self::resolve_settings`] for settings set from QML (a `WidgetHost`'s `settings`): a
    /// value TOML cannot hold ([`qml_to_toml`]) is reported as mistyped (unknown when the
    /// type declares no such setting) and the setting keeps its default.
    pub fn resolve_qml<'a>(
        &self,
        settings: impl IntoIterator<Item = (&'a str, &'a crate::value::Value)>,
    ) -> Resolved {
        let mut overrides = toml::Table::new();
        let mut unheld = vec![];
        for (k, v) in settings {
            match qml_to_toml(v) {
                Some(t) => {
                    overrides.insert(k.to_string(), t);
                }
                None => unheld.push(match self.setting(k) {
                    Some(s) => SettingError::WrongType {
                        key: k.to_string(),
                        expected: s.kind.name(),
                    },
                    None => SettingError::Unknown { key: k.to_string() },
                }),
            }
        }
        let mut r = self.resolve_settings(&overrides);
        r.errors.extend(unheld);
        r
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RegistryError {
    Duplicate(String),
    NoPlacement(String),
    BadSize(String),
    EmptyChoice { widget: String, key: String },
    DuplicateSetting { widget: String, key: String },
    BadDefault { widget: String, error: SettingError },
    Manifest(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::Duplicate(id) => write!(f, "widget type \"{id}\" is registered twice"),
            RegistryError::NoPlacement(id) => write!(f, "widget type \"{id}\" has no placement"),
            RegistryError::BadSize(id) => {
                write!(f, "widget type \"{id}\": size needs min <= default <= max")
            }
            RegistryError::EmptyChoice { widget, key } => {
                write!(
                    f,
                    "widget type \"{widget}\": choice \"{key}\" has no options"
                )
            }
            RegistryError::DuplicateSetting { widget, key } => {
                write!(
                    f,
                    "widget type \"{widget}\": setting \"{key}\" is declared twice"
                )
            }
            RegistryError::BadDefault { widget, error } => {
                write!(f, "widget type \"{widget}\": default of {error}")
            }
            RegistryError::Manifest(e) => write!(f, "widgets.toml: {e}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Registry {
    types: BTreeMap<String, WidgetType>,
}

impl Registry {
    pub fn register(&mut self, t: WidgetType) -> Result<(), RegistryError> {
        if self.types.contains_key(&t.id) {
            return Err(RegistryError::Duplicate(t.id));
        }
        if t.placements.is_empty() {
            return Err(RegistryError::NoPlacement(t.id));
        }
        if !t.size.valid() {
            return Err(RegistryError::BadSize(t.id));
        }
        for (i, s) in t.settings.iter().enumerate() {
            if t.settings[..i].iter().any(|o| o.key == s.key) {
                return Err(RegistryError::DuplicateSetting {
                    widget: t.id.clone(),
                    key: s.key.clone(),
                });
            }
            if matches!(&s.kind, SettingKind::Choice(o) if o.is_empty()) {
                return Err(RegistryError::EmptyChoice {
                    widget: t.id.clone(),
                    key: s.key.clone(),
                });
            }
            let raw = match &s.default {
                SettingValue::Bool(b) => toml::Value::Boolean(*b),
                SettingValue::Int(i) => toml::Value::Integer(*i),
                SettingValue::Real(x) => toml::Value::Float(*x),
                SettingValue::Str(v) | SettingValue::Color(v) | SettingValue::Choice(v) => {
                    toml::Value::String(v.clone())
                }
            };
            if let Err(error) = s.kind.check(&s.key, &raw) {
                return Err(RegistryError::BadDefault {
                    widget: t.id.clone(),
                    error,
                });
            }
        }
        self.types.insert(t.id.clone(), t);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&WidgetType> {
        self.types.get(id)
    }

    /// A layout entry's settings as its container publishes them: its type's
    /// [`WidgetType::resolve_settings`] of the layout's raw overrides. An unknown type has no
    /// settings and no setting errors: its `WidgetHost` reports the type.
    pub fn resolve(&self, r#type: &str, overrides: &toml::Table) -> Resolved {
        self.get(r#type)
            .map(|t| t.resolve_settings(overrides))
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// All types, by id.
    pub fn iter(&self) -> impl Iterator<Item = &WidgetType> {
        self.types.values()
    }

    /// Types that may be placed in `p`, by id.
    pub fn for_placement(&self, p: Placement) -> impl Iterator<Item = &WidgetType> {
        self.iter().filter(move |t| t.allows(p))
    }

    /// Registers every type of a `widgets.toml` manifest; component paths are relative to
    /// `base`. A bad entry is reported and skipped, the others still register.
    pub fn load_manifest(&mut self, src: &str, base: &Path) -> Vec<RegistryError> {
        let m: Manifest = match toml::from_str(src) {
            Ok(m) => m,
            Err(e) => return vec![RegistryError::Manifest(e.to_string())],
        };
        let mut errs = vec![];
        for w in m.widget {
            match w.into_type(base).and_then(|t| self.register(t)) {
                Ok(()) => {}
                Err(e) => errs.push(e),
            }
        }
        errs
    }

    pub fn load_manifest_file(&mut self, f: &Path) -> Vec<RegistryError> {
        match std::fs::read_to_string(f) {
            Ok(src) => self.load_manifest(&src, f.parent().unwrap_or(Path::new("."))),
            Err(e) => vec![RegistryError::Manifest(format!("{}: {e}", f.display()))],
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(default)]
    widget: Vec<ManifestWidget>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestWidget {
    #[serde(rename = "type")]
    id: String,
    name: String,
    #[serde(default)]
    icon: String,
    placements: Vec<Placement>,
    component: PathBuf,
    size: ManifestSize,
    #[serde(default)]
    setting: Vec<ManifestSetting>,
    #[serde(default)]
    pill: bool,
    #[serde(default, rename = "enabledBy")]
    enabled_by: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestSize {
    min: [f64; 2],
    #[serde(default)]
    max: Option<[f64; 2]>,
    default: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestSetting {
    key: String,
    #[serde(default)]
    label: String,
    kind: String,
    default: toml::Value,
    #[serde(default)]
    min: Option<toml::Value>,
    #[serde(default)]
    max: Option<toml::Value>,
    #[serde(default)]
    options: Vec<String>,
}

impl ManifestWidget {
    fn into_type(self, base: &Path) -> Result<WidgetType, RegistryError> {
        let id = self.id;
        let bad = |e: String| RegistryError::Manifest(format!("widget \"{id}\": {e}"));
        let size = |[w, h]: [f64; 2]| Size { w, h };
        if let Some(path) = &self.enabled_by {
            let ident = |k: &str| {
                k.chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            };
            if !path.split('.').all(ident) {
                return Err(bad(format!(
                    "enabledBy \"{path}\" is not a dotted path of config keys"
                )));
            }
        }
        let mut settings = vec![];
        for s in self.setting {
            let int = |v: &Option<toml::Value>| match v {
                None => Ok(None),
                Some(toml::Value::Integer(i)) => Ok(Some(*i)),
                Some(_) => Err(bad(format!(
                    "setting \"{}\": int bounds must be integers",
                    s.key
                ))),
            };
            let real = |v: &Option<toml::Value>| match v {
                None => Ok(None),
                Some(toml::Value::Integer(i)) => Ok(Some(*i as f64)),
                Some(toml::Value::Float(f)) => Ok(Some(*f)),
                Some(_) => Err(bad(format!(
                    "setting \"{}\": real bounds must be numbers",
                    s.key
                ))),
            };
            let kind = match s.kind.as_str() {
                "bool" => SettingKind::Bool,
                "int" => SettingKind::Int {
                    min: int(&s.min)?,
                    max: int(&s.max)?,
                },
                "real" => SettingKind::Real {
                    min: real(&s.min)?,
                    max: real(&s.max)?,
                },
                "string" => SettingKind::Str,
                "color" => SettingKind::Color,
                "choice" => SettingKind::Choice(s.options.clone()),
                k => return Err(bad(format!("setting \"{}\": unknown kind \"{k}\"", s.key))),
            };
            if !matches!(kind, SettingKind::Choice(_)) && !s.options.is_empty() {
                return Err(bad(format!(
                    "setting \"{}\": options need kind = \"choice\"",
                    s.key
                )));
            }
            let default =
                kind.check(&s.key, &s.default)
                    .map_err(|error| RegistryError::BadDefault {
                        widget: id.clone(),
                        error,
                    })?;
            settings.push(SettingSpec {
                label: if s.label.is_empty() {
                    s.key.clone()
                } else {
                    s.label
                },
                key: s.key,
                kind,
                default,
            });
        }
        Ok(WidgetType {
            name: self.name,
            icon: self.icon,
            placements: self.placements,
            size: SizeLimits {
                min: size(self.size.min),
                max: self.size.max.map_or(
                    Size {
                        w: f64::INFINITY,
                        h: f64::INFINITY,
                    },
                    size,
                ),
                default: size(self.size.default),
            },
            settings,
            component: base.join(self.component),
            pill: self.pill,
            enabled_by: self.enabled_by,
            id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> WidgetType {
        WidgetType {
            id: "clock".into(),
            name: "Clock".into(),
            icon: "schedule".into(),
            placements: vec![Placement::Panel, Placement::Desktop],
            size: SizeLimits {
                min: Size { w: 80.0, h: 24.0 },
                max: Size {
                    w: 400.0,
                    h: f64::INFINITY,
                },
                default: Size { w: 120.0, h: 32.0 },
            },
            settings: vec![
                SettingSpec {
                    key: "seconds".into(),
                    label: "Show seconds".into(),
                    kind: SettingKind::Bool,
                    default: SettingValue::Bool(false),
                },
                SettingSpec {
                    key: "digits".into(),
                    label: "Digits".into(),
                    kind: SettingKind::Int {
                        min: Some(1),
                        max: Some(4),
                    },
                    default: SettingValue::Int(2),
                },
            ],
            component: "Clock.qml".into(),
            pill: false,
            enabled_by: None,
        }
    }

    fn table(src: &str) -> toml::Table {
        toml::from_str(src).unwrap()
    }

    #[test]
    fn register_and_look_up_by_type() {
        let mut r = Registry::default();
        r.register(clock()).unwrap();
        assert_eq!(r.get("clock").unwrap().name, "Clock");
        assert!(r.get("weather").is_none());
        assert_eq!(
            r.register(clock()),
            Err(RegistryError::Duplicate("clock".into()))
        );
        let panel: Vec<_> = r.for_placement(Placement::Panel).map(|t| &t.id).collect();
        assert_eq!(panel, ["clock"]);
        assert_eq!(r.for_placement(Placement::Sidebar).count(), 0);
    }

    #[test]
    fn register_rejects_inconsistent_types() {
        let mut r = Registry::default();
        let mut t = clock();
        t.placements.clear();
        assert_eq!(
            r.register(t),
            Err(RegistryError::NoPlacement("clock".into()))
        );
        let mut t = clock();
        t.size.default.w = 10.0;
        assert_eq!(r.register(t), Err(RegistryError::BadSize("clock".into())));
        let mut t = clock();
        t.settings[1].default = SettingValue::Int(9);
        assert!(matches!(
            r.register(t),
            Err(RegistryError::BadDefault { .. })
        ));
        let mut t = clock();
        t.settings[1].key = "seconds".into();
        assert!(matches!(
            r.register(t),
            Err(RegistryError::DuplicateSetting { .. })
        ));
        let mut t = clock();
        t.settings[0].kind = SettingKind::Choice(vec![]);
        assert!(matches!(
            r.register(t),
            Err(RegistryError::EmptyChoice { .. })
        ));
        assert!(r.is_empty());
    }

    #[test]
    fn resolve_fills_defaults_and_keeps_valid_overrides() {
        let r = clock().resolve_settings(&table("digits = 3"));
        assert!(r.errors.is_empty());
        assert_eq!(r.get("seconds"), Some(&SettingValue::Bool(false)));
        assert_eq!(r.get("digits"), Some(&SettingValue::Int(3)));
        let keys: Vec<_> = r.values.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["seconds", "digits"], "declaration order");
    }

    #[test]
    fn resolve_reports_bad_overrides_and_uses_defaults() {
        let r = clock().resolve_settings(&table("seconds = 1\ndigits = 7\nextra = true"));
        assert_eq!(r.get("seconds"), Some(&SettingValue::Bool(false)));
        assert_eq!(r.get("digits"), Some(&SettingValue::Int(2)));
        assert_eq!(
            r.errors,
            [
                SettingError::Unknown {
                    key: "extra".into()
                },
                SettingError::WrongType {
                    key: "seconds".into(),
                    expected: "bool"
                },
                SettingError::OutOfRange {
                    key: "digits".into()
                },
            ]
        );
    }

    #[test]
    fn qml_values_convert_to_the_toml_the_schema_checks() {
        use crate::value::Value;
        for (kind, v) in [
            (SettingKind::Bool, SettingValue::Bool(true)),
            (
                SettingKind::Int {
                    min: None,
                    max: None,
                },
                SettingValue::Int(-3),
            ),
            (
                SettingKind::Real {
                    min: None,
                    max: None,
                },
                SettingValue::Real(2.5),
            ),
            (SettingKind::Str, SettingValue::Str("a".into())),
            (SettingKind::Color, SettingValue::Color("#ff8800".into())),
            (
                SettingKind::Choice(vec!["x".into()]),
                SettingValue::Choice("x".into()),
            ),
        ] {
            let back = qml_to_toml(&v.to_qml()).unwrap();
            assert_eq!(kind.check("k", &back), Ok(v.clone()), "{v:?}");
        }
        // JS has one number type: integral numbers are integers, as far as i64 reaches.
        assert_eq!(qml_to_toml(&Value::Num(4.0)), Some(toml::Value::Integer(4)));
        assert_eq!(
            qml_to_toml(&Value::Num(-9_223_372_036_854_775_808.0)),
            Some(toml::Value::Integer(i64::MIN))
        );
        assert_eq!(
            qml_to_toml(&Value::Num(9_223_372_036_854_775_808.0)),
            Some(toml::Value::Float(9_223_372_036_854_775_808.0))
        );
        assert!(matches!(
            qml_to_toml(&Value::Num(f64::NAN)),
            Some(toml::Value::Float(x)) if x.is_nan()
        ));
        for v in [
            Value::Null,
            Value::Undefined,
            Value::array(vec![]),
            Value::Obj(crate::eval::new_obj()),
        ] {
            assert_eq!(qml_to_toml(&v), None, "{v:?}");
        }
    }

    #[test]
    fn values_no_setting_can_hold_are_schema_errors_from_qml_and_toml() {
        use crate::value::Value;
        let list = Value::array(vec![Value::Num(1.0)]);
        let r = clock().resolve_qml([
            ("digits", &Value::Num(3.0)),
            ("seconds", &list),
            ("extra", &Value::Null),
        ]);
        assert_eq!(r.get("digits"), Some(&SettingValue::Int(3)));
        assert_eq!(r.get("seconds"), Some(&SettingValue::Bool(false)));
        assert!(r.errors.contains(&SettingError::WrongType {
            key: "seconds".into(),
            expected: "bool"
        }));
        assert!(r.errors.contains(&SettingError::Unknown {
            key: "extra".into()
        }));
        assert_eq!(r.errors.len(), 2);
        // The same values from a layout file: arrays, tables and datetimes are rejected,
        // not substituted.
        let r = clock().resolve_settings(&table(
            "seconds = [1]\ndigits = { n = 3 }\n[x]\nwhen = 1979-05-27",
        ));
        assert_eq!(r.get("seconds"), Some(&SettingValue::Bool(false)));
        assert_eq!(r.get("digits"), Some(&SettingValue::Int(2)));
        assert_eq!(r.errors.len(), 3, "{:?}", r.errors);
    }

    #[test]
    fn resolved_settings_publish_values_and_errors_to_qml() {
        let r = clock().resolve_settings(&table("digits = 9"));
        let crate::value::Value::Obj(o) = r.to_qml() else {
            panic!()
        };
        let o = o.borrow();
        assert_eq!(o.props["seconds"].to_js_string(), "false");
        assert_eq!(o.props["digits"].to_js_string(), "2");
        assert_eq!(
            r.errors_qml().to_js_string(),
            SettingError::OutOfRange {
                key: "digits".into()
            }
            .to_string()
        );
        let mut log = vec![];
        r.report("c", &mut log);
        assert_eq!(log.len(), 1);
        assert!(log[0].starts_with("layout widget \"c\": "), "{log:?}");
        let unknown = Registry::default().resolve("nope", &table("a = 1"));
        assert!(unknown.values.is_empty() && unknown.errors.is_empty());
    }

    #[test]
    fn kinds_check_values() {
        let real = SettingKind::Real {
            min: Some(0.0),
            max: Some(1.0),
        };
        let v = |s: &str| table(&format!("v = {s}"))["v"].clone();
        assert_eq!(real.check("k", &v("1")), Ok(SettingValue::Real(1.0)));
        assert_eq!(real.check("k", &v("0.25")), Ok(SettingValue::Real(0.25)));
        assert!(real.check("k", &v("nan")).is_err());
        assert!(real.check("k", &v("1.5")).is_err());
        let int = SettingKind::Int {
            min: None,
            max: None,
        };
        assert!(int.check("k", &v("1.5")).is_err(), "no silent truncation");
        let color = SettingKind::Color;
        for ok in ["\"#abc\"", "\"#80ff0000\"", "\"steelblue\""] {
            assert!(color.check("k", &v(ok)).is_ok(), "{ok}");
        }
        for bad in ["\"#ggg\"", "\"\"", "\"blu\""] {
            assert!(color.check("k", &v(bad)).is_err(), "{bad}");
        }
        let choice = SettingKind::Choice(vec!["a".into(), "b".into()]);
        assert_eq!(
            choice.check("k", &v("\"b\"")),
            Ok(SettingValue::Choice("b".into()))
        );
        assert!(choice.check("k", &v("\"c\"")).is_err());
        assert!(SettingKind::Str.check("k", &v("3")).is_err());
    }

    #[test]
    fn size_clamps_to_limits() {
        let l = clock().size;
        assert_eq!(l.clamp(Size { w: 10.0, h: 1e6 }), Size { w: 80.0, h: 1e6 });
        assert_eq!(
            l.clamp(Size { w: 999.0, h: 0.0 }),
            Size { w: 400.0, h: 24.0 }
        );
    }

    #[test]
    fn manifest_builds_types() {
        let mut r = Registry::default();
        let errs = r.load_manifest(
            r#"
            [[widget]]
            type = "media"
            name = "Media"
            icon = "music_note"
            placements = ["panel", "sidebar"]
            component = "widgets/Media.qml"
            size = { min = [100, 24], default = [200, 32] }
            [[widget.setting]]
            key = "source"
            kind = "choice"
            options = ["mpris", "none"]
            default = "mpris"
            [[widget.setting]]
            key = "opacity"
            kind = "real"
            min = 0
            max = 1
            default = 0.8
            "#,
            Path::new("/shell"),
        );
        assert!(errs.is_empty(), "{errs:?}");
        let t = r.get("media").unwrap();
        assert_eq!(t.component, Path::new("/shell/widgets/Media.qml"));
        assert_eq!(t.size.max.w, f64::INFINITY);
        assert!(t.allows(Placement::Sidebar) && !t.allows(Placement::Desktop));
        assert_eq!(
            t.setting("opacity").unwrap().default,
            SettingValue::Real(0.8)
        );
        assert_eq!(t.setting("source").unwrap().label, "source");
        assert_eq!(t.enabled_by, None, "no enabledBy: always shown");
    }

    #[test]
    fn manifest_reads_the_flag_a_type_is_enabled_by() {
        let entry = |path: &str| {
            format!(
                "[[widget]]\ntype = \"w\"\nname = \"W\"\nplacements = [\"desktop\"]\n\
                 component = \"W.qml\"\nsize = {{ min = [1, 1], default = [1, 1] }}\n\
                 enabledBy = \"{path}\"\n"
            )
        };
        let mut r = Registry::default();
        let errs = r.load_manifest(&entry("background.widgets.w.enable"), Path::new("."));
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(
            r.get("w").unwrap().enabled_by.as_deref(),
            Some("background.widgets.w.enable")
        );
        for bad in [
            "",
            "background..enable",
            "background.widgets.w.enable.",
            "a.1b",
        ] {
            let mut r = Registry::default();
            let errs = r.load_manifest(&entry(bad), Path::new("."));
            assert!(
                matches!(&errs[..], [RegistryError::Manifest(e)] if e.contains("enabledBy")),
                "{bad:?}: {errs:?}"
            );
            assert!(r.get("w").is_none());
        }
    }

    #[test]
    fn manifest_errors_skip_only_the_bad_entry() {
        let mut r = Registry::default();
        let errs = r.load_manifest(
            r#"
            [[widget]]
            type = "a"
            name = "A"
            placements = ["panel"]
            component = "A.qml"
            size = { min = [1, 1], default = [1, 1] }
            [[widget.setting]]
            key = "x"
            kind = "number"
            default = 1
            [[widget]]
            type = "b"
            name = "B"
            placements = ["desktop"]
            component = "B.qml"
            size = { min = [1, 1], default = [1, 1] }
            "#,
            Path::new("."),
        );
        assert_eq!(errs.len(), 1);
        assert!(
            errs[0].to_string().contains("unknown kind \"number\""),
            "{}",
            errs[0]
        );
        assert!(r.get("a").is_none() && r.get("b").is_some());
        let errs = r.load_manifest("[[widget]]\ntype = 3\n", Path::new("."));
        assert!(matches!(errs[..], [RegistryError::Manifest(_)]));
        let errs = r.load_manifest("[[widgets]]\n", Path::new("."));
        assert!(
            matches!(errs[..], [RegistryError::Manifest(_)]),
            "unknown keys are errors"
        );
    }
}
