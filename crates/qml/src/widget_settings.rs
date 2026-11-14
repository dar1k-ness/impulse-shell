//! `Impulse/WidgetSettings`: the settings of one layout widget, generated from its type's schema
//! (`widgets.toml`). Set `connector` and `widgetId`; it publishes `valid`, `widgetType`, `title`,
//! `icon`, `canUndo`, `canRedo` and `fields`, one per declared setting in declaration order:
//! `{ key, label, kind, value, min, max, options, error, input, isDefault }`. `kind` is `bool`,
//! `int`, `real`, `str`, `color` or `choice`; `min`/`max` are `null` when unbounded.
//!
//! `setValue(key, value)` checks the value against the schema. An accepted value is one undo
//! step (`Edit::SetSettings`) written to `layout.toml`; a rejected one writes nothing and shows
//! its message in the field's `error`, with the rejected text in `input`, until the field is
//! edited again or the dialog moves to another widget. `resetValue(key)` drops the override, so
//! the type's default applies. `undo()`/`redo()` step the layout's shared history.

use std::collections::BTreeMap;

use crate::layout_model::{Edit, Widget};
use crate::runtime::World;
use crate::value::Value;
use crate::widgets::{SettingError, SettingKind, SettingSpec, WidgetType};

pub(crate) const TYPE: &str = "WidgetSettings";

/// Per item: the widget the drafts belong to (`connector`, `widgetId`) and the rejected inputs,
/// by setting key: (what was entered, why it was rejected).
pub type Drafts = ((String, String), BTreeMap<String, (String, String)>);

fn is_settings(w: &World, item: usize) -> bool {
    w.items
        .get(item)
        .is_some_and(|i| i.types.iter().any(|t| t == TYPE))
}

fn text(w: &mut World, item: usize, k: &str) -> String {
    match w.read(item, k) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let o = crate::eval::new_obj();
    for (k, v) in pairs {
        crate::eval::set_own(&o, k, v);
    }
    Value::Obj(o)
}

fn find<'a>(w: &'a World, connector: &str, id: &str) -> Option<&'a Widget> {
    w.layout
        .as_ref()?
        .monitor(connector)?
        .containers
        .iter()
        .flat_map(|c| c.widgets.iter())
        .find(|wd| wd.id == id)
}

fn kind_name(k: &SettingKind) -> &'static str {
    match k {
        SettingKind::Bool => "bool",
        SettingKind::Int { .. } => "int",
        SettingKind::Real { .. } => "real",
        SettingKind::Str => "str",
        SettingKind::Color => "color",
        SettingKind::Choice(_) => "choice",
    }
}

fn bounds(k: &SettingKind) -> (Option<f64>, Option<f64>) {
    match k {
        SettingKind::Int { min, max } => (min.map(|v| v as f64), max.map(|v| v as f64)),
        SettingKind::Real { min, max } => (*min, *max),
        _ => (None, None),
    }
}

fn num_text(v: f64) -> String {
    Value::Num(v).to_js_string()
}

/// The message shown under a field; the dialog names the setting already.
fn message(spec: &SettingSpec, e: &SettingError) -> String {
    match e {
        SettingError::Unknown { .. } => "Unknown setting".into(),
        SettingError::WrongType { expected, .. } => match *expected {
            "bool" => "Must be on or off".into(),
            "int" => "Must be a whole number".into(),
            "real" => "Must be a number".into(),
            "string" | "color" | "choice" => "Must be text".into(),
            other => format!("Must be {}", article(other)),
        },
        SettingError::OutOfRange { .. } => match bounds(&spec.kind) {
            (Some(lo), Some(hi)) => {
                format!("Must be between {} and {}", num_text(lo), num_text(hi))
            }
            (Some(lo), None) => format!("Must be at least {}", num_text(lo)),
            (None, Some(hi)) => format!("Must be at most {}", num_text(hi)),
            (None, None) => "Out of range".into(),
        },
        SettingError::BadColor { value, .. } => format!("\"{value}\" is not a color"),
        SettingError::NotAChoice { value, .. } => format!("\"{value}\" is not one of the options"),
    }
}

fn article(noun: &str) -> String {
    match noun.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => format!("an {noun}"),
        _ => format!("a {noun}"),
    }
}

/// The TOML value a QML value stands for in a setting of kind `k`. Text typed into a number
/// field is parsed; anything else keeps its own type and fails the schema check.
fn to_toml(k: &SettingKind, v: &Value) -> toml::Value {
    let parse_num = |s: &str| s.trim().parse::<f64>().ok().filter(|n| n.is_finite());
    match (k, v) {
        (_, Value::Bool(b)) => toml::Value::Boolean(*b),
        (SettingKind::Int { .. }, Value::Num(n)) if n.fract() == 0.0 && n.abs() < 9.0e15 => {
            toml::Value::Integer(*n as i64)
        }
        (SettingKind::Int { .. }, Value::Str(s)) => match s.trim().parse::<i64>() {
            Ok(i) => toml::Value::Integer(i),
            Err(_) => toml::Value::String(s.to_string()),
        },
        (SettingKind::Real { .. }, Value::Str(s)) => match parse_num(s) {
            Some(n) => toml::Value::Float(n),
            None => toml::Value::String(s.to_string()),
        },
        (_, Value::Num(n)) => toml::Value::Float(*n),
        (_, Value::Str(s)) => toml::Value::String(s.to_string()),
        (_, other) => toml::Value::String(other.to_js_string()),
    }
}

fn input_text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        other => other.to_js_string(),
    }
}

fn fields(
    ty: &WidgetType,
    wd: &Widget,
    drafts: Option<&BTreeMap<String, (String, String)>>,
) -> Value {
    let resolved = ty.resolve_settings(&wd.settings);
    let stored_error = |spec: &SettingSpec| {
        resolved.errors.iter().find_map(|e| {
            let key = match e {
                SettingError::Unknown { key }
                | SettingError::WrongType { key, .. }
                | SettingError::OutOfRange { key }
                | SettingError::BadColor { key, .. }
                | SettingError::NotAChoice { key, .. } => key,
            };
            (key == &spec.key).then(|| format!("Saved value ignored: {}", message(spec, e)))
        })
    };
    let list = ty
        .settings
        .iter()
        .map(|spec| {
            let value = resolved.get(&spec.key).unwrap_or(&spec.default).to_qml();
            let (min, max) = bounds(&spec.kind);
            let opt = |b: Option<f64>| b.map_or(Value::Null, Value::Num);
            let options = match &spec.kind {
                SettingKind::Choice(c) => Value::array(c.iter().map(|s| Value::str(s)).collect()),
                _ => Value::array(Vec::new()),
            };
            let draft = drafts.and_then(|d| d.get(&spec.key));
            let (input, error) = match draft {
                Some((input, error)) => (input.clone(), error.clone()),
                None => (String::new(), stored_error(spec).unwrap_or_default()),
            };
            obj(vec![
                ("key", Value::str(&spec.key)),
                ("label", Value::str(&spec.label)),
                ("kind", Value::str(kind_name(&spec.kind))),
                ("value", value),
                ("min", opt(min)),
                ("max", opt(max)),
                ("options", options),
                ("error", Value::str(&error)),
                ("input", Value::str(&input)),
                (
                    "isDefault",
                    Value::Bool(!wd.settings.contains_key(&spec.key)),
                ),
            ])
        })
        .collect();
    Value::array(list)
}

/// Recompute the outputs when the layout, `connector` or `widgetId` changed (or `force`).
pub(crate) fn sync(w: &mut World, item: usize, force: bool) -> bool {
    let generation = w.layout.as_ref().map_or(0, |l| l.generation());
    let target = (text(w, item, "connector"), text(w, item, "widgetId"));
    let key = (generation, target.0.clone(), target.1.clone());
    if !force && w.widget_settings_synced.get(&item) == Some(&key) {
        return false;
    }
    if w.widget_settings_drafts
        .get(&item)
        .is_some_and(|(t, _)| *t != target)
    {
        w.widget_settings_drafts.remove(&item);
    }
    let widget = find(w, &target.0, &target.1).cloned();
    let ty = widget
        .as_ref()
        .and_then(|wd| w.widgets.get(&wd.r#type))
        .cloned();
    let (can_undo, can_redo) = w.layout.as_ref().map_or((false, false), |l| {
        (l.editor().can_undo(), l.editor().can_redo())
    });
    let drafts = w.widget_settings_drafts.get(&item).map(|(_, d)| d);
    let out: [(&str, &str, Value); 7] = match (&widget, &ty) {
        (Some(wd), Some(ty)) => [
            ("valid", "bool", Value::Bool(true)),
            ("widgetType", "string", Value::str(&ty.id)),
            ("title", "string", Value::str(&ty.name)),
            ("icon", "string", Value::str(&ty.icon)),
            ("fields", "var", fields(ty, wd, drafts)),
            ("canUndo", "bool", Value::Bool(can_undo)),
            ("canRedo", "bool", Value::Bool(can_redo)),
        ],
        _ => [
            ("valid", "bool", Value::Bool(false)),
            ("widgetType", "string", Value::str("")),
            ("title", "string", Value::str("")),
            ("icon", "string", Value::str("")),
            ("fields", "var", Value::array(Vec::new())),
            ("canUndo", "bool", Value::Bool(can_undo)),
            ("canRedo", "bool", Value::Bool(can_redo)),
        ],
    };
    for (k, ty, v) in out {
        if !w.items[item].props.contains_key(k) {
            w.declare(item, k, ty);
        }
        w.write(item, k, v);
    }
    w.widget_settings_synced.insert(item, key);
    true
}

fn report(w: &mut World, r: Result<bool, String>) -> bool {
    match r {
        Ok(changed) => changed,
        Err(e) => {
            w.errors.push(format!("WidgetSettings: {e}"));
            true
        }
    }
}

fn set_settings(w: &mut World, connector: &str, id: &str, settings: toml::Table) -> bool {
    let Some(live) = w.layout.as_mut() else {
        return false;
    };
    let monitor = live.attach(connector);
    let edit = Edit::SetSettings {
        monitor,
        id: id.to_string(),
        settings,
    };
    match live.apply(&[edit]) {
        Ok(r) => report(w, r),
        Err(e) => {
            w.errors.push(format!("WidgetSettings: {e}"));
            false
        }
    }
}

/// `setValue`, `resetValue`, `undo`, `redo`; each returns whether the layout changed.
/// `args: None` only asks whether the method exists (see `Host::native`).
pub(crate) fn call(
    w: &mut World,
    item: usize,
    name: &str,
    args: Option<&[Value]>,
) -> Option<Value> {
    if !matches!(name, "setValue" | "resetValue" | "undo" | "redo") || !is_settings(w, item) {
        return None;
    }
    let Some(args) = args else {
        return Some(Value::Undefined);
    };
    let target = (text(w, item, "connector"), text(w, item, "widgetId"));
    let key = match args.first() {
        Some(Value::Str(s)) => s.to_string(),
        Some(v) => v.to_js_string(),
        None => String::new(),
    };
    let changed = match name {
        "undo" | "redo" => {
            w.widget_settings_drafts.remove(&item);
            match w.layout.as_mut() {
                Some(live) => {
                    let r = if name == "undo" {
                        live.undo()
                    } else {
                        live.redo()
                    };
                    report(w, r)
                }
                None => false,
            }
        }
        _ => {
            let Some(wd) = find(w, &target.0, &target.1).cloned() else {
                return Some(Value::Bool(false));
            };
            let Some(spec) = w
                .widgets
                .get(&wd.r#type)
                .and_then(|t| t.setting(&key))
                .cloned()
            else {
                return Some(Value::Bool(false));
            };
            if let Some((t, d)) = w.widget_settings_drafts.get_mut(&item)
                && *t == target
            {
                d.remove(&key);
            }
            let mut settings = wd.settings.clone();
            if name == "resetValue" {
                settings
                    .remove(&key)
                    .is_some_and(|_| set_settings(w, &target.0, &target.1, settings))
            } else {
                let v = args.get(1).cloned().unwrap_or(Value::Undefined);
                let tv = to_toml(&spec.kind, &v);
                match spec.kind.check(&key, &tv) {
                    Ok(_) if wd.settings.get(&key) == Some(&tv) => false,
                    Ok(_) => {
                        settings.insert(key, tv);
                        set_settings(w, &target.0, &target.1, settings)
                    }
                    Err(e) => {
                        let msg = message(&spec, &e);
                        let drafts = w
                            .widget_settings_drafts
                            .entry(item)
                            .or_insert_with(|| (target.clone(), BTreeMap::new()));
                        drafts.1.insert(key, (input_text(&v), msg));
                        false
                    }
                }
            }
        }
    };
    sync(w, item, true);
    Some(Value::Bool(changed))
}
