//! Semantic type registry for C++-backed Qt/Quickshell types, built from the `.qmltypes`
//! metadata shipped by Qt 6.12 and Quickshell (copied verbatim into `qml/qmltypes`).
//! Enum numeric values come from `enums.tsv`, dumped from the real engine (`enumdump`).
//! `.qmltypes` lists every slot without its access; `private_methods.tsv` names the private
//! ones (`tools/gen/qmltypes/private_methods.py`, from Qt's meta-objects), which QML does not see.
use crate::qtobj::QtGlobal;
use rustc_hash::FxHashMap as HashMap;

mod data {
    include!(concat!(env!("OUT_DIR"), "/qmltypes.rs"));
}

#[derive(Debug, Clone, Default)]
pub struct PropInfo {
    pub name: String,
    pub ty: String,
    pub list: bool,
    pub readonly: bool,
    /// The property has a BINDABLE accessor: a binding observes it with a
    /// QPropertyChangeTrigger, and an observer hears a change before the NOTIFY signal.
    pub bindable: bool,
    /// Qt declares it BINDABLE, in any module (`bindable` is only kept for Quickshell's): its
    /// binding is a QQmlPropertyBinding, which a creation sets after the other bindings, and
    /// an observer of it hears a change before the NOTIFY signal.
    pub qproperty: bool,
    /// `ty` names an enum, looked up from the declaring class as C++ does (`Registry::load`).
    pub enumeration: bool,
    /// Has a RESET function: writing `undefined` calls it.
    pub resettable: bool,
    /// Has a NOTIFY signal: its setter compares the value with the one it holds.
    pub notify: bool,
    /// `FINAL`: a property of this name declared in QML is a load error.
    pub is_final: bool,
    /// A constant read-only pointer to an object of a class QML cannot make (`MouseArea.drag`,
    /// `Flickable.visibleArea`, `DragHandler.xAxis`): the object is part of its owner, there
    /// from the start and never null. Qt's own types only (`Registry::load`).
    pub part: bool,
}

#[derive(Debug, Clone, Default)]
pub struct EnumInfo {
    pub name: String,
    /// The enum a `Q_DECLARE_FLAGS` type wraps (`Alignment` → `AlignmentFlag`).
    pub alias: Option<String>,
    pub values: Vec<String>,
    pub flag: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParamInfo {
    pub name: String,
    /// The C++ type as `QMetaMethod::methodSignature` writes it (`QObject*`, `QList<int>`).
    pub ty: String,
}

/// A `Signal { name; Parameter { name; type } ... }` of a C++ type (qmltypes "Signal").
#[derive(Debug, Clone, Default)]
pub struct SignalInfo {
    pub name: String,
    /// `Parameter` names in declaration order: a handler's parameters, and the argument count a
    /// call from JS must supply (QV4 CallPrecise).
    pub params: Vec<ParamInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct TypeInfo {
    pub cpp: String,
    pub prototype: Option<String>,
    /// Extension object/namespace whose members (e.g. enums) are exposed on this type.
    pub extension: Option<String>,
    pub attached: Option<String>,
    pub default_prop: Option<String>,
    pub props: Vec<PropInfo>,
    pub enums: Vec<EnumInfo>,
    pub methods: Vec<String>,
    /// The overloads of `methods` with their parameters, in declaration order (a default
    /// argument declares one more). Not the JavaScript functions (QQmlV4Function), which take
    /// any arguments.
    pub method_sigs: Vec<SignalInfo>,
    pub signals: Vec<SignalInfo>,
    pub singleton: bool,
    pub creatable: bool,
    /// (module uri, qml name)
    pub exports: Vec<(String, String)>,
}

#[derive(Default)]
pub struct Registry {
    pub by_cpp: HashMap<String, TypeInfo>,
    /// QML name → (module, cpp name)
    pub by_name: HashMap<String, Vec<(String, String)>>,
    /// (module, qml name, value) → number, ground truth from the real engine.
    pub enum_values: HashMap<(String, String, String), f64>,
    /// Every C++ property name → whether some property of that name is an object model.
    /// Lookups of names no type declares skip the per-type chain walk.
    pub prop_names: HashMap<String, bool>,
    pub signal_names: rustc_hash::FxHashSet<String>,
    pub method_names: rustc_hash::FxHashSet<String>,
}

/// Minimal line parser for `.qmltypes` (machine-generated, one `key: value` per line,
/// arrays possibly spanning lines); much faster than the general QML/JS front end.
fn parse_qmltypes(src: &str) -> Vec<TypeInfo> {
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut cur: Option<TypeInfo> = None;
    let mut sub: HashMap<String, String> = HashMap::default();
    // Fields of the enclosing blocks: a `Signal`'s name outlives its multi-line `Parameter`s.
    let mut outer: Vec<HashMap<String, String>> = Vec::new();
    let mut pending: Option<(String, String)> = None;
    // `Parameter` names of the enclosing `Signal`/`Method` block.
    let mut params: Vec<ParamInfo> = Vec::new();
    for line in src.lines() {
        let l = line.trim();
        // The file is QML: a comment is no part of it, whatever it quotes.
        if l.starts_with("//") {
            continue;
        }
        if let Some((k, acc)) = &mut pending {
            acc.push_str(l);
            if l.ends_with(']') {
                let (k, v) = (k.clone(), acc.clone());
                pending = None;
                set_field(&stack, &mut cur, &mut sub, &k, &v);
            }
            continue;
        }
        if let Some((block, body)) = l.strip_suffix('}').and_then(|x| x.split_once('{')) {
            let block = block.trim().to_string();
            let mut fields = HashMap::default();
            for f in body.split(';') {
                if let Some((k, v)) = f.split_once(':') {
                    fields.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
            let saved = std::mem::replace(&mut sub, fields);
            stack.push(block);
            close_block(&mut stack, &mut cur, &mut sub, &mut params, &mut out);
            sub = saved;
            continue;
        }
        if let Some(block) = l.strip_suffix('{') {
            let block = block.trim().to_string();
            if block == "Component" {
                cur = Some(TypeInfo {
                    creatable: true,
                    ..Default::default()
                });
            }
            outer.push(std::mem::take(&mut sub));
            stack.push(block);
            continue;
        }
        if l == "}" {
            close_block(&mut stack, &mut cur, &mut sub, &mut params, &mut out);
            sub = outer.pop().unwrap_or_default();
            continue;
        }
        let Some((k, v)) = l.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        if v.starts_with('[') && !v.ends_with(']') {
            pending = Some((k.to_string(), v.to_string()));
            continue;
        }
        set_field(&stack, &mut cur, &mut sub, k, v);
    }
    out
}

fn close_block(
    stack: &mut Vec<String>,
    cur: &mut Option<TypeInfo>,
    sub: &mut HashMap<String, String>,
    params: &mut Vec<ParamInfo>,
    out: &mut Vec<TypeInfo>,
) {
    let b = stack.pop().unwrap_or_default();
    let get = |k: &str| sub.get(k).cloned().unwrap_or_default();
    if b == "Parameter" {
        let mut ty = unquote(&get("type"));
        if get("isPointer") == "true" {
            ty += "*";
        }
        if get("isTypeConstant") == "true" {
            ty = format!("const {ty}");
        }
        if get("isList") == "true" {
            ty = format!("QList<{ty}>");
        }
        params.push(ParamInfo {
            name: unquote(&get("name")),
            ty,
        });
        return;
    }
    let params = std::mem::take(params);
    match (b.as_str(), cur.as_mut()) {
        ("Component", _) => out.extend(cur.take()),
        ("Property", Some(t)) => t.props.push(PropInfo {
            name: unquote(&get("name")),
            ty: unquote(&get("type")) + if get("isPointer") == "true" { "*" } else { "" },
            list: get("isList") == "true",
            readonly: get("isReadonly") == "true",
            bindable: !get("bindable").is_empty(),
            qproperty: !get("bindable").is_empty(),
            enumeration: false,
            resettable: !get("reset").is_empty(),
            notify: !get("notify").is_empty(),
            is_final: get("isFinal") == "true",
            part: get("isReadonly") == "true"
                && get("isPointer") == "true"
                && (get("isConstant") == "true" || get("isPropertyConstant") == "true"),
        }),
        ("Enum", Some(t)) => t.enums.push(EnumInfo {
            name: unquote(&get("name")),
            alias: sub.get("alias").map(|a| unquote(a)),
            values: strings(&get("values")),
            flag: get("isFlag") == "true",
        }),
        // A constructor is no method of the object (`item.QObject` is undefined in Qt).
        ("Method", Some(t)) if get("isConstructor") != "true" => {
            let name = unquote(&get("name"));
            if get("isJavaScriptFunction") != "true" {
                t.method_sigs.push(SignalInfo {
                    name: name.clone(),
                    params,
                });
            }
            t.methods.push(name);
        }
        ("Method", Some(_)) => {}
        ("Signal", Some(t)) => t.signals.push(SignalInfo {
            name: unquote(&get("name")),
            params,
        }),
        _ => {}
    }
}

fn set_field(
    stack: &[String],
    cur: &mut Option<TypeInfo>,
    sub: &mut HashMap<String, String>,
    k: &str,
    v: &str,
) {
    match (stack.last().map(String::as_str), cur.as_mut()) {
        (Some("Component"), Some(t)) => match k {
            "name" => t.cpp = unquote(v),
            "prototype" => t.prototype = Some(unquote(v)),
            "extension" => t.extension = Some(unquote(v)),
            "attachedType" => t.attached = Some(unquote(v)),
            "defaultProperty" => t.default_prop = Some(unquote(v)),
            "isSingleton" => t.singleton = v == "true",
            "isCreatable" => t.creatable = v != "false",
            "exports" => {
                for x in strings(v) {
                    if let Some((m, rest)) = x.split_once('/') {
                        let n = rest.split(' ').next().unwrap_or(rest).to_string();
                        if !t.exports.contains(&(m.to_string(), n.clone())) {
                            t.exports.push((m.to_string(), n));
                        }
                    }
                }
            }
            _ => {}
        },
        _ => {
            sub.insert(k.to_string(), v.to_string());
        }
    }
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

fn strings(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = s.split('"');
    it.next();
    while let Some(v) = it.next() {
        out.push(v.to_string());
        it.next();
    }
    out
}

impl Registry {
    pub fn load() -> Registry {
        let mut r = Registry::default();
        let sources = data::QMLTYPES.iter();
        #[cfg(test)]
        let sources = sources.chain(&[("Impulse/test.qmltypes", crate::impulse::tests::QMLTYPES)]);
        // The shell's own members of a class another module declares.
        let mut own = Vec::new();
        for (path, src) in sources {
            for mut t in parse_qmltypes(src) {
                // Quickshell's and the shell's own constant pointers are objects and models
                // the shell hands out.
                if !path.starts_with("Qt") {
                    t.props.iter_mut().for_each(|p| p.part = false);
                }
                if path.starts_with("Impulse/") && t.exports.is_empty() && t.prototype.is_none() {
                    own.push(t);
                    continue;
                }
                for (module, n) in &t.exports {
                    let e = r.by_name.entry(n.clone()).or_default();
                    if !e.iter().any(|(mm, _)| mm == module) {
                        e.push((module.clone(), t.cpp.clone()));
                    }
                }
                r.by_cpp.insert(t.cpp.clone(), t);
            }
        }
        for t in own {
            match r.by_cpp.get_mut(&t.cpp) {
                Some(of) => {
                    of.props.extend(t.props);
                    of.signals.extend(t.signals);
                    of.methods.extend(t.methods);
                    of.enums.extend(t.enums);
                }
                None => {
                    r.by_cpp.insert(t.cpp.clone(), t);
                }
            }
        }
        // QQmlPropertyCache::append skips QMetaMethod::Private methods.
        for line in include_str!("private_methods.tsv").lines() {
            if let Some((cpp, m)) = line.split_once('\t')
                && let Some(t) = r.by_cpp.get_mut(cpp)
            {
                t.methods.retain(|x| x != m);
                t.method_sigs.retain(|x| x.name != m);
            }
        }
        for line in include_str!("enums.tsv").lines() {
            let mut p = line.split('\t');
            if let (Some(m), Some(t), Some(v), Some(n)) = (p.next(), p.next(), p.next(), p.next())
                && let Ok(n) = n.parse()
            {
                r.enum_values.insert((m.into(), t.into(), v.into()), n);
            }
        }
        let enum_props: Vec<(String, usize)> = r
            .by_cpp
            .values()
            .flat_map(|t| {
                let r = &r;
                t.props
                    .iter()
                    .enumerate()
                    .filter(move |(_, p)| !p.list && r.names_enum(&t.cpp, &p.ty))
                    .map(move |(i, _)| (t.cpp.clone(), i))
            })
            .collect();
        for (cpp, i) in enum_props {
            if let Some(t) = r.by_cpp.get_mut(&cpp) {
                t.props[i].enumeration = true;
            }
        }
        // What the parser marked is a constant read-only pointer; a part is one whose class
        // QML cannot make.
        let no_parts: Vec<(String, usize)> = r
            .by_cpp
            .values()
            .flat_map(|t| {
                let r = &r;
                t.props
                    .iter()
                    .enumerate()
                    .filter(move |(_, p)| {
                        p.part
                            && r.by_cpp
                                .get(p.ty.trim_end_matches('*'))
                                .is_none_or(|c| c.creatable && !c.exports.is_empty())
                    })
                    .map(move |(i, _)| (t.cpp.clone(), i))
            })
            .collect();
        for (cpp, i) in no_parts {
            if let Some(t) = r.by_cpp.get_mut(&cpp) {
                t.props[i].part = false;
            }
        }
        for t in r.by_cpp.values() {
            for p in &t.props {
                *r.prop_names.entry(p.name.clone()).or_default() |= is_model(&p.ty);
            }
            r.signal_names
                .extend(t.signals.iter().map(|s| s.name.clone()));
            r.method_names.extend(t.methods.iter().cloned());
        }
        r
    }

    /// Whether the type name `ty`, written in class `cpp`, is an enum (or flags) type: a
    /// qualified `Scope::Name` is looked up in `Scope`, an unqualified name in `cpp` and then its
    /// bases, as C++ name lookup does in member declarations.
    fn names_enum(&self, cpp: &str, ty: &str) -> bool {
        let (scope, name) = ty.rsplit_once("::").unwrap_or((cpp, ty));
        self.chain(scope).iter().any(|t| {
            t.enums
                .iter()
                .any(|e| e.name == name || e.alias.as_deref() == Some(name))
        })
    }

    /// Prototype chain, most-derived first.
    pub fn chain(&self, cpp: &str) -> Vec<&TypeInfo> {
        let mut out = Vec::new();
        let mut c = self.by_cpp.get(cpp);
        while let Some(t) = c {
            if out.len() > 32 {
                break;
            }
            out.push(t);
            c = t.prototype.as_ref().and_then(|p| self.by_cpp.get(p));
        }
        out
    }

    /// Resolve a QML type name by the name alone: the project's own `Impulse` module first,
    /// then Quickshell's, then QtQuick and QtQml.
    pub fn resolve(&self, name: &str) -> Option<(&str, &TypeInfo)> {
        let Some(cands) = self.by_name.get(name) else {
            // QtQuick.Controls DayOfWeekRow/MonthGrid/WeekNumberColumn are style QML files over
            // the QtQuick.Templates Abstract* controls.
            let abs = format!("Abstract{name}");
            if matches!(name, "DayOfWeekRow" | "MonthGrid" | "WeekNumberColumn")
                && self.by_name.contains_key(&abs)
            {
                return self.resolve(&abs);
            }
            // Kirigami is not shipped: its Icon is an Item painted like an Image (scene.rs).
            return (name == "Kirigami.Icon")
                .then(|| self.resolve("Item"))
                .flatten();
        };
        let rank = |m: &str| {
            if m == "Impulse" {
                0
            } else if m.starts_with("Rustshell") {
                1
            } else if m == "QtQuick" || m == "QtQml" {
                2
            } else if m == "Qt5Compat.GraphicalEffects" {
                // RadialGradient is also a QtQuick.Shapes gradient, which nothing here draws
                // (`typecheck::NATIVE_OF`).
                3
            } else {
                4
            }
        };
        let (m, cpp) = cands.iter().min_by_key(|(m, _)| rank(m))?;
        Some((m.as_str(), self.by_cpp.get(cpp)?))
    }

    /// The attached type of the QML type `name` of `module`, whichever of its C++ classes
    /// (`QQuickWindow`, `QQuickWindowQmlImpl`) declares it.
    pub fn attached(&self, module: &str, name: &str) -> Option<&str> {
        let export = (module.to_string(), name.to_string());
        let mut all = self.by_cpp.values().filter(|t| t.exports.contains(&export));
        all.find_map(|t| t.attached.as_deref())
    }

    /// Whether `import uri` makes the C++ type `name` visible: the module's own exports and
    /// those of the modules its qmldir imports (QtQuick has QtQml's, QtQml the builtins',
    /// QtQuick.Window has QtQuick's).
    pub fn exports(&self, uri: &str, name: &str) -> bool {
        let also: &[&str] = match uri {
            "QtQuick" => &["QtQml", "QML", "QtQml.Models", "QtQml.WorkerScript"],
            "QtQml" => &["QML", "QtQml.Models", "QtQml.WorkerScript"],
            "QtQuick.Window" => &[
                "QtQuick",
                "QtQml",
                "QML",
                "QtQml.Models",
                "QtQml.WorkerScript",
            ],
            // The style's controls are QML files over the templates of the same names.
            "QtQuick.Controls" => &["QtQuick.Templates"],
            _ => &[],
        };
        let mods = self.by_name.get(name);
        (mods.into_iter().flatten()).any(|(m, _)| m == uri || also.contains(&m.as_str()))
    }

    /// `Type.Value` / `Type.Enum.Value` namespace for a QML type name.
    pub fn enum_namespace(&self, name: &str) -> Vec<(Option<String>, String, f64)> {
        let Some((module, t)) = self.resolve(name) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut tys = self.chain(&t.cpp);
        let exts: Vec<_> = tys.iter().filter_map(|ty| ty.extension.clone()).collect();
        for x in exts {
            tys.extend(self.chain(&x));
        }
        for ty in tys {
            for e in &ty.enums {
                for (i, v) in e.values.iter().enumerate() {
                    let n = self
                        .enum_values
                        .get(&(module.to_string(), name.to_string(), v.clone()))
                        .copied()
                        .unwrap_or(i as f64);
                    out.push((Some(e.name.clone()), v.clone(), n));
                }
            }
        }
        out
    }
}

static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();

pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::load)
}

/// What real Quickshell calls its module and its singleton. The tests that run a file in
/// both shells write it in those words, so a build with the interpreter reads them as this
/// engine's own names; the shipped build has no interpreter and knows only its own.
#[cfg(feature = "interp")]
const THEIRS: &str = concat!("Quick", "shell");

/// The uri of an import written for real Quickshell (its module, the directories of the
/// tree after its prefix), as this engine names it.
#[cfg(feature = "interp")]
pub fn own_uri(uri: String) -> String {
    for (theirs, ours) in [(THEIRS, "Rustshell"), (concat!("q", "s"), "ri")] {
        match uri.strip_prefix(theirs) {
            Some(rest) if rest.is_empty() || rest.starts_with('.') => {
                return format!("{ours}{rest}");
            }
            _ => {}
        }
    }
    uri
}

/// The name of a type written for real Quickshell (its singleton), as this engine names it.
#[cfg(feature = "interp")]
pub fn own_type(name: &str) -> &str {
    if name == THEIRS { "Rustshell" } else { name }
}

fn is_model(ty: &str) -> bool {
    ty.contains("ObjectModel")
}

/// Default value of a C++ property type (QVariant → undefined, QObject* → null).
pub fn cpp_default(p: &PropInfo) -> crate::value::Value {
    use crate::value::Value;
    if p.list {
        return Value::array(Vec::new());
    }
    if p.ty.ends_with('*') {
        return Value::Null;
    }
    if p.enumeration {
        return Value::Num(0.0);
    }
    match p.ty.as_str() {
        "int" | "uint" | "double" | "float" | "qreal" | "qlonglong" | "qulonglong" | "short"
        | "ushort" | "quint8" | "qint8" | "quint16" | "quint32" | "qint32" | "qint64"
        | "quint64" | "long" | "ulong" => Value::Num(0.0),
        "bool" => Value::Bool(false),
        "QString" | "QUrl" => Value::str(""),
        "QColor" => Value::Color(crate::color::Color::INVALID),
        t if let Some(vt) = crate::valuetypes::Vt::of_type(t) => {
            crate::valuetypes::default_value(vt)
        }
        "QVariant" | "QJSValue" | "QVariantMap" | "QVariantHash" => Value::Undefined,
        "QStringList" | "QVariantList" | "QList<QUrl>" | "QStringView" => Value::array(Vec::new()),
        t if t.starts_with("QList<") || t.starts_with("QVector<") => Value::array(Vec::new()),
        t if t.starts_with("Q")
            && !t.ends_with('*')
            && matches!(
                t,
                "QPoint"
                    | "QSize"
                    | "QRect"
                    | "QDateTime"
                    | "QDate"
                    | "QTime"
                    | "QMargins"
                    | "QMarginsF"
            ) =>
        {
            Value::Undefined
        }
        t if registry()
            .by_cpp
            .get(t)
            .is_some_and(|ti| !ti.enums.is_empty() && ti.props.is_empty()) =>
        {
            Value::Num(0.0)
        }
        t if t.contains("::") => Value::Num(0.0),
        _ => Value::Null,
    }
}

/// Property metadata for a QML type chain (most-derived name first), by property name then type.
type PropCache =
    rustc_hash::FxHashMap<String, rustc_hash::FxHashMap<String, Option<&'static PropInfo>>>;

thread_local! {
    static PROP_CACHE: std::cell::RefCell<PropCache> = Default::default();
}

thread_local! {
    static TYPE_SETS: std::cell::RefCell<rustc_hash::FxHashMap<Vec<crate::runtime::Sym>, u32>> = Default::default();
    static SET_PROPS: std::cell::RefCell<
        rustc_hash::FxHashMap<(u32, crate::runtime::Sym), Option<&'static PropInfo>>,
    > = Default::default();
}

/// Interned id (from 1) of a most-derived-first type list.
pub fn type_set(types: &[crate::runtime::Sym]) -> u32 {
    TYPE_SETS.with_borrow_mut(|m| {
        if let Some(&id) = m.get(types) {
            return id;
        }
        let id = m.len() as u32 + 1;
        m.insert(types.to_vec(), id);
        id
    })
}

/// `find_prop` for a type list interned as `set`: one pointer-keyed probe.
pub fn find_prop_in(
    set: u32,
    types: &[crate::runtime::Sym],
    name: &str,
) -> Option<&'static PropInfo> {
    find_prop_in_sym(set, types, crate::runtime::Sym::new(name))
}

pub fn find_prop_in_sym(
    set: u32,
    types: &[crate::runtime::Sym],
    sym: crate::runtime::Sym,
) -> Option<&'static PropInfo> {
    type Slot = (u32, usize, Option<&'static PropInfo>);
    thread_local!(static RECENT: std::cell::Cell<[Slot; 1024]> =
        const { std::cell::Cell::new([(0, 0, None); 1024]) });
    let a = sym.as_str().as_ptr() as usize;
    let slot = (a >> 3 ^ a >> 13 ^ (set as usize).wrapping_mul(0x9E37)) & 1023;
    let (ks, ka, kv) = RECENT.with(|c| {
        let c: &std::cell::Cell<[_]> = c;
        c.as_slice_of_cells()[slot].get()
    });
    if ks == set && ka == a {
        return kv;
    }
    let key = (set, sym);
    let found = match SET_PROPS.with_borrow(|m| m.get(&key).copied()) {
        Some(hit) => hit,
        None => {
            let found = find_prop(types, sym.as_str());
            SET_PROPS.with_borrow_mut(|m| m.insert(key, found));
            found
        }
    };
    RECENT.with(|c| {
        let c: &std::cell::Cell<[_]> = c;
        c.as_slice_of_cells()[slot].set((set, a, found));
    });
    found
}

/// QML `instanceof` with a type name for an object of the type list `types` (interned as
/// `set`): a listed type is `qml`, or a C++ prototype chain contains `qml`'s class (a Controls
/// `Label` is a QQuickText, so a `Text`).
pub fn inherits_in(set: u32, types: &[crate::runtime::Sym], qml: crate::runtime::Sym) -> bool {
    thread_local!(static MEMO: std::cell::RefCell<
        rustc_hash::FxHashMap<(u32, crate::runtime::Sym), bool>,
    > = Default::default());
    if let Some(v) = MEMO.with_borrow(|m| m.get(&(set, qml)).copied()) {
        return v;
    }
    let r = registry();
    let base = r.resolve(qml.as_str()).map(|(_, t)| t.cpp.as_str());
    let v = types.iter().any(|t| {
        t.as_str() == qml.as_str()
            || base.is_some_and(|b| {
                r.resolve(t)
                    .is_some_and(|(_, ti)| r.chain(&ti.cpp).iter().any(|c| c.cpp == b))
            })
    });
    MEMO.with_borrow_mut(|m| m.insert((set, qml), v));
    v
}

pub fn find_prop(types: &[crate::runtime::Sym], name: &str) -> Option<&'static PropInfo> {
    PROP_CACHE.with_borrow_mut(|cache| {
        let per = match cache.get_mut(name) {
            Some(per) => per,
            None => {
                if !registry().prop_names.contains_key(name) {
                    return None;
                }
                cache.entry(name.to_string()).or_default()
            }
        };
        types.iter().find_map(|t| {
            if let Some(hit) = per.get(t.as_str()) {
                return *hit;
            }
            let r = registry();
            // A QML type name, or the C++ class of an object `native_object` made.
            let cpp = r.resolve(t).map_or(t.as_str(), |(_, ti)| ti.cpp.as_str());
            let found = r
                .chain(cpp)
                .into_iter()
                .find_map(|c| c.props.iter().find(|p| p.name == name));
            per.insert(t.to_string(), found);
            found
        })
    })
}

/// Property lookup by C++ class name (e.g. the `QQuickAnchors` behind `item.anchors`).
pub fn find_prop_cpp(cpp: &str, name: &str) -> Option<&'static PropInfo> {
    registry()
        .chain(cpp)
        .into_iter()
        .find_map(|c| c.props.iter().find(|p| p.name == name))
}

/// An item whose class declares an `enabled` of its own (QQuickMouseArea, QQuickPinchArea),
/// hiding QQuickItem's: that one is only what was set. The item state under it, which its
/// children inherit, stays enabled.
pub fn own_enabled(types: &[crate::runtime::Sym]) -> bool {
    thread_local!(static ITEMS: Option<&'static PropInfo> = find_prop_cpp("QQuickItem", "enabled"));
    match (find_prop(types, "enabled"), ITEMS.with(|p| *p)) {
        (Some(p), Some(item)) => !std::ptr::eq(p, item) && inherits(types, "QQuickItem"),
        _ => false,
    }
}

/// Property `name` of the group a property of C++ type `ty` opens: `anchors.left` of
/// `QQuickAnchors*`, `font.pixelSize` of the QFont (declared by its extension type).
pub fn find_group_member(ty: &str, name: &str) -> Option<&'static PropInfo> {
    let r = registry();
    let tys = r.chain(ty.trim_end_matches('*'));
    let own = tys
        .iter()
        .find_map(|c| c.props.iter().find(|p| p.name == name));
    own.or_else(|| {
        tys.iter()
            .filter_map(|c| c.extension.as_deref())
            .flat_map(|x| r.chain(x))
            .find_map(|c| c.props.iter().find(|p| p.name == name))
    })
}

/// Some C++ type declares an object-model property called `name` (cheap pre-check).
pub fn maybe_model_prop(name: &str) -> bool {
    registry().prop_names.get(name).copied().unwrap_or(false)
}

pub fn has_signal(types: &[crate::runtime::Sym], name: &str) -> bool {
    let r = registry();
    if !r.signal_names.contains(name) {
        return false;
    }
    types.iter().any(|t| {
        r.resolve(t).is_some_and(|(_, ti)| {
            r.chain(&ti.cpp)
                .iter()
                .any(|c| c.signals.iter().any(|m| m.name == name))
        })
    })
}

/// The C++ signal `name` of an item of `types`: every overload the most-derived class declaring
/// it has (moc clones of default arguments included), in qmltypes order.
pub fn cpp_signal(types: &[crate::runtime::Sym], name: &str) -> Vec<&'static SignalInfo> {
    let r = registry();
    if !r.signal_names.contains(name) {
        return Vec::new();
    }
    types
        .iter()
        .filter_map(|t| r.resolve(t))
        .flat_map(|(_, ti)| r.chain(&ti.cpp))
        .map(|c| {
            c.signals
                .iter()
                .filter(|s| s.name == name)
                .collect::<Vec<_>>()
        })
        .find(|v| !v.is_empty())
        .unwrap_or_default()
}

/// The overloads of C++ method `name` on the first class of `types`' chain that declares it;
/// empty for a JavaScript function, which takes any arguments.
pub fn cpp_method(types: &[crate::runtime::Sym], name: &str) -> Vec<&'static SignalInfo> {
    let r = registry();
    if !r.method_names.contains(name) {
        return Vec::new();
    }
    types
        .iter()
        .filter_map(|t| r.resolve(t))
        .flat_map(|(_, ti)| r.chain(&ti.cpp))
        .find(|c| c.methods.iter().any(|m| m == name))
        .map(|c| c.method_sigs.iter().filter(|s| s.name == name).collect())
        .unwrap_or_default()
}

/// A QQuickPopup (Popup, Menu, Dialog, Drawer, ToolTip...).
pub fn is_popup(types: &[crate::runtime::Sym]) -> bool {
    inherits(types, "QQuickPopup")
}

/// Whether one of `types` is the C++ class `cpp` or derives from it.
pub fn inherits(types: &[crate::runtime::Sym], cpp: &str) -> bool {
    let r = registry();
    types.iter().any(|t| {
        r.resolve(t)
            .is_some_and(|(_, ti)| r.chain(&ti.cpp).iter().any(|c| c.cpp == cpp))
    })
}

/// The names of the C++ properties of `types` and of its signals and methods, each list in
/// the order QQmlPropertyCache holds them (base class first; a class's signals before its
/// methods) and without repeats: what V4 enumerates on the object's wrapper. QObject's own
/// methods (`toString`, `destroy`) are not enumerable.
pub fn members(types: &[crate::runtime::Sym]) -> (Vec<&'static str>, Vec<&'static str>) {
    let r = registry();
    let cpp = types
        .iter()
        .find_map(|t| r.resolve(t))
        .map_or("QObject", |(_, ti)| ti.cpp.as_str());
    let (mut props, mut methods): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    let add = |to: &mut Vec<&'static str>, n: &'static str| {
        if !to.contains(&n) {
            to.push(n);
        }
    };
    for c in r.chain(cpp).into_iter().rev() {
        for p in &c.props {
            add(&mut props, &p.name);
        }
        for s in &c.signals {
            add(&mut methods, &s.name);
        }
        if c.cpp != "QObject" {
            for m in &c.methods {
                add(&mut methods, m);
            }
        }
    }
    (props, methods)
}

/// The class that declares the method `name` of `types`, by its QML name where it has one
/// (`Item` for `childAt` of a Rectangle).
pub fn declarer(types: &[crate::runtime::Sym], name: &str) -> Option<&'static str> {
    let r = registry();
    if !r.method_names.contains(name) {
        return None;
    }
    types.iter().find_map(|t| {
        let (_, ti) = r.resolve(t)?;
        let c = r
            .chain(&ti.cpp)
            .into_iter()
            .find(|c| c.methods.iter().any(|m| m == name))?;
        Some(
            c.exports
                .first()
                .map_or(c.cpp.as_str(), |(_, n)| n.as_str()),
        )
    })
}

pub fn has_method(types: &[crate::runtime::Sym], name: &str) -> bool {
    let r = registry();
    if !r.method_names.contains(name) {
        return false;
    }
    types.iter().any(|t| {
        r.resolve(t).is_some_and(|(_, ti)| {
            r.chain(&ti.cpp)
                .iter()
                .any(|c| c.methods.iter().any(|m| m == name))
        })
    })
}

/// `QStandardPaths::standardLocations`, matching Qt 6.12 under Quickshell (application name
/// "rustshell", empty organization) as dumped on the reference system.
pub fn standard_location(loc: i64) -> Vec<String> {
    standard_location_in(loc, &crate::xdg::var)
}

pub fn standard_location_in(loc: i64, env: &crate::xdg::Env) -> Vec<String> {
    use crate::xdg::{self, s};
    let home = s(xdg::home_in(env));
    let user_dir = |key: &str, def: &str| {
        let f = format!("{}/user-dirs.dirs", s(xdg::config_home_in(env)));
        std::fs::read_to_string(f)
            .ok()
            .and_then(|s| {
                s.lines().find_map(|l| {
                    let v = l.strip_prefix(&format!("XDG_{key}_DIR="))?;
                    Some(v.trim_matches('"').replace("$HOME", &home))
                })
            })
            .unwrap_or_else(|| format!("{home}/{def}"))
    };
    let data = s(xdg::data_home_in(env));
    let config = s(xdg::config_home_in(env));
    let sys_data = ["/usr/local/share", "/usr/share"];
    let app = "rustshell";
    match loc {
        0 => vec![user_dir("DESKTOP", "Desktop")],
        1 => vec![user_dir("DOCUMENTS", "Documents")],
        2 => vec![
            format!("{data}/fonts"),
            format!("{home}/.fonts"),
            "/usr/local/share/fonts".into(),
            "/usr/share/fonts".into(),
        ],
        3 => std::iter::once(format!("{data}/applications"))
            .chain(sys_data.iter().map(|d| format!("{d}/applications")))
            .collect(),
        4 => vec![user_dir("MUSIC", "Music")],
        5 => vec![user_dir("VIDEOS", "Videos")],
        6 => vec![user_dir("PICTURES", "Pictures")],
        7 => vec!["/tmp".into()],
        8 => vec![home],
        9 | 17 => std::iter::once(format!("{data}/{app}"))
            .chain(sys_data.iter().map(|d| format!("{d}/{app}")))
            .collect(),
        10 => vec![format!("{}/{app}", s(xdg::cache_home_in(env)))],
        11 => std::iter::once(data)
            .chain(sys_data.iter().map(|d| d.to_string()))
            .collect(),
        12 => vec![env("XDG_RUNTIME_DIR").unwrap_or_default()],
        13 | 16 => vec![config, "/etc/xdg".into()],
        14 => vec![user_dir("DOWNLOAD", "Downloads")],
        15 => vec![s(xdg::cache_home_in(env))],
        18 => vec![format!("{config}/{app}"), format!("/etc/xdg/{app}")],
        19 => vec![user_dir("PUBLICSHARE", "Public")],
        20 => vec![user_dir("TEMPLATES", "Templates")],
        21 => vec![format!("{}/{app}", s(xdg::state_home_in(env)))],
        22 => vec![s(xdg::state_home_in(env))],
        _ => Vec::new(),
    }
}

pub fn namespace_object(ns: &[(Option<String>, String, f64)]) -> crate::value::Value {
    use crate::eval::{new_obj, set_own};
    use crate::value::Value;
    let o = new_obj();
    let mut subs: HashMap<String, crate::value::Obj> = HashMap::default();
    for (e, v, n) in ns {
        set_own(&o, v, Value::Num(*n));
        if let Some(e) = e {
            let sub = subs.entry(e.clone()).or_insert_with(new_obj);
            set_own(sub, v, Value::Num(*n));
        }
    }
    for (e, sub) in subs {
        let entries = ns
            .iter()
            .filter(|(x, ..)| x.as_deref() == Some(e.as_str()))
            .map(|(_, v, n)| (std::rc::Rc::<str>::from(v.as_str()), *n))
            .collect();
        sub.borrow_mut().native = Some(std::rc::Rc::new(QmlEnum(entries)));
        set_own(&o, &e, Value::Obj(sub));
    }
    Value::Obj(o)
}

/// The native payload of a QML enum object (`Text.HAlignment`): its keys and values in
/// declaration order, which `Qt.enumValueToString` and kin read.
#[derive(Debug)]
pub struct QmlEnum(pub Vec<(std::rc::Rc<str>, f64)>);

/// The native payload of a QML type's JS object (QQmlTypeWrapper of a type).
#[derive(Debug)]
pub struct QmlType {
    /// What it converts to: `QtQuick/Item`, or the bare name of a type that is a QML file.
    pub name: std::rc::Rc<str>,
    /// The C++ attached type (`QQuickKeysAttached`) of a type that has one.
    pub attached: Option<std::rc::Rc<str>>,
}

/// The native payload of an import qualifier's JS object (QQmlTypeWrapper of a namespace).
#[derive(Debug)]
pub struct QmlNamespace {
    /// The object the qualifier was read on: `Q.ListView` is its attached object.
    pub scope: usize,
    /// Read as a member of that object (`item.Q`), not as a name of its scope (`Q`).
    pub member: bool,
}

/// The native payload of a ListModel row's JS object (QV4's ModelObject).
#[derive(Debug)]
pub struct ListRow;

/// What `o.k = v` does on the JS object of a C++ one where V4 stores nothing.
pub enum HostWrite {
    /// Nothing, whatever the strictness.
    Silent,
    /// Nothing, and a TypeError in strict mode code.
    Refused,
    /// A TypeError naming the read-only member, whatever the strictness.
    ReadOnly(String),
}

/// What a write from script to `name` meets on an object of the C++ class `cpp`, which has
/// the members of its class only (QV4 QObjectWrapper::setQmlProperty): `None` for a writable
/// property. A handler is refused under its signal's name.
pub fn cpp_write(cpp: &str, name: &str) -> Option<HostWrite> {
    let chain = registry().chain(cpp);
    let prop = |n: &str| {
        chain
            .iter()
            .find_map(|c| c.props.iter().find(|x| x.name == n))
    };
    // `toString` and `destroy` are no members of the class: QV4 answers reads of them itself
    // and looks neither up for a write.
    let func = |n: &str| {
        !matches!(n, "toString" | "destroy")
            && (chain.iter())
                .any(|c| c.signals.iter().any(|s| s.name == n) || c.methods.iter().any(|m| m == n))
    };
    if let Some(pi) = prop(name) {
        return (pi.readonly && !pi.list).then(|| HostWrite::ReadOnly(name.to_string()));
    }
    if func(name)
        || name
            .strip_suffix("Changed")
            .is_some_and(|n| prop(n).is_some())
    {
        return Some(HostWrite::ReadOnly(name.to_string()));
    }
    let handled = || {
        let mut rest = name.strip_prefix("on")?.chars();
        let c = rest.next().filter(|c| c.is_uppercase())?;
        Some(c.to_lowercase().chain(rest).collect::<String>())
    };
    Some(match handled().filter(|sig| func(sig)) {
        Some(sig) => HostWrite::ReadOnly(sig),
        None => HostWrite::Refused,
    })
}

/// The write of `k` to `o` when `o` is the JS object of a QML type or enum (QQmlTypeWrapper,
/// which has nothing to write), the `Qt` object (a QObject whose methods and constant
/// properties are read-only) or a ListModel row (which takes its roles only).
pub fn host_write(o: &crate::value::JsObject, k: &str) -> Option<HostWrite> {
    let n = o.native.as_ref()?;
    if let Some(t) = n.downcast_ref::<QmlType>() {
        // The type's attached object takes the write (a new one for each object in Qt; here
        // the type's own object keeps the value).
        return match &t.attached {
            Some(cpp) => cpp_write(cpp, k),
            None => Some(HostWrite::Refused),
        };
    }
    if n.is::<QmlEnum>() {
        return Some(HostWrite::Refused);
    }
    if n.is::<crate::qtobj::QtGlobal>() {
        if crate::eval::own_accessor(o, k).is_some() {
            return None;
        }
        let method = matches!(o.props.get(k), Some(crate::value::Value::Obj(f)) if f.borrow().call.is_some());
        return Some(match method || k == "platform" {
            true => HostWrite::ReadOnly(k.to_string()),
            false => HostWrite::Refused,
        });
    }
    if n.is::<ListRow>() {
        let own = o.props.contains_key(k) || crate::eval::array_index(k).is_some();
        return (!own).then_some(HostWrite::Silent);
    }
    None
}

/// Whether `o` is a row of a ListModel without `dynamicRoles`.
pub fn is_list_row(o: &crate::value::JsObject) -> bool {
    o.native.as_ref().is_some_and(|n| n.is::<ListRow>())
}

/// Whether `delete o[k]` leaves `o` as it is: no member of these objects can be deleted, and
/// V4 answers `true` all the same.
pub fn host_keeps(o: &crate::value::JsObject) -> bool {
    (o.native.as_ref()).is_some_and(|n| {
        n.is::<QmlType>()
            || n.is::<QmlEnum>()
            || n.is::<crate::qtobj::QtGlobal>()
            || n.is::<ListRow>()
    })
}

/// Whether `o` is the JS object of a QML type or of an import qualifier, or `Qt`.
pub fn is_type_wrapper(o: &crate::value::JsObject) -> bool {
    (o.native.as_ref())
        .is_some_and(|n| n.is::<QmlType>() || n.is::<QmlNamespace>() || n.is::<QtGlobal>())
}

/// Whether `o` is one of those whose names come from QQmlTypeWrapper's prototype, the functions'
/// ([`wrapper_proto`]): an import qualifier and a type with no object behind it. The names of
/// a type with an attached type and of `Qt` are those of that object, a QObject.
pub fn inherits_function(o: &crate::value::JsObject) -> bool {
    (o.native.as_ref()).is_some_and(|n| match n.downcast_ref::<QmlType>() {
        Some(t) => t.attached.is_none(),
        None => n.is::<QmlNamespace>(),
    })
}

/// Whether `o` is the `Qt` object.
pub fn is_qt_global(o: &crate::value::JsObject) -> bool {
    o.native.as_ref().is_some_and(|n| n.is::<QtGlobal>())
}

/// `toString` of an object that converts as a QML type does without inheriting it.
pub fn own_to_string() -> crate::value::Value {
    crate::eval::fn_value(crate::eval::Callable::Native("toString", type_to_string))
}

/// The prototype of every QML type object, import qualifier object and `Qt`
/// (QQmlTypeWrapper's): `toString`, on top of %Function.prototype%.
pub fn wrapper_proto(function_proto: &crate::value::Obj) -> crate::value::Obj {
    let p = crate::eval::new_obj();
    p.borrow_mut().proto = Some(function_proto.clone());
    let f = crate::eval::Callable::Native("toString", type_to_string);
    crate::eval::set_hidden(&p, "toString", crate::eval::fn_value(f));
    p
}

/// The QML type `o` is the JS object of.
pub fn qml_type(o: &crate::value::Obj) -> Option<std::rc::Rc<QmlType>> {
    let n = o.borrow().native.clone()?;
    n.downcast::<QmlType>().ok()
}

/// QQmlTypeWrapper::virtualIsEqualTo: two type objects are equal when what they stand for is,
/// which is nothing for a type with no attached type and the attached object for one with.
pub fn same_type(a: &crate::value::Obj, b: &crate::value::Obj) -> bool {
    match (qml_type(a), qml_type(b)) {
        (Some(a), Some(b)) => a.attached == b.attached,
        _ => false,
    }
}

/// `toString` of every QML type and import qualifier object (QQmlTypeWrapper's
/// method_toString): the type's name, `Unknown Type` for a qualifier, `undefined` for any
/// other `this`.
pub fn type_to_string(
    _: &mut crate::eval::Interp,
    this: &crate::value::Value,
    _: &[crate::value::Value],
) -> crate::eval::R<crate::value::Value> {
    use crate::value::Value;
    let Value::Obj(o) = this else {
        return Ok(Value::Undefined);
    };
    if let Some(t) = qml_type(o) {
        return Ok(Value::str(&match &t.attached {
            // The attached object, as any QObject converts.
            Some(a) => format!("{a}({:p})", std::rc::Rc::as_ptr(&t)),
            None => t.name.to_string(),
        }));
    }
    let b = o.borrow();
    let Some(n) = b.native.as_ref() else {
        return Ok(Value::Undefined);
    };
    Ok(if n.is::<QmlNamespace>() {
        Value::str("Unknown Type")
    } else if n.is::<QtGlobal>() {
        // The singleton, as any QObject converts.
        Value::str(&format!("Qt({:p})", std::rc::Rc::as_ptr(n).cast::<()>()))
    } else {
        Value::Undefined
    })
}

pub fn standard_paths_object(name: &str) -> crate::value::Value {
    use crate::eval::{Callable, fn_value};
    use crate::value::Value;
    let v = namespace_object(&registry().enum_namespace(name));
    let Value::Obj(o) = &v else { return v };
    let locs: crate::eval::NativeFn = |_, _, a| {
        let loc = a.first().map(|v| v.to_number()).unwrap_or(0.0) as i64;
        Ok(Value::array(
            standard_location(loc)
                .into_iter()
                .map(|p| Value::str(&crate::native::path_to_url(p.as_ref())))
                .collect(),
        ))
    };
    let writable: crate::eval::NativeFn = |_, _, a| {
        let loc = a.first().map(|v| v.to_number()).unwrap_or(0.0) as i64;
        let p = standard_location(loc)
            .into_iter()
            .next()
            .unwrap_or_default();
        Ok(Value::str(&crate::native::path_to_url(p.as_ref())))
    };
    crate::eval::set_own(
        o,
        "standardLocations",
        fn_value(Callable::Native("standardLocations", locs)),
    );
    crate::eval::set_own(
        o,
        "writableLocation",
        fn_value(Callable::Native("writableLocation", writable)),
    );
    crate::unwritten::fill(o, name);
    v
}

fn effect(it: &mut crate::eval::Interp, e: crate::svc::Effect) {
    if let Some(w) = crate::runtime::world_of(it) {
        w.borrow_mut().effects.push(e);
    }
}

/// wlr-foreign-toplevel `activate`/`close` on a Hyprland client: Hyprland focuses (switching
/// workspace) or closes the window, which is what the address dispatchers do.
fn toplevel_dispatch(it: &mut crate::eval::Interp, this: &crate::value::Value, dsp: &str) {
    use crate::value::Value;
    let (Value::Item(t), Some(w)) = (this, crate::runtime::world_of(it)) else {
        return;
    };
    let mut w = w.borrow_mut();
    // CWindow::activate returns early for the focused window (no refocus, no cursor warp).
    if dsp == "focus" && w.read(*t, "activated").is_some_and(|v| v.truthy()) {
        return;
    }
    let Some(Value::Item(ht)) = w.read(*t, "HyprlandToplevel") else {
        return;
    };
    let Some(Value::Str(a)) = w.read(ht, "address") else {
        return;
    };
    // HyprlandToplevel.address has no 0x prefix; Hyprland window selectors need it.
    let d = format!("hl.dsp.{dsp}({{window=\"address:0x{a}\"}})");
    w.effects
        .push(crate::svc::Effect::HyprDispatch(vec![Value::str(&d)]));
}

pub(crate) fn sni_effect(
    it: &mut crate::eval::Interp,
    this: &crate::value::Value,
    m: &str,
    a: &[crate::value::Value],
) {
    let mut v = vec![this.clone()];
    v.extend_from_slice(a);
    effect(it, crate::svc::Effect::Sni(crate::svc::Call::new(m, v)));
}

fn bluez_effect(
    it: &mut crate::eval::Interp,
    this: &crate::value::Value,
    m: &str,
    a: &[crate::value::Value],
) {
    let mut v = vec![this.clone()];
    v.extend_from_slice(a);
    effect(it, crate::svc::Effect::Bluez(crate::svc::Call::new(m, v)));
}

fn mpris_effect(
    it: &mut crate::eval::Interp,
    this: &crate::value::Value,
    m: &str,
    a: &[crate::value::Value],
) {
    let mut v = vec![this.clone()];
    v.extend_from_slice(a);
    effect(it, crate::svc::Effect::Mpris(crate::svc::Call::new(m, v)));
}

/// Instantiate a C++ singleton (`Rustshell`, `Hyprland`, `Pipewire`, ...) as a reactive item
/// whose properties the Rust service layer updates; methods are natives.
pub fn native_singleton(w: &mut crate::runtime::World, name: &str) -> usize {
    let r = registry();
    let id = w.new_inst(name, None);
    let Some((_, t)) = r.resolve(name) else {
        return id;
    };
    fill_native(w, id, name, &t.cpp);
    if let Some(init) = crate::impulse::find(name).and_then(|t| t.init) {
        init(w, id);
    }
    id
}

/// A QInputMethod call, queued for the host; a `void` slot, it returns `undefined`, and
/// arguments beyond its parameters are ignored (with a warning).
fn im_request(
    it: &mut crate::eval::Interp,
    r: crate::inputmethod::Request,
) -> crate::eval::R<crate::value::Value> {
    if let Some(w) = crate::runtime::world_of(it) {
        w.borrow_mut().input_method.requests.push(r);
    }
    Ok(crate::value::Value::Undefined)
}

/// An instance of the C++ QObject type `cpp` (`QStyleHints`) that QML reaches only through
/// another object's property, with the properties, signals and methods of its metatype.
pub fn native_object(w: &mut crate::runtime::World, cpp: &str) -> usize {
    let id = w.new_inst(cpp, None);
    fill_native(w, id, cpp, cpp);
    id
}

/// Pointer properties Quickshell guarantees non-null (created with their owner).
const NON_NULL: &[(&str, &str)] = &[("UPower", "displayDevice")];

fn fill_native(w: &mut crate::runtime::World, id: usize, name: &str, cpp: &str) {
    use crate::eval::{Callable, NativeFn, fn_value};
    use crate::value::Value;
    let r = registry();
    for c in r.chain(cpp) {
        for p in &c.props {
            if w.items[id].props.contains_key(&p.name) {
                continue;
            }
            let v = if NON_NULL.contains(&(name, p.name.as_str())) {
                let pc = p.ty.trim_end_matches('*');
                let qn = r
                    .by_cpp
                    .get(pc)
                    .and_then(|t| t.exports.first())
                    .map(|e| e.1.clone())
                    .unwrap_or_else(|| pc.to_string());
                let sub = w.new_inst(&qn, None);
                // QObject children of the object, destroyed with it.
                w.owned.entry(id).or_default().push(sub);
                fill_native(w, sub, &qn, pc);
                Value::Item(sub)
            } else if is_model(&p.ty) {
                let m = w.new_inst("ObjectModel", None);
                w.owned.entry(id).or_default().push(m);
                w.declare(m, "values", "list");
                Value::Item(m)
            } else {
                cpp_default(p)
            };
            w.declare(id, &p.name, "var");
            w.items[id].props.get_mut(&p.name).unwrap().value = v;
            w.stamp(id);
        }
        for m in &c.methods {
            if w.items[id].props.contains_key(m) {
                continue;
            }
            let f: NativeFn = match (name, m.as_str()) {
                _ if let Some(f) = crate::impulse::method(name, m) => f,
                (_, "execDetached") => |it, _, a| {
                    effect(it, crate::svc::Effect::ExecDetached(a.to_vec()));
                    Ok(Value::Undefined)
                },
                ("Repository", "definitionForName") => |_, _, a| {
                    Ok(crate::syntax::definition_value(
                        a.first().unwrap_or(&Value::Undefined),
                    ))
                },
                ("Hyprland", "monitorFor") => |it, _, a| {
                    let Some(w) = crate::runtime::world_of(it) else {
                        return Ok(Value::Null);
                    };
                    let mut w = w.borrow_mut();
                    Ok(crate::hypr::monitor_for(&mut w, a.first()))
                },
                ("Notification", "expire") => |it, this, a| {
                    crate::notifications::method(it, this, "expire", a);
                    Ok(Value::Undefined)
                },
                ("Notification", "dismiss") => |it, this, a| {
                    crate::notifications::method(it, this, "dismiss", a);
                    Ok(Value::Undefined)
                },
                ("Notification", "sendInlineReply") => |it, this, a| {
                    crate::notifications::method(it, this, "sendInlineReply", a);
                    Ok(Value::Undefined)
                },
                ("NotificationAction", "invoke") => |it, this, a| {
                    crate::notifications::method(it, this, "invoke", a);
                    Ok(Value::Undefined)
                },
                ("SystemTrayItem", "activate") => |it, this, a| {
                    sni_effect(it, this, "Activate", a);
                    Ok(Value::Undefined)
                },
                ("SystemTrayItem", "secondaryActivate") => |it, this, a| {
                    sni_effect(it, this, "SecondaryActivate", a);
                    Ok(Value::Undefined)
                },
                ("SystemTrayItem", "scroll") => |it, this, a| {
                    sni_effect(it, this, "Scroll", a);
                    Ok(Value::Undefined)
                },
                ("MprisPlayer", "togglePlaying") => |it, this, a| {
                    mpris_effect(it, this, "PlayPause", a);
                    Ok(Value::Undefined)
                },
                ("MprisPlayer", "play") => |it, this, a| {
                    mpris_effect(it, this, "Play", a);
                    Ok(Value::Undefined)
                },
                ("MprisPlayer", "pause") => |it, this, a| {
                    mpris_effect(it, this, "Pause", a);
                    Ok(Value::Undefined)
                },
                ("MprisPlayer", "stop") => |it, this, a| {
                    mpris_effect(it, this, "Stop", a);
                    Ok(Value::Undefined)
                },
                ("MprisPlayer", "next") => |it, this, a| {
                    mpris_effect(it, this, "Next", a);
                    Ok(Value::Undefined)
                },
                ("MprisPlayer", "previous") => |it, this, a| {
                    mpris_effect(it, this, "Previous", a);
                    Ok(Value::Undefined)
                },
                ("MprisPlayer", "seek") => |it, this, a| {
                    mpris_effect(it, this, "Seek", a);
                    Ok(Value::Undefined)
                },
                ("MprisPlayer", "raise") => |it, this, a| {
                    mpris_effect(it, this, "Raise", a);
                    Ok(Value::Undefined)
                },
                ("MprisPlayer", "quit") => |it, this, a| {
                    mpris_effect(it, this, "Quit", a);
                    Ok(Value::Undefined)
                },
                ("BluetoothDevice", "connect") => |it, this, a| {
                    bluez_effect(it, this, "connect", a);
                    Ok(Value::Undefined)
                },
                ("BluetoothDevice", "disconnect") => |it, this, a| {
                    bluez_effect(it, this, "disconnect", a);
                    Ok(Value::Undefined)
                },
                ("BluetoothDevice", "pair") => |it, this, a| {
                    bluez_effect(it, this, "pair", a);
                    Ok(Value::Undefined)
                },
                ("BluetoothDevice", "cancelPair") => |it, this, a| {
                    bluez_effect(it, this, "cancelPair", a);
                    Ok(Value::Undefined)
                },
                ("BluetoothDevice", "forget") => |it, this, a| {
                    bluez_effect(it, this, "forget", a);
                    Ok(Value::Undefined)
                },
                (
                    "UPowerDeviceState"
                    | "UPowerDeviceType"
                    | "PowerProfile"
                    | "PerformanceDegradationReason"
                    | "BluetoothAdapterState"
                    | "BluetoothDeviceState",
                    "toString",
                ) => |it, this, a| {
                    let ty = match (crate::runtime::world_of(it), this) {
                        (Some(w), Value::Item(i)) => w.borrow().items[*i]
                            .types
                            .first()
                            .map(|t| t.to_string())
                            .unwrap_or_default(),
                        _ => String::new(),
                    };
                    let n = a.first().map_or(f64::NAN, |v| v.to_number());
                    Ok(crate::upower::enum_string(&ty, n)
                        .or_else(|| crate::bluez::enum_string(&ty, n))
                        .map_or(Value::Undefined, |s| Value::str(&s)))
                },
                ("QInputMethod", "show") => {
                    |it, _, _| im_request(it, crate::inputmethod::Request::Show)
                }
                ("QInputMethod", "hide") => {
                    |it, _, _| im_request(it, crate::inputmethod::Request::Hide)
                }
                ("QInputMethod", "reset") => {
                    |it, _, _| im_request(it, crate::inputmethod::Request::Reset)
                }
                ("QInputMethod", "commit") => {
                    |it, _, _| im_request(it, crate::inputmethod::Request::Commit)
                }
                ("QInputMethod", "update") => |it, _, a| {
                    let [q] = a else {
                        return it.throw(
                            "Error",
                            if a.is_empty() {
                                "Insufficient arguments"
                            } else {
                                "Too many arguments"
                            },
                        );
                    };
                    let q = crate::eval::to_i32(q.to_number()) as u32;
                    if let Some(w) = crate::runtime::world_of(it) {
                        crate::inputmethod::update(&mut w.borrow_mut(), q);
                    }
                    Ok(Value::Undefined)
                },
                ("QInputMethod", "invokeAction") => |it, _, a| {
                    let [act, pos] = a else {
                        return it.throw(
                            "Error",
                            if a.len() < 2 {
                                "Insufficient arguments"
                            } else {
                                "Too many arguments"
                            },
                        );
                    };
                    let (act, pos) = (
                        crate::eval::to_i32(act.to_number()),
                        crate::eval::to_i32(pos.to_number()),
                    );
                    im_request(it, crate::inputmethod::Request::InvokeAction(act, pos))
                },
                ("PamResult", "toString") => |_, _, a| {
                    let v = a.first().map_or(-1.0, |v| v.to_number());
                    Ok(Value::str(crate::pam::result_string(v)))
                },
                ("PamError", "toString") => |_, _, a| {
                    let v = a.first().map_or(-1.0, |v| v.to_number());
                    Ok(Value::str(crate::pam::error_string(v)))
                },
                ("AuthFlow", "submit") => |it, this, a| {
                    if let (Some(w), Value::Item(i)) = (crate::runtime::world_of(it), this) {
                        crate::polkit::flow_call(&mut w.borrow_mut(), *i, "submit", a);
                    }
                    Ok(Value::Undefined)
                },
                ("AuthFlow", "cancelAuthenticationRequest") => |it, this, a| {
                    if let (Some(w), Value::Item(i)) = (crate::runtime::world_of(it), this) {
                        crate::polkit::flow_call(&mut w.borrow_mut(), *i, "cancel", a);
                    }
                    Ok(Value::Undefined)
                },
                ("DesktopEntries", "byId") => |it, _, a| {
                    let n = a.first().map(|v| v.to_js_string()).unwrap_or_default();
                    let Some(w) = crate::runtime::world_of(it) else {
                        return Ok(Value::Null);
                    };
                    let mut w = w.borrow_mut();
                    Ok(crate::desktop::by_id(&mut w, &n))
                },
                ("DesktopEntries", "heuristicLookup") => |it, _, a| {
                    let n = a.first().map(|v| v.to_js_string()).unwrap_or_default();
                    let Some(w) = crate::runtime::world_of(it) else {
                        return Ok(Value::Null);
                    };
                    let mut w = w.borrow_mut();
                    Ok(crate::desktop::heuristic_lookup(&mut w, &n))
                },
                ("DesktopEntry" | "DesktopAction", "execute") => |it, this, _| {
                    if let (Some(w), Value::Item(i)) = (crate::runtime::world_of(it), this) {
                        crate::desktop::execute(&mut w.borrow_mut(), *i);
                    }
                    Ok(Value::Undefined)
                },
                // HyprlandWorkspace::activate, as Quickshell dispatches it to a Hyprland that
                // takes Lua (the form `toplevel_dispatch` sends too).
                ("HyprlandWorkspace", "activate") => |it, this, _| {
                    let (Value::Item(ws), Some(w)) = (this, crate::runtime::world_of(it)) else {
                        return Ok(Value::Undefined);
                    };
                    let mut w = w.borrow_mut();
                    let name = w.read(*ws, "name").map(|v| v.to_js_string());
                    let name = name.unwrap_or_default().replace('\\', "\\\\");
                    let d = format!(
                        "hl.dsp.focus({{ workspace = \"{}\" }})",
                        name.replace('"', "\\\"")
                    );
                    w.effects
                        .push(crate::svc::Effect::HyprDispatch(vec![Value::str(&d)]));
                    Ok(Value::Undefined)
                },
                ("Toplevel", "activate") => |it, this, _| {
                    toplevel_dispatch(it, this, "focus");
                    Ok(Value::Undefined)
                },
                ("Toplevel", "close") => |it, this, _| {
                    toplevel_dispatch(it, this, "window.close");
                    Ok(Value::Undefined)
                },
                (_, "dispatch") => |it, _, a| {
                    effect(it, crate::svc::Effect::HyprDispatch(a.to_vec()));
                    Ok(Value::Undefined)
                },
                (_, "reload") => |it, _, a| {
                    effect(it, crate::svc::Effect::Reload(a.to_vec()));
                    Ok(Value::Undefined)
                },
                (_, "env") => |_, _, a| {
                    let k = a.first().map(|v| v.to_js_string()).unwrap_or_default();
                    // Quickshell: `null` for a variable that is not set
                    // (`RustshellGlobal::env`, src/core/qmlglobal.cpp).
                    Ok(std::env::var_os(k)
                        .map(|v| Value::str(&v.to_string_lossy()))
                        .unwrap_or(Value::Null))
                },
                (_, "shellPath") => |it, _, a| {
                    let rel = a.first().map(|v| v.to_js_string()).unwrap_or_default();
                    let root = crate::runtime::world_of(it)
                        .and_then(|w| w.borrow().root_dir.clone())
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    Ok(Value::str(&format!("{root}/{rel}")))
                },
                (_, "iconPath") => |_, _, a| {
                    let n = a.first().map(|v| v.to_js_string()).unwrap_or_default();
                    Ok(Value::str(&match a.get(1) {
                        Some(Value::Bool(true)) if crate::icons::lookup(&n, 64).is_none() => {
                            String::new()
                        }
                        Some(Value::Str(f)) if !f.is_empty() => {
                            format!("image://icon/{n}?fallback={f}")
                        }
                        _ => format!("image://icon/{n}"),
                    }))
                },
                // Not written: the object has no function of its own for it, and the call
                // throws (`crate::unwritten::error`).
                _ => continue,
            };
            w.declare(id, m, "function");
            w.items[id].props.get_mut(m).unwrap().value = fn_value(Callable::Native("native", f));
        }
    }
}

/// Prefix of virtual paths for QML files shipped inside modules (Quickshell's `FileView.qml`).
pub const MODULE_PREFIX: &str = "qrc:/";

pub fn module_file(p: &std::path::Path) -> Option<&'static str> {
    let rel = p.to_str()?.strip_prefix(MODULE_PREFIX)?;
    data::MODULE_FILES
        .iter()
        .find(|(r, _)| *r == rel)
        .map(|(_, s)| *s)
}

/// QML-implemented types a module's `qmldir` exports (`FileView 0.0 FileView.qml`).
pub fn module_types(uri: &str) -> HashMap<String, crate::resolve::TypeRef> {
    (*module_types_shared(uri)).clone()
}

/// [`module_types`], the one table of the module for every document importing it.
pub fn module_types_shared(uri: &str) -> crate::resolve::Types {
    thread_local! {
        static MODULES: std::cell::RefCell<HashMap<String, crate::resolve::Types>> =
            Default::default();
    }
    if let Some(t) = MODULES.with_borrow(|m| m.get(uri).cloned()) {
        return t;
    }
    let t = std::rc::Rc::new(read_module_types(uri));
    MODULES.with_borrow_mut(|m| m.insert(uri.to_string(), t.clone()));
    t
}

fn read_module_types(uri: &str) -> HashMap<String, crate::resolve::TypeRef> {
    let dir = uri.replace('.', "/");
    let qmldir = format!("{dir}/qmldir");
    let mut out = HashMap::default();
    let Some((_, src)) = data::MODULE_FILES.iter().find(|(r, _)| *r == qmldir) else {
        return out;
    };
    for line in src.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        let (singleton, w) = match w.first() {
            Some(&"singleton") => (true, &w[1..]),
            _ => (false, &w[..]),
        };
        if let [name, _ver, file] = w
            && file.ends_with(".qml")
            && name.starts_with(char::is_uppercase)
        {
            out.insert(
                name.to_string(),
                crate::resolve::TypeRef {
                    file: std::path::PathBuf::from(format!("{MODULE_PREFIX}{dir}/{file}")).into(),
                    singleton,
                    registered: None,
                    no_pragma: false,
                },
            );
        }
    }
    out
}

/// Default property of a type when it takes a Component (`Variants`, `Repeater`), so a bare
/// child object becomes the delegate.
/// Whether a type is a known non-`QQuickItem` (Behavior, Timer, …): such objects live in
/// `data`/`resources` but never in `Item.children`. Unresolvable types count as items.
pub fn is_non_item(types: &[crate::runtime::Sym]) -> bool {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, bool>> = Default::default();
    }
    // The C++ base is the last registered type of the chain: a QML component may share its
    // name with a registered type (the original's `IdleInhibitor.qml` button vs Quickshell's QObject).
    let r = registry();
    let Some((name, ti)) = types
        .iter()
        .rev()
        .find_map(|t| r.resolve(t).map(|x| (t, x.1)))
    else {
        return false;
    };
    if let Some(v) = CACHE.with_borrow(|c| c.get(name.as_str()).copied()) {
        return v;
    }
    let v = !r.chain(&ti.cpp).iter().any(|c| c.cpp == "QQuickItem");
    CACHE.with_borrow_mut(|c| c.insert(name.to_string(), v));
    v
}

/// The C++ class `types` end in, when the registry knows it.
pub fn cpp_class(types: &[crate::runtime::Sym]) -> Option<&'static str> {
    let r = registry();
    (types.iter()).find_map(|t| r.resolve(t).map(|(_, ti)| ti.cpp.as_str()))
}

/// Whether C++ class `cpp` is `base` or derives from it.
pub fn cpp_inherits(cpp: &str, base: &str) -> bool {
    registry().chain(cpp).iter().any(|c| c.cpp == base)
}

/// The name of the default property of the C++ type `types` end in: `defaultProperty` of the
/// nearest class in its chain (`data` for an Item, `flickableData` for a Flickable, `delegate`
/// for a Repeater). `Some(None)` for a class without one (QtObject, Timer), `None` for a type
/// the registry does not know.
pub fn default_prop(types: &[crate::runtime::Sym]) -> Option<Option<&'static str>> {
    let r = registry();
    let cpp = cpp_class(types)?;
    Some(
        r.chain(cpp)
            .into_iter()
            .find_map(|c| c.default_prop.as_deref()),
    )
}

pub fn default_component_prop(types: &[crate::runtime::Sym]) -> Option<String> {
    let r = registry();
    let d = types.iter().find_map(|t| {
        let (_, ti) = r.resolve(t)?;
        r.chain(&ti.cpp)
            .into_iter()
            .find_map(|c| c.default_prop.clone())
    })?;
    find_prop(types, &d)
        .is_some_and(|p| p.ty == "QQmlComponent*")
        .then_some(d)
}

/// QtQuick.Controls.Basic style paddings of the most derived known control type:
/// (`padding`, extra on `horizontalPadding`, extra on `leftPadding`).
pub fn basic_padding(types: &[crate::runtime::Sym]) -> Option<(f64, f64, f64)> {
    let r = registry();
    for t in types {
        let Some((_, ti)) = r.resolve(t) else {
            continue;
        };
        for c in r.chain(&ti.cpp) {
            return Some(match c.cpp.strip_prefix("QQuick").unwrap_or(&c.cpp) {
                "Button" | "DelayButton" => (6.0, 2.0, 0.0),
                "TextField" | "TextArea" => (6.0, 0.0, 4.0),
                "RoundButton" | "ToolButton" | "TabButton" | "CheckBox" | "RadioButton"
                | "Switch" | "MenuItem" | "Slider" | "RangeSlider" | "PageIndicator"
                | "BusyIndicator" | "ToolTip" => (6.0, 0.0, 0.0),
                "ItemDelegate" | "Popup" | "Pane" | "Frame" | "Dialog" | "DialogButtonBox" => {
                    (12.0, 0.0, 0.0)
                }
                "ScrollBar" | "ScrollIndicator" => (2.0, 0.0, 0.0),
                // topPadding/bottomPadding: 6 only (horizontal stays 0).
                "DayOfWeekRow" => (6.0, -6.0, 0.0),
                "Menu" | "Drawer" | "ToolBar" | "Control" | "Container" | "TabBar"
                | "AbstractButton" | "ComboBox" | "SpinBox" | "Dial" | "ProgressBar" | "Item"
                | "TextInput" | "TextEdit" | "QObject" | "ScrollView" | "Page" => return None,
                _ => continue,
            });
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Sym;
    use crate::value::Value;

    fn syms(names: &[&str]) -> Vec<Sym> {
        names.iter().map(|n| Sym::new(n)).collect()
    }

    fn ty(cpp: &str) -> &'static TypeInfo {
        registry()
            .by_cpp
            .get(cpp)
            .unwrap_or_else(|| panic!("{cpp} missing from the registry"))
    }

    fn prop(cpp: &str, name: &str) -> &'static PropInfo {
        find_prop_cpp(cpp, name).unwrap_or_else(|| panic!("{cpp}.{name} not found"))
    }

    fn chain_names(cpp: &str) -> Vec<&'static str> {
        registry()
            .chain(cpp)
            .iter()
            .map(|t| t.cpp.as_str())
            .collect()
    }

    /// Value of `Type.Value` as QML sees it.
    fn enum_value(qml: &str, value: &str) -> Option<f64> {
        registry()
            .enum_namespace(qml)
            .into_iter()
            .find(|(_, v, _)| v == value)
            .map(|(_, _, n)| n)
    }

    /// Value of `Type.Enum.Value` as QML sees it.
    fn scoped_enum_value(qml: &str, en: &str, value: &str) -> Option<f64> {
        registry()
            .enum_namespace(qml)
            .into_iter()
            .find(|(e, v, _)| e.as_deref() == Some(en) && v == value)
            .map(|(_, _, n)| n)
    }

    fn enum_info<'a>(t: &'a TypeInfo, name: &str) -> &'a EnumInfo {
        t.enums
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{}::{name} missing", t.cpp))
    }

    // qquickrectangle_p.h / qquickitem.h / qquicktext_p.h prototypes.
    #[test]
    fn prototype_chains_follow_the_cpp_hierarchy() {
        assert_eq!(
            chain_names("QQuickRectangle"),
            ["QQuickRectangle", "QQuickItem", "QObject"]
        );
        assert_eq!(ty("QQuickItem").prototype.as_deref(), Some("QObject"));
        assert_eq!(
            chain_names("QQuickText"),
            [
                "QQuickText",
                "QQuickImplicitSizeItem",
                "QQuickItem",
                "QObject"
            ]
        );
        assert_eq!(
            chain_names("QQuickRowLayout"),
            [
                "QQuickRowLayout",
                "QQuickLinearLayout",
                "QQuickGridLayoutBase",
                "QQuickLayout",
                "QQuickItem",
                "QObject"
            ]
        );
        assert!(inherits(&syms(&["Rectangle"]), "QQuickItem"));
        assert!(inherits(&syms(&["Text"]), "QQuickImplicitSizeItem"));
        assert!(!inherits(&syms(&["Timer"]), "QQuickItem"));
        assert!(is_popup(&syms(&["Menu"])));
        assert!(!is_popup(&syms(&["Rectangle"])));
    }

    // QObject as Qt's builtins.qmltypes describes it: objectName/objectNameChanged, toString()
    // and destroy() exist on every QObject.
    #[test]
    fn qobject_members_are_reached_through_the_chain() {
        assert_eq!(chain_names("QQuickRectangle").last(), Some(&"QObject"));
        assert_eq!(prop("QQuickRectangle", "objectName").ty, "QString");
        assert!(has_signal(&syms(&["Rectangle"]), "objectNameChanged"));
        assert!(has_method(&syms(&["Rectangle"]), "toString"));
        assert!(has_method(&syms(&["Rectangle"]), "destroy"));
        assert_eq!(
            registry()
                .resolve("QtObject")
                .map(|(m, t)| (m, t.cpp.as_str())),
            Some(("QML", "QObject"))
        );
    }

    #[test]
    fn properties_resolve_through_the_prototype_chain() {
        let rect = syms(&["Rectangle"]);
        assert_eq!(find_prop(&rect, "color").unwrap().ty, "QColor");
        assert_eq!(find_prop(&rect, "radius").unwrap().ty, "double");
        assert_eq!(find_prop(&rect, "topLeftRadius").unwrap().ty, "double");
        // Inherited from QQuickItem.
        assert_eq!(find_prop(&rect, "width").unwrap().ty, "double");
        assert_eq!(find_prop(&rect, "visible").unwrap().ty, "bool");
        assert_eq!(
            find_prop(&rect, "transformOrigin").unwrap().ty,
            "TransformOrigin"
        );
        assert_eq!(
            find_prop(&rect, "focusPolicy").unwrap().ty,
            "Qt::FocusPolicy"
        );
        // The most derived declaration wins: QsWindow's width is an int, not QQuickItem's double.
        assert_eq!(find_prop_cpp("ProxyWindowBase", "width").unwrap().ty, "int");
        assert_eq!(find_prop(&syms(&["Text"]), "color").unwrap().ty, "QColor");
        assert_eq!(
            find_prop(&syms(&["Text"]), "elide").unwrap().ty,
            "TextElideMode"
        );
        // Most derived type of the list first; later ones are fallbacks.
        let mixed = syms(&["NoSuchType", "Text"]);
        assert_eq!(find_prop(&mixed, "lineCount").unwrap().ty, "int");
        // Interned type-set lookups agree with the uncached walk.
        let set = type_set(&rect);
        assert_eq!(type_set(&rect), set);
        assert_ne!(type_set(&syms(&["Text"])), set);
        for name in ["color", "width", "noSuchProperty", "radius"] {
            assert_eq!(
                find_prop_in(set, &rect, name).map(|p| &p.ty),
                find_prop(&rect, name).map(|p| &p.ty)
            );
        }
    }

    #[test]
    fn property_flags_match_the_metadata() {
        let children = prop("QQuickItem", "children");
        assert_eq!(children.ty, "QQuickItem");
        assert!(children.list && children.readonly);
        let data = prop("QQuickItem", "data");
        assert_eq!(data.ty, "QObject");
        assert!(data.list && data.readonly);
        let anchors = prop("QQuickItem", "anchors");
        assert_eq!(anchors.ty, "QQuickAnchors*");
        assert!(anchors.readonly && !anchors.list);
        let parent = prop("QQuickItem", "parent");
        assert_eq!(parent.ty, "QQuickItem*");
        assert!(!parent.readonly && !parent.list);
        let rect = prop("QQuickItem", "childrenRect");
        assert_eq!(rect.ty, "QRectF");
        assert!(rect.readonly);
        assert!(prop("QQuickItem", "activeFocus").readonly);
        assert!(!prop("QQuickItem", "focus").readonly);
        let border = prop("QQuickRectangle", "border");
        assert_eq!(border.ty, "QQuickPen*");
        assert!(border.readonly);
        let count = prop("QQuickRepeater", "count");
        assert_eq!(count.ty, "int");
        assert!(count.readonly);
        assert_eq!(prop("QQuickRepeater", "delegate").ty, "QQmlComponent*");
        assert!(prop("QQuickText", "effectiveHorizontalAlignment").readonly);
        assert!(!prop("QQuickText", "horizontalAlignment").readonly);
        let screens = prop("RustshellGlobal", "screens");
        assert!(screens.list && screens.readonly);
        assert_eq!(screens.ty, "RustshellScreenInfo");
        assert!(prop("QQuickItem", "x").bindable && !prop("QQuickItem", "z").bindable);
        let ws = prop("qs::wm::WindowManagerQml", "windowsets");
        assert!(ws.bindable && ws.readonly);
        assert_eq!(ws.ty, "QList<Windowset*>");
        assert!(!prop("RustshellGlobal", "clipboardText").bindable);
    }

    #[test]
    fn attached_types() {
        let r = registry();
        let (m, keys) = r.resolve("Keys").unwrap();
        assert_eq!(m, "QtQuick");
        assert_eq!(keys.cpp, "QQuickKeysAttached");
        assert_eq!(keys.attached.as_deref(), Some("QQuickKeysAttached"));
        assert!(!keys.creatable);
        assert_eq!(prop("QQuickKeysAttached", "enabled").ty, "bool");

        let (m, layout) = r.resolve("Layout").unwrap();
        assert_eq!(m, "QtQuick.Layouts");
        assert_eq!(layout.cpp, "QQuickLayout");
        assert_eq!(layout.attached.as_deref(), Some("QQuickLayoutAttached"));
        assert_eq!(prop("QQuickLayoutAttached", "fillWidth").ty, "bool");
        assert_eq!(prop("QQuickLayoutAttached", "preferredWidth").ty, "double");
        assert_eq!(
            prop("QQuickLayoutAttached", "alignment").ty,
            "Qt::Alignment"
        );
        assert_eq!(prop("QQuickLayoutAttached", "rowSpan").ty, "int");
        assert!(!has_signal(&syms(&["Layout"]), "itemAdded"));

        let (m, win) = r.resolve("QsWindow").unwrap();
        assert_eq!(m, "Rustshell._Window");
        assert_eq!(win.attached.as_deref(), Some("QsWindowAttached"));
        let w = prop("QsWindowAttached", "window");
        assert_eq!(w.ty, "QObject*");
        assert!(w.readonly);
        assert!(
            ty("QsWindowAttached")
                .methods
                .iter()
                .any(|m| m == "itemRect")
        );

        assert_eq!(ty("QQuickRectangle").attached, None);

        // Qt's own .qmltypes of the Material style: the attached object is the style itself.
        let (m, material) = r.resolve("Material").unwrap();
        assert_eq!(m, "QtQuick.Controls.Material");
        assert_eq!(material.attached.as_deref(), Some("QQuickMaterialStyle"));
        assert!(!prop("QQuickMaterialStyle", "accent").readonly);
        assert!(prop("QQuickMaterialStyle", "primaryColor").readonly);

        // The shell's own member of a class of Quickshell's, beside Quickshell's own.
        let cpp = "qs::wayland::layershell::WlrLayershell";
        assert_eq!(
            r.attached("Rustshell.Wayland._WlrLayerShell", "WlrLayershell"),
            Some(cpp)
        );
        assert!(prop(cpp, "exclusiveEdge").enumeration);
        assert!(prop(cpp, "layer").enumeration);
        assert!(
            ty(cpp)
                .signals
                .iter()
                .any(|s| s.name == "exclusiveEdgeChanged")
        );
        assert!(!ty(cpp).exports.is_empty());
    }

    // Qt::AlignmentFlag (qnamespace.h): AlignLeft 0x1, AlignRight 0x2, AlignHCenter 0x4,
    // AlignJustify 0x8, AlignTop 0x20, AlignBottom 0x40, AlignVCenter 0x80, AlignCenter 0x84.
    #[test]
    fn alignment_enums_carry_qt_alignment_flag_values() {
        for (v, n) in [
            ("AlignLeft", 0x1),
            ("AlignRight", 0x2),
            ("AlignHCenter", 0x4),
            ("AlignJustify", 0x8),
            ("AlignTop", 0x20),
            ("AlignBottom", 0x40),
            ("AlignVCenter", 0x80),
        ] {
            assert_eq!(enum_value("Text", v), Some(n as f64), "Text.{v}");
        }
        assert_eq!(
            scoped_enum_value("Text", "HAlignment", "AlignHCenter"),
            Some(4.0)
        );
        assert_eq!(
            scoped_enum_value("Text", "VAlignment", "AlignVCenter"),
            Some(128.0)
        );
        assert_eq!(enum_value("TextInput", "AlignRight"), Some(2.0));
        assert_eq!(enum_value("Image", "AlignBottom"), Some(64.0));
        // QQuickFlickable::PositionMode reuses Qt::AlignmentFlag.
        assert_eq!(enum_value("Flickable", "AlignCenter"), Some(0x84 as f64));
        assert_eq!(enum_value("ListView", "AlignTop"), Some(0x20 as f64));
    }

    // QQuickItem::TransformOrigin (qquickitem.h): TopLeft = 0 ... BottomRight = 8, in order.
    #[test]
    fn item_transform_origin() {
        let names = [
            "TopLeft",
            "Top",
            "TopRight",
            "Left",
            "Center",
            "Right",
            "BottomLeft",
            "Bottom",
            "BottomRight",
        ];
        assert_eq!(enum_info(ty("QQuickItem"), "TransformOrigin").values, names);
        assert!(!enum_info(ty("QQuickItem"), "TransformOrigin").flag);
        for (i, v) in names.iter().enumerate() {
            assert_eq!(enum_value("Item", v), Some(i as f64), "Item.{v}");
            assert_eq!(
                scoped_enum_value("Item", "TransformOrigin", v),
                Some(i as f64)
            );
            // Inherited by every Item subtype.
            assert_eq!(enum_value("Rectangle", v), Some(i as f64), "Rectangle.{v}");
        }
        assert_eq!(enum_value("Text", "Center"), Some(4.0));
    }

    // QQuickItem::Flag (qquickitem.h): ItemClipsChildrenToShape 0x01 ... ItemObservesViewport 0x40.
    #[test]
    fn item_flags_are_a_flag_enum() {
        let e = enum_info(ty("QQuickItem"), "Flags");
        assert!(e.flag);
        for (v, n) in [
            ("ItemClipsChildrenToShape", 0x01),
            ("ItemAcceptsInputMethod", 0x02),
            ("ItemIsFocusScope", 0x04),
            ("ItemHasContents", 0x08),
            ("ItemAcceptsDrops", 0x10),
            ("ItemIsViewport", 0x20),
            ("ItemObservesViewport", 0x40),
        ] {
            assert!(e.values.iter().any(|x| x == v));
            assert_eq!(enum_value("Item", v), Some(n as f64), "Item.{v}");
        }
        assert!(enum_info(ty("QQuickAnchors"), "Anchors").flag);
    }

    // Text enums alias Qt::TextElideMode, Qt::TextFormat (+ StyledText = 4) and
    // QTextOption::WrapMode (qquicktext_p.h, qnamespace.h, qtextoption.h).
    #[test]
    fn text_enums_match_the_qt_headers() {
        for (v, n) in [
            ("ElideLeft", 0),
            ("ElideRight", 1),
            ("ElideMiddle", 2),
            ("ElideNone", 3),
        ] {
            assert_eq!(
                scoped_enum_value("Text", "TextElideMode", v),
                Some(n as f64),
                "Text.{v}"
            );
        }
        // Declaration order differs from the numeric values: values must not be indices.
        assert_eq!(
            enum_info(ty("QQuickText"), "TextFormat").values,
            [
                "PlainText",
                "RichText",
                "MarkdownText",
                "AutoText",
                "StyledText"
            ]
        );
        for (v, n) in [
            ("PlainText", 0),
            ("RichText", 1),
            ("AutoText", 2),
            ("MarkdownText", 3),
            ("StyledText", 4),
        ] {
            assert_eq!(enum_value("Text", v), Some(n as f64), "Text.{v}");
        }
        for (v, n) in [
            ("NoWrap", 0),
            ("WordWrap", 1),
            ("WrapAnywhere", 3),
            ("WrapAtWordBoundaryOrAnywhere", 4),
            ("Wrap", 4),
        ] {
            assert_eq!(enum_value("Text", v), Some(n as f64), "Text.{v}");
        }
        for (v, n) in [("Normal", 0), ("Outline", 1), ("Raised", 2), ("Sunken", 3)] {
            assert_eq!(enum_value("Text", v), Some(n as f64), "Text.{v}");
        }
        assert_eq!(enum_value("Text", "ProportionalHeight"), Some(0.0));
        assert_eq!(enum_value("Text", "FixedHeight"), Some(1.0));
        assert_eq!(enum_value("Text", "Fit"), Some(3.0));
    }

    #[test]
    fn attached_and_quickshell_enums() {
        // QQuickKeysAttached::Priority { BeforeItem, AfterItem }.
        assert_eq!(enum_value("Keys", "BeforeItem"), Some(0.0));
        assert_eq!(enum_value("Keys", "AfterItem"), Some(1.0));
        // QQuickLayout::SizePolicy { SizePolicyImplicit = 1, SizePolicyExplicit }.
        assert_eq!(enum_value("Layout", "SizePolicyImplicit"), Some(1.0));
        assert_eq!(enum_value("Layout", "SizePolicyExplicit"), Some(2.0));
        // Quickshell Edges::Enum: None 0, Top 1, Left 2, Right 4, Bottom 8.
        assert_eq!(
            enum_info(ty("Edges"), "Enum").values,
            ["None", "Top", "Left", "Right", "Bottom"]
        );
        for (v, n) in [
            ("None", 0),
            ("Top", 1),
            ("Left", 2),
            ("Right", 4),
            ("Bottom", 8),
        ] {
            assert_eq!(enum_value("Edges", v), Some(n as f64), "Edges.{v}");
        }
        // QQmlTimer has no enums of its own and nothing to inherit.
        assert!(registry().enum_namespace("Timer").is_empty());
    }

    #[test]
    fn exports_modules_and_registration_flags() {
        let r = registry();
        assert_eq!(
            ty("QQuickRectangle").exports,
            [("QtQuick".to_string(), "Rectangle".to_string())]
        );
        assert_eq!(
            r.by_name["Rectangle"],
            [("QtQuick".to_string(), "QQuickRectangle".to_string())]
        );
        let (m, rect) = r.resolve("Rectangle").unwrap();
        assert_eq!((m, rect.cpp.as_str()), ("QtQuick", "QQuickRectangle"));
        assert!(rect.creatable && !rect.singleton);
        assert_eq!(r.resolve("Timer").unwrap().1.cpp, "QQmlTimer");
        assert_eq!(r.resolve("Timer").unwrap().0, "QtQml");
        assert_eq!(r.resolve("RowLayout").unwrap().0, "QtQuick.Layouts");
        let (m, panel) = r.resolve("PanelWindow").unwrap();
        assert_eq!(
            (m, panel.cpp.as_str()),
            ("Rustshell._Window", "PanelWindowInterface")
        );
        assert!(!panel.creatable);
        let (m, n) = r.resolve("Notification").unwrap();
        assert_eq!(
            (m, n.cpp.as_str()),
            (
                "Rustshell.Services.Notifications",
                "qs::service::notifications::Notification"
            )
        );
        let (m, qs) = r.resolve("Rustshell").unwrap();
        assert_eq!((m, qs.cpp.as_str()), ("Rustshell", "RustshellGlobal"));
        assert!(qs.singleton && !qs.creatable);
        let wm = ty("qs::wm::WindowManagerQml");
        assert!(wm.singleton);
        assert_eq!(
            wm.exports,
            [(
                "Rustshell.WindowManager".to_string(),
                "WindowManager".to_string()
            )]
        );
        // ObjectModel is exported by QtQml.Models and Quickshell: Quickshell's wins.
        let mut mods: Vec<_> = r.by_name["ObjectModel"]
            .iter()
            .map(|(m, _)| m.as_str())
            .collect();
        mods.sort();
        assert_eq!(mods, ["QtQml.Models", "Rustshell"]);
        let (m, om) = r.resolve("ObjectModel").unwrap();
        assert_eq!((m, om.cpp.as_str()), ("Rustshell", "UntypedObjectModel"));
        assert_eq!(ty("QQmlObjectModel").exports[0].0, "QtQml.Models");
        // Opaque one-line forward declarations in other files do not replace the real type.
        assert!(!om.props.is_empty() || !om.methods.is_empty() || !om.signals.is_empty());
    }

    #[test]
    fn signals_and_methods() {
        let rect = syms(&["Rectangle"]);
        assert!(has_signal(&rect, "colorChanged"));
        assert!(has_signal(&rect, "radiusChanged"));
        assert!(has_signal(&rect, "childrenRectChanged"));
        assert!(has_signal(&rect, "windowChanged"));
        assert!(!has_signal(&rect, "itemAdded"));
        assert!(has_signal(&syms(&["Repeater"]), "itemAdded"));
        assert!(has_signal(&syms(&["Repeater"]), "itemRemoved"));
        assert!(has_signal(&syms(&["Text"]), "linkActivated"));
        assert!(has_signal(&syms(&["Text"]), "elideModeChanged"));
        assert!(has_signal(&syms(&["Rustshell"]), "reloadFailed"));
        assert!(has_signal(&syms(&["QsWindow"]), "closed"));

        let item = syms(&["Item"]);
        for m in [
            "mapToItem",
            "mapFromItem",
            "mapToGlobal",
            "forceActiveFocus",
            "childAt",
            "contains",
            "grabToImage",
            "nextItemInFocusChain",
        ] {
            assert!(has_method(&item, m), "Item.{m}");
            assert!(has_method(&rect, m), "Rectangle.{m} (inherited)");
        }
        assert!(has_method(&syms(&["Text"]), "linkAt"));
        assert!(has_method(&syms(&["Repeater"]), "itemAt"));
        assert!(!has_method(&rect, "itemAt"));
        for m in ["env", "iconPath", "shellPath", "execDetached", "hasVersion"] {
            assert!(has_method(&syms(&["Rustshell"]), m), "Quickshell.{m}");
        }
        // Overloads are kept per declaration (forceActiveFocus() and forceActiveFocus(reason)).
        let item_t = ty("QQuickItem");
        assert_eq!(
            item_t
                .methods
                .iter()
                .filter(|m| *m == "forceActiveFocus")
                .count(),
            2
        );
        assert_eq!(
            item_t.methods.iter().filter(|m| *m == "mapToItem").count(),
            4
        );
        // One-line `Parameter { name: ... }` blocks never leak into member names.
        for p in ["reason", "callback", "point", "item", "x", "forward"] {
            assert!(
                !item_t.methods.iter().any(|m| m == p),
                "QQuickItem method {p}"
            );
        }
        assert!(!ty("QQuickText").signals.iter().any(|s| s.name == "link"));
        assert!(!ty("QQuickItem").signals.iter().any(|s| s.name == "window"));
        assert!(
            !ty("QQuickRepeater")
                .signals
                .iter()
                .any(|s| s.name == "index")
        );
    }

    // quickshell-service-notifications.qmltypes: `Signal { name: "notification"` with a multi-line
    // `Parameter { ... }` block (NotificationServer.onNotification).
    #[test]
    fn multi_line_parameter_blocks_keep_the_member_name() {
        let server = ty("qs::service::notifications::NotificationServerQml");
        assert!(server.signals.iter().any(|s| s.name == "notification"));
        assert!(has_signal(&syms(&["NotificationServer"]), "notification"));
        for t in registry().by_cpp.values() {
            assert!(
                !t.signals.iter().any(|s| s.name.is_empty()),
                "{}: unnamed signal",
                t.cpp
            );
            assert!(
                !t.methods.iter().any(String::is_empty),
                "{}: unnamed method",
                t.cpp
            );
            assert!(
                !t.props.iter().any(|p| p.name.is_empty()),
                "{}: unnamed prop",
                t.cpp
            );
        }
    }

    #[test]
    fn an_impulse_type_wins_its_name_over_a_quickshell_one() {
        let mut r = Registry::load();
        assert_eq!(r.resolve("Process").unwrap().0, "Rustshell.Io");
        r.by_name
            .get_mut("Process")
            .unwrap()
            .push(("Impulse".into(), "ImpulseTestProbe".into()));
        let (module, t) = r.resolve("Process").unwrap();
        assert_eq!((module, t.cpp.as_str()), ("Impulse", "ImpulseTestProbe"));
    }

    /// A `.qmltypes` file is QML: a `//` comment is no part of it, whatever it quotes.
    #[test]
    fn every_component_of_every_table_file_is_read() {
        for (path, src) in data::QMLTYPES {
            let lines = || src.lines().map(str::trim);
            // An opaque value type, written on one line, has no members and is not kept.
            let components = lines().filter(|l| *l == "Component {").count();
            let properties = lines().filter(|l| l.starts_with("Property {")).count();
            let read = parse_qmltypes(src);
            let props: usize = read.iter().map(|t| t.props.len()).sum();
            assert_eq!(
                (read.len(), props),
                (components, properties),
                "{path}: components and properties read, and written in the file"
            );
        }
    }

    #[test]
    fn parser_skips_comments() {
        let src = r#"import QtQuick.tooling 1.2
// `found` carries the list of `{ at: [x, y], size: [width, height] }`. A comment that
// ends with a brace {
Module {
    Component {
        // name: "Other"
        name: "QFoo"
        exports: ["Foo/Foo 1.0"]
        Property { name: "running"; type: "bool" }
    }
}
"#;
        let read = parse_qmltypes(src);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].cpp, "QFoo");
        assert_eq!(read[0].exports, [("Foo".to_string(), "Foo".to_string())]);
        assert_eq!(read[0].props.len(), 1);
        assert_eq!(read[0].props[0].name, "running");
    }

    #[test]
    fn parser_reads_the_qmltypes_shapes() {
        let src = r#"import QtQuick.tooling 1.2
Module {
    Component {
        file: "x.h"
        name: "QFoo"
        accessSemantics: "reference"
        defaultProperty: "kids"
        prototype: "QQuickItem"
        extension: "QFooExt"
        attachedType: "QFooAttached"
        exports: ["Foo/Foo 1.0", "Foo/Foo 6.0", "Bar/OldFoo 2.0"]
        isCreatable: false
        Enum {
            name: "Mode"
            isFlag: true
            values: [
                "A",
                "B"
            ]
        }
        Enum { name: "Small"; values: ["X", "Y"] }
        Property { name: "kids"; type: "QObject"; isList: true; isReadonly: true }
        Property {
            name: "target"
            type: "QQuickItem"
            isPointer: true
            bindable: "bindableTarget"
        }
        Signal { name: "fired" }
        Signal {
            name: "moved"
            Parameter { name: "dx"; type: "double" }
        }
        Method { name: "go"; type: "bool" }
    }
    Component { isTypeOpaque: true; name: "Opaque"; accessSemantics: "value" }
    Component {
        name: "QSingle"
        isSingleton: true
    }
}
"#;
        let types = parse_qmltypes(src);
        assert_eq!(types.len(), 2);
        let t = &types[0];
        assert_eq!(t.cpp, "QFoo");
        assert_eq!(t.prototype.as_deref(), Some("QQuickItem"));
        assert_eq!(t.extension.as_deref(), Some("QFooExt"));
        assert_eq!(t.attached.as_deref(), Some("QFooAttached"));
        assert_eq!(t.default_prop.as_deref(), Some("kids"));
        assert!(!t.creatable && !t.singleton);
        assert_eq!(
            t.exports,
            [
                ("Foo".to_string(), "Foo".to_string()),
                ("Bar".to_string(), "OldFoo".to_string())
            ]
        );
        assert_eq!(t.enums.len(), 2);
        assert_eq!((t.enums[0].name.as_str(), t.enums[0].flag), ("Mode", true));
        assert_eq!(t.enums[0].values, ["A", "B"]);
        assert_eq!(t.enums[1].values, ["X", "Y"]);
        assert!(!t.enums[1].flag);
        let kids = &t.props[0];
        assert_eq!((kids.name.as_str(), kids.ty.as_str()), ("kids", "QObject"));
        assert!(kids.list && kids.readonly && !kids.bindable);
        let target = &t.props[1];
        assert_eq!(target.ty, "QQuickItem*");
        assert!(target.bindable && !target.readonly && !target.list);
        let sigs: Vec<_> = t
            .signals
            .iter()
            .map(|s| (s.name.as_str(), s.params.clone()))
            .collect();
        let dx = ParamInfo {
            name: "dx".into(),
            ty: "double".into(),
        };
        assert_eq!(sigs, [("fired", vec![]), ("moved", vec![dx])]);
        assert_eq!(t.methods, ["go"]);
        assert_eq!(types[1].cpp, "QSingle");
        assert!(types[1].singleton && types[1].creatable);
    }

    #[test]
    fn default_properties() {
        assert_eq!(ty("QQuickItem").default_prop.as_deref(), Some("data"));
        assert_eq!(ty("QQuickRectangle").default_prop.as_deref(), Some("data"));
        assert_eq!(
            ty("QQuickRepeater").default_prop.as_deref(),
            Some("delegate")
        );
        assert_eq!(
            default_component_prop(&syms(&["Repeater"])).as_deref(),
            Some("delegate")
        );
        // `data` is a QObject list, not a Component.
        assert_eq!(default_component_prop(&syms(&["Rectangle"])), None);
        assert_eq!(default_component_prop(&syms(&["NoSuchType"])), None);
    }

    /// The default property is the one of the nearest class that names it: QQuickItemView is
    /// a QQuickFlickable (`flickableData`), ListView, GridView and PathView say `data` again.
    #[test]
    fn the_default_property_is_the_nearest_class_s() {
        let of = |t: &str| default_prop(&syms(&[t]));
        assert_eq!(of("Item"), Some(Some("data")));
        assert_eq!(of("Flickable"), Some(Some("flickableData")));
        assert_eq!(
            ty("QQuickItemView").default_prop.as_deref(),
            Some("flickableData")
        );
        assert_eq!(of("ListView"), Some(Some("data")));
        assert_eq!(of("GridView"), Some(Some("data")));
        assert_eq!(of("PathView"), Some(Some("data")));
        assert_eq!(of("Pane"), Some(Some("contentData")));
        assert_eq!(of("Repeater"), Some(Some("delegate")));
        assert_eq!(of("QtObject"), Some(None));
        assert_eq!(of("Timer"), Some(None));
        assert_eq!(of("NoSuchType"), None);
        // A QML file's own name comes first; the C++ type is found behind it.
        assert_eq!(
            default_prop(&syms(&["MyFlick", "Flickable"])),
            Some(Some("flickableData"))
        );
    }

    #[test]
    fn cpp_defaults_follow_the_property_type() {
        let num = |v: Value| match v {
            Value::Num(n) => n,
            v => panic!("expected a number, got {v:?}"),
        };
        assert!(matches!(
            cpp_default(prop("QQuickItem", "parent")),
            Value::Null
        ));
        assert_eq!(num(cpp_default(prop("QQuickItem", "width"))), 0.0);
        assert!(matches!(
            cpp_default(prop("QQuickItem", "visible")),
            Value::Bool(false)
        ));
        assert!(matches!(cpp_default(prop("QQuickItem", "state")), Value::Str(s) if s.is_empty()));
        assert!(matches!(
            cpp_default(prop("QQuickRectangle", "color")),
            Value::Color(c) if c == crate::color::Color::INVALID
        ));
        assert!(matches!(
            cpp_default(prop("QQuickRepeater", "model")),
            Value::Undefined
        ));
        assert_eq!(
            cpp_default(prop("QQuickItem", "childrenRect")).to_js_string(),
            "QRectF(0, 0, 0, 0)"
        );
        assert!(matches!(
            cpp_default(prop("QQuickItem", "children")),
            Value::Obj(_)
        ));
        assert!(matches!(
            cpp_default(prop("QQuickItem", "focusPolicy")),
            Value::Num(_)
        ));
        assert_eq!(
            num(cpp_default(prop("QQuickLayoutAttached", "alignment"))),
            0.0
        );
    }

    // An enum-typed property holds a number in QML (QMetaProperty reads it as int).
    #[test]
    fn class_local_enum_properties_default_to_numbers() {
        assert!(matches!(
            cpp_default(prop("QQuickText", "elide")),
            Value::Num(_)
        ));
        assert!(matches!(
            cpp_default(prop("QQuickItem", "transformOrigin")),
            Value::Num(_)
        ));
    }

    #[test]
    fn unknown_names() {
        let r = registry();
        assert!(r.resolve("NoSuchType").is_none());
        assert!(r.chain("NoSuchCpp").is_empty());
        assert!(r.enum_namespace("NoSuchType").is_empty());
        let rect = syms(&["Rectangle"]);
        assert!(find_prop(&rect, "noSuchProperty").is_none());
        assert!(find_prop_cpp("NoSuchCpp", "width").is_none());
        // A real property name on an unknown type.
        assert!(find_prop(&syms(&["NoSuchType"]), "width").is_none());
        assert!(!has_signal(&rect, "noSuchSignal"));
        assert!(!has_method(&rect, "noSuchMethod"));
        assert!(!has_signal(&syms(&["NoSuchType"]), "colorChanged"));
        assert!(!inherits(&syms(&["NoSuchType"]), "QQuickItem"));
        assert_eq!(enum_value("Text", "NoSuchValue"), None);
        // Unknown types are treated as items; non-QQuickItem registered types are not.
        assert!(!is_non_item(&syms(&["NoSuchType"])));
        assert!(is_non_item(&syms(&["Timer"])));
        assert!(!is_non_item(&syms(&["Rectangle"])));
        // A QML component named like a registered QObject type stays an item if it derives one.
        assert!(!is_non_item(&syms(&["MyButton", "Rectangle"])));
        // Kirigami is not shipped: its Icon resolves to Item.
        assert_eq!(r.resolve("Kirigami.Icon").unwrap().1.cpp, "QQuickItem");
        assert!(!maybe_model_prop("noSuchProperty"));
        assert!(!maybe_model_prop("width"));
    }

    #[test]
    fn enum_values_table_is_keyed_by_module_and_qml_name() {
        let r = registry();
        let v = |m: &str, t: &str, n: &str| {
            r.enum_values
                .get(&(m.to_string(), t.to_string(), n.to_string()))
                .copied()
        };
        assert_eq!(v("QtQuick", "Text", "ElideRight"), Some(1.0));
        assert_eq!(v("QtQuick", "Item", "BottomRight"), Some(8.0));
        assert_eq!(
            v("QtQuick.Layouts", "Layout", "SizePolicyExplicit"),
            Some(2.0)
        );
        assert_eq!(v("Rustshell", "Edges", "Bottom"), Some(8.0));
        // QQmlBind::RestorationMode: RestoreNone 0, RestoreBinding 1, RestoreValue 2,
        // RestoreBindingOrValue 3.
        assert_eq!(v("QtQml", "Binding", "RestoreBindingOrValue"), Some(3.0));
        assert_eq!(v("QtQuick", "Rectangle", "ElideRight"), None);
    }
}
