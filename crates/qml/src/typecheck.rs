//! Object types a QML tree instantiates that the native runtime has no implementation of.
//! An unresolvable name is a load error in Qt; a C++ type native does not implement would
//! become a plain object (no behaviour, no signals), which is how missing `QsMenuAnchor` /
//! `SyntaxHighlighter` went unnoticed. Both are build errors of the AOT shell (ricompiled-gen)
//! and of `riaotcheck --strict`, reported per site as `file:line`.
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// C++ (qmltypes) types with a native implementation, by exported QML name.
pub const NATIVE: &[&str] = &[
    "Action",
    "AnchorAnimation",
    "AnchorChanges",
    "AnimatedImage",
    "ApplicationWindow",
    "Behavior",
    "Button",
    "Canvas",
    "ColorAnimation",
    "ColorOverlay",
    "ColorQuantizer",
    "Column",
    "ColumnLayout",
    "ComboBox",
    "Component",
    "Connections",
    "DayOfWeekRow",
    "DelegateChoice",
    "DelegateChooser",
    "Desaturate",
    "DesktopContainer",
    "DragHandler",
    "DropArea",
    "DropShadow",
    "EditController",
    "Flickable",
    "FloatingWindow",
    "Flow",
    "FocusScope",
    "FolderListModel",
    "FontMetrics",
    "FrameAnimation",
    "GaussianBlur",
    "GlobalShortcut",
    "Gradient",
    "GradientStop",
    "Grid",
    "GridLayout",
    "GridView",
    "HyprlandFocusGrab",
    "Icon",
    "IdleInhibitor",
    "Image",
    "Instantiator",
    "IpcHandler",
    "Item",
    "ItemDelegate",
    "JsonAdapter",
    "JsonObject",
    "LazyLoader",
    "ListElement",
    "ListModel",
    "ListView",
    "Loader",
    "Matrix4x4",
    "Menu",
    "MenuItem",
    "MouseArea",
    "MultiEffect",
    "NotificationServer",
    "NumberAnimation",
    "OpacityAnimator",
    "OpacityMask",
    "PamContext",
    "PanelContainer",
    "PanelWindow",
    "ParallelAnimation",
    "PathAngleArc",
    "PathLine",
    "PathPolyline",
    "PauseAnimation",
    "PolkitAgent",
    "Popup",
    "PopupWindow",
    "PositionSource",
    "Process",
    "ProgressBar",
    "PropertyAction",
    "PropertyAnimation",
    "PropertyChanges",
    "PwObjectTracker",
    "QsMenuOpener",
    "QtObject",
    "RadialGradient",
    "RadioButton",
    "Rectangle",
    "RectangularShadow",
    "Region",
    "Repeater",
    "Rotation",
    "RotationAnimation",
    "RotationAnimator",
    "Row",
    "RowLayout",
    "Scale",
    "ScaleAnimator",
    "Scope",
    "ScreencopyView",
    "ScriptAction",
    "ScriptModel",
    "ScrollBar",
    "ScrollView",
    "SequentialAnimation",
    "Shape",
    "ShapePath",
    "Shear",
    "ShellRoot",
    "SidebarContainer",
    "Singleton",
    "Slider",
    "SmoothedAnimation",
    "SpinBox",
    "SplitParser",
    "StackView",
    "State",
    "StdioCollector",
    "SwipeView",
    "Switch",
    "Synchronizer",
    "SyntaxHighlighter",
    "SystemClock",
    "TabBar",
    "TabButton",
    "Text",
    "TextArea",
    "TextEdit",
    "TextField",
    "TextInput",
    "TextMetrics",
    "Timer",
    "ToolTip",
    "Transition",
    "Translate",
    "Variants",
    "WheelHandler",
    "WidgetHost",
    "WidgetSettings",
    "Window",
    "WlSessionLock",
    "WlSessionLockSurface",
    "XAnimator",
    "YAnimator",
];
// Notes on entries whose native implementation is not a type of its own:
// - Animators run as the property animation on the property their type implies
//   (`runtime::animator`); `FrameAnimation` is driven by `World::step_frame_anims`.
// - `ListElement` outside a `ListModel` is, as in Qt (QQmlListElement), an object without
//   properties; inside one it is a row of the model (`runtime::list_elements`).
// - `PwObjectTracker` binds the nodes in `objects` so their properties are live; native `pw`
//   binds every node and device as its registry global appears, which satisfies that for any
//   `objects`.

/// Names that more than one module exports, of which the engine implements the one of the
/// module given here: the type of another module under that name is not implemented.
pub const NATIVE_OF: &[(&str, &str)] = &[("RadialGradient", "Qt5Compat.GraphicalEffects")];

/// The module whose type the bare `name` is in a document with these imports: the last import
/// without a qualifier that exports the name (QQmlImports: a later import shadows an earlier
/// one).
fn module_of<'a>(imports: &'a [crate::ast::Import], name: &str) -> Option<&'a str> {
    let reg = crate::types::registry();
    (imports.iter().rev())
        .filter(|i| i.alias.is_none())
        .map(|i| i.uri.as_str())
        .find(|uri| reg.exports(uri, name))
}

#[derive(Debug, Default)]
pub struct Report {
    /// Names neither a QML file, an inline component nor a C++ type resolves.
    pub unknown: BTreeMap<String, Vec<String>>,
    /// C++ types missing from `NATIVE`.
    pub unimplemented: BTreeMap<String, Vec<String>>,
    /// Every C++ type used.
    pub cpp: BTreeMap<String, usize>,
    /// Files that cannot be read or parsed, with the error: the tree is not checked there.
    pub broken: BTreeMap<String, String>,
}

impl Report {
    /// One `file:line: message` line per offending site, sorted; empty when the tree is clean.
    pub fn errors(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .broken
            .iter()
            .map(|(f, e)| format!("{f}: {e}"))
            .collect();
        for (t, sites) in &self.unknown {
            for s in sites {
                out.push(format!("{s}: {t} is not a type"));
            }
        }
        for (t, sites) in &self.unimplemented {
            for s in sites {
                out.push(format!("{s}: {t} has no native implementation"));
            }
        }
        out.sort();
        out
    }
}

/// Whether `name` is a C++ type the engine implements or the type registry knows.
pub fn native_type(name: &str) -> bool {
    NATIVE.contains(&name)
        || crate::impulse::element(name).is_some()
        || crate::types::registry().resolve(name).is_some()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "qml") {
            out.push(p);
        }
    }
}

/// Names of the inline components declared under `ms`.
pub(crate) fn components(ms: &[crate::ast::Member], out: &mut HashSet<String>) {
    use crate::ast::Member;
    for m in ms {
        match m {
            Member::Component { name, object } => {
                out.insert(name.clone());
                components(&object.members, out);
            }
            Member::Child(o) | Member::On { object: o, .. } => components(&o.members, out),
            Member::Group { members, .. } => components(members, out),
            _ => {}
        }
    }
}

/// Check every `.qml` under `root` (a shell config directory: `qs.*` imports resolve there).
pub fn check(root: &Path) -> Report {
    let mut files = vec![];
    walk(root, &mut files);
    files.sort();
    let reg = crate::types::registry();
    let mut r = Report::default();
    for f in &files {
        let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
        let doc = match std::fs::read_to_string(f) {
            Ok(src) => crate::parse(&src).map_err(|e| format!("parse error: {e}")),
            Err(e) => Err(format!("cannot read: {e}")),
        };
        let doc = match doc {
            Ok(d) => d,
            Err(e) => {
                r.broken.insert(rel, e);
                continue;
            }
        };
        let visible = crate::resolve::visible_types(root, f, &doc);
        let mut inline = HashSet::new();
        components(&doc.root.members, &mut inline);
        let aliases: HashSet<&str> = doc
            .imports
            .iter()
            .filter_map(|i| i.alias.as_deref())
            .collect();
        doc.root.walk(&mut |o| {
            let n = o.type_name.as_str();
            // Behind an import qualifier the parser has found the module of a C++ type.
            let module = match &o.scope {
                crate::ast::Scope::Module(m) => Some(&**m),
                _ => None,
            };
            if module.is_none() && (inline.contains(n) || visible.contains_key(n)) {
                return;
            }
            let bare = match n.split_once('.') {
                Some((a, b)) if module.is_some() || aliases.contains(a) => b,
                _ => n,
            };
            let site = format!("{rel}:{}", o.line);
            if module.is_none() && inline.contains(bare) {
                return;
            }
            let module = module.or_else(|| module_of(&doc.imports, bare));
            let other_module = NATIVE_OF
                .iter()
                .any(|(t, native)| *t == bare && module.is_some_and(|m| m != *native));
            if !other_module && (NATIVE.contains(&bare) || crate::impulse::element(bare).is_some())
            {
                *r.cpp.entry(bare.to_string()).or_default() += 1;
                return;
            }
            if reg.resolve(n).or_else(|| reg.resolve(bare)).is_none() {
                r.unknown.entry(n.to_string()).or_default().push(site);
                return;
            }
            *r.cpp.entry(bare.to_string()).or_default() += 1;
            r.unimplemented
                .entry(bare.to_string())
                .or_default()
                .push(site);
        });
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, files: &[(&str, &str)]) -> Report {
        let d = std::env::temp_dir().join(format!("qml-typecheck-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for (f, src) in files {
            std::fs::write(d.join(f), src).unwrap();
        }
        let r = check(&d);
        let _ = std::fs::remove_dir_all(&d);
        r
    }

    #[test]
    fn unknown_and_unimplemented_types_are_reported() {
        let r = fixture(
            "mixed",
            &[
                ("Own.qml", "import QtQuick\nItem {}\n"),
                (
                    "main.qml",
                    "import QtQuick\nimport Quickshell\nimport QtPositioning\nItem {\n component Inline: Rectangle {}\n\
                     Own {}\n Inline {}\n QsMenuAnchor {}\n SpringAnimation {}\n Bogus {}\n}\n",
                ),
            ],
        );
        assert_eq!(r.unknown.keys().collect::<Vec<_>>(), ["Bogus"]);
        assert_eq!(r.unknown["Bogus"], ["main.qml:10"]);
        assert_eq!(
            r.unimplemented.keys().collect::<Vec<_>>(),
            ["SpringAnimation"]
        );
        assert_eq!(
            r.errors(),
            [
                "main.qml:10: Bogus is not a type",
                "main.qml:9: SpringAnimation has no native implementation"
            ]
        );
    }

    /// `RadialGradient` is the Qt5Compat effect here; QtQuick.Shapes' gradient of that name is
    /// not drawn, and which of the two a document means follows its imports as in Qt.
    #[test]
    fn a_name_of_two_modules_is_native_only_from_the_implemented_one() {
        let doc = |imports: &str, ty: &str| {
            let src = format!("import QtQuick\n{imports}\nItem {{\n {ty} {{}}\n}}\n");
            fixture("two-modules", &[("main.qml", &src)]).errors()
        };
        let no = ["main.qml:5: RadialGradient has no native implementation"];
        let (fx, shapes) = ("import Qt5Compat.GraphicalEffects", "import QtQuick.Shapes");
        assert!(doc(&format!("{fx}\n"), "RadialGradient").is_empty());
        assert_eq!(doc(&format!("{shapes}\n"), "RadialGradient"), no);
        // The later import shadows the earlier one.
        assert_eq!(doc(&format!("{fx}\n{shapes}"), "RadialGradient"), no);
        assert!(doc(&format!("{shapes}\n{fx}"), "RadialGradient").is_empty());
        assert!(doc(&format!("{shapes} as S\n{fx}"), "RadialGradient").is_empty());
        assert_eq!(doc(&format!("{shapes} as S\n{fx}"), "S.RadialGradient"), no);
    }

    #[test]
    fn unparsable_files_are_errors() {
        let r = fixture(
            "broken",
            &[
                ("Bad.qml", "import QtQuick\nItem {\n"),
                ("Ok.qml", "import QtQuick\nItem {}\n"),
            ],
        );
        assert_eq!(r.broken.keys().collect::<Vec<_>>(), ["Bad.qml"]);
        let errors = r.errors();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("Bad.qml: parse error"), "{errors:?}");
    }

    /// Every object type `qml` instantiates resolves and is implemented natively.
    #[test]
    fn tree_type_inventory() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
        let r = check(&root);
        assert_eq!(r.errors(), Vec::<String>::new());
        for t in [
            "SyntaxHighlighter",
            "HyprlandFocusGrab",
            "QsMenuOpener",
            "FrameAnimation",
            "XAnimator",
        ] {
            assert!(r.cpp.contains_key(t), "{t}");
        }
        // Every `NATIVE` entry is a type the original instantiates or one of the Qt modules declares: an
        // entry misspelled, or left behind after its type is gone, fails here.
        let reg = crate::types::registry();
        let stale: Vec<_> = NATIVE
            .iter()
            .filter(|t| !r.cpp.contains_key(**t) && reg.resolve(t).is_none())
            .collect();
        assert_eq!(
            stale,
            Vec::<&&str>::new(),
            "NATIVE entries that name no type"
        );
    }
}
