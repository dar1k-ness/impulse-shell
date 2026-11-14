//! Methods Qt (and Quickshell) declare that a call in `qml/` may reach on a value whose type
//! only a run shows (`docs/unwritten-methods.tsv`), written as Qt 6.12 has them
//! (`tests/unwritten/may.qml`, `tests/unwritten/rustshell/shell.qml`).
use super::*;

/// The QML types whose methods the object `item` has: its own, and for the object of a
/// grouped property (`popup.anchor`) the type of that property.
pub(crate) fn types_of(w: &World, item: usize) -> Vec<Sym> {
    let Some(inst) = w.items.get(item) else {
        return Vec::new();
    };
    let Some((owner, prop)) = w.groups.get(&item) else {
        return inst.types.clone();
    };
    let r = crate::types::registry();
    let class = (w.items.get(*owner))
        .and_then(|o| o.prop_info(prop))
        .and_then(|p| r.by_cpp.get(p.ty.trim_end_matches('*')))
        .and_then(|t| t.exports.first());
    class.map_or_else(Vec::new, |(_, n)| vec![Sym::new(n)])
}

/// QV4's error for a call no overload of `name` takes.
fn no_overload(name: &str, candidates: &[&str]) -> HostError {
    let list: String = (candidates.iter())
        .map(|c| format!("\n    {name}({c})"))
        .collect();
    HostError::new(
        "Error",
        format!("Unable to determine callable overload.  Candidates are:{list}"),
    )
}

/// `QRegularExpression::wildcardToRegularExpression` on Unix: `*` and `?` stop at a `/`.
fn wildcard(glob: &str) -> String {
    let mut out = String::new();
    let mut cs = glob.chars().peekable();
    while let Some(c) = cs.next() {
        match c {
            '*' => out += "[^/]*",
            '?' => out += "[^/]",
            '[' => {
                out.push('[');
                if cs.next_if_eq(&'!').is_some() {
                    out.push('^');
                }
                if cs.next_if_eq(&']').is_some() {
                    out += "\\]";
                }
                for c in cs.by_ref() {
                    match c {
                        '\\' => out += "\\\\",
                        _ => out.push(c),
                    }
                    if c == ']' {
                        break;
                    }
                }
            }
            c if c.is_alphanumeric() || c == '_' => out.push(c),
            c => {
                out.push('\\');
                out.push(c);
            }
        }
    }
    out
}

/// QQuickComboBoxPrivate::match from row 0: the first row whose text `text` matches by the
/// Qt::MatchFlags `flags`, or -1.
fn combo_find(w: &mut World, item: usize, text: &str, flags: u32) -> f64 {
    let sensitive = flags & 16 != 0;
    let fold = |s: &str| match sensitive {
        true => s.to_owned(),
        false => s.to_lowercase(),
    };
    let kind = flags & 0x0f;
    let rx = match kind {
        4 => Some(text.to_owned()),
        5 => Some(wildcard(text)),
        _ => None,
    }
    .and_then(|p| {
        let i = if sensitive { "" } else { "(?i)" };
        fancy_regex::Regex::new(&format!("{i}\\A(?:{p})\\z")).ok()
    });
    let want = fold(text);
    let count = w.combo_box_prop(item, "count").to_number() as usize;
    for row in 0..count {
        let t = w.combo_text_at(item, row as f64);
        let found = match kind {
            0 => t == text,
            4 | 5 => (rx.as_ref()).is_some_and(|rx| rx.is_match(&t).unwrap_or(false)),
            2 => fold(&t).starts_with(&want),
            3 => fold(&t).ends_with(&want),
            8 => fold(&t) == want,
            _ => fold(&t).contains(&want),
        };
        if found {
            return row as f64;
        }
    }
    -1.0
}

/// QQuickCanvasItem::save: the canvas as an image file, of the format its suffix names.
fn canvas_save(world: &Rc<RefCell<World>>, item: usize, args: &[Value]) -> bool {
    let name = args[0].to_js_string();
    let (path, has_context) = {
        let w = world.borrow();
        let url = w.resolved_url(item, &name);
        (
            url.strip_prefix("file://").unwrap_or(&url).to_owned(),
            w.items[item].props.contains_key("\x01ctx2d"),
        )
    };
    // QQuickCanvasItem::toImage is a null image until the canvas has a context.
    if name.is_empty() || !has_context {
        return false;
    }
    let Some(img) = crate::context2d::image_of(world, item) else {
        return false;
    };
    // toImage(QRectF(QPointF(0, 0), imageSize)): QImage::copy of that rectangle, the part
    // outside the canvas transparent; the whole canvas for an empty size.
    let side = |k: &str| match args.get(1) {
        Some(Value::Obj(o)) => o.borrow().props.get(k).map_or(0.0, |v| v.to_number()),
        _ => 0.0,
    };
    let (sw, sh) = (side("width").round(), side("height").round());
    let (cw, ch) = match sw >= 1.0 && sh >= 1.0 {
        true => (sw as usize, sh as usize),
        false => (img.w, img.h),
    };
    let mut px = vec![0u8; cw * ch * 4];
    for y in 0..ch.min(img.h) {
        let n = cw.min(img.w) * 4;
        px[y * cw * 4..y * cw * 4 + n].copy_from_slice(&img.px[y * img.w * 4..y * img.w * 4 + n]);
    }
    if cw == 0 || ch == 0 {
        return false;
    }
    image::save_buffer(path, &px, cw as u32, ch as u32, image::ColorType::Rgba8).is_ok()
}

/// The call of `name` on `item` when it is one of these methods; with no `args`, whether it
/// is one.
pub(super) fn call(
    host: &mut WorldHost,
    item: usize,
    name: &str,
    args: Option<&[Value]>,
) -> Option<HostR> {
    let types = {
        let w = host.0.borrow();
        if w.items.get(item)?.props.contains_key(name) {
            return None;
        }
        types_of(&w, item)
    };
    let is = |cpp: &str| crate::types::inherits(&types, cpp);
    let model = is("QAbstractItemModel");
    let written = match name {
        "find" | "selectAll" => is("QQuickComboBox"),
        "dismiss" => is("QQuickMenu"),
        "save" => is("QQuickCanvasItem"),
        "createObject" => is("QQmlAbstractDelegateComponent"),
        "sort" | "submit" | "match" => model,
        "indexOf" => is("UntypedObjectModel"),
        "updateAnchor" => is("PopupAnchor"),
        _ => false,
    };
    if !written {
        return None;
    }
    let Some(args) = args else {
        return Some(Ok(Value::Undefined));
    };
    let world = host.0.clone();
    let mut w = world.borrow_mut();
    Some(match name {
        "find" if args.is_empty() => Err(no_overload(name, &["QString", "QString,Qt::MatchFlags"])),
        "find" => {
            // Qt::MatchExactly is 0: what `find(text)` matches by.
            let flags = args.get(1).map_or(0.0, |f| f.to_number()) as u32;
            let text = args[0].to_js_string();
            Ok(Value::Num(combo_find(&mut w, item, &text, flags)))
        }
        "selectAll" => {
            // QQuickComboBox::selectAll: the text of a `contentItem` that is a QQuickTextInput.
            let input = match w.read(item, "contentItem") {
                Some(Value::Item(c)) if !w.dead.contains(&c) => Some(c),
                _ => None,
            }
            .filter(|&c| crate::types::inherits(&w.items[c].types, "QQuickTextInput"));
            drop(w);
            if let Some(c) = input {
                host.method(c, "selectAll", Some(&[]));
            }
            Ok(Value::Undefined)
        }
        "dismiss" => {
            // QQuickMenu::dismiss: this menu and each menu it is a submenu of.
            let mut menu = Some(item);
            while let Some(m) = menu.take() {
                w.popup_set_visible(m, false);
                let parent = match w.read(m, "parent") {
                    Some(Value::Item(p)) if !w.dead.contains(&p) => p,
                    _ => break,
                };
                if crate::types::inherits(&w.items[parent].types, "QQuickMenuItem")
                    && let Some(Value::Item(up)) = w.read(parent, "menu")
                    && up != m
                    && !w.dead.contains(&up)
                {
                    menu = Some(up);
                }
            }
            Ok(Value::Undefined)
        }
        "save" if args.is_empty() => Err(no_overload(name, &["QString", "QString,QSizeF"])),
        "save" => {
            drop(w);
            Ok(Value::Bool(canvas_save(&world, item, args)))
        }
        // QQmlComponent::createObject of a component that is no document of an engine
        // ("QQmlComponent: Must provide an engine before calling create").
        "createObject" => Ok(Value::Null),
        "sort" if args.is_empty() => Err(no_overload(name, &["int", "int,Qt::SortOrder"])),
        // QAbstractItemModel::sort: nothing, for a model that does not sort itself.
        "sort" => Ok(Value::Undefined),
        // QAbstractItemModel::submit.
        "submit" => Ok(Value::Bool(true)),
        "match" if args.len() < 3 => Err(no_overload(
            name,
            &[
                "QModelIndex,int,QVariant",
                "QModelIndex,int,QVariant,int",
                "QModelIndex,int,QVariant,int,Qt::MatchFlags",
            ],
        )),
        // No value a script has here is a QModelIndex (`index()` is not written).
        "match" => Err(HostError::new(
            "TypeError",
            "Passing incompatible arguments to C++ functions from JavaScript is not allowed.",
        )),
        "indexOf" if args.is_empty() => Err(HostError::new("Error", "Insufficient arguments")),
        "indexOf" => {
            // UntypedObjectModel::indexOf: the row of the object, or -1.
            let rows = match w.read(item, "values") {
                Some(v) => model_elems(&w, &v),
                None => Vec::new(),
            };
            let at = rows.iter().position(|r| match (r, &args[0]) {
                (Value::Item(a), Value::Item(b)) => a == b,
                _ => false,
            });
            Ok(Value::Num(at.map_or(-1.0, |i| i as f64)))
        }
        // PopupAnchor::updateAnchor: the rectangle of `anchor.item` is read again at every
        // placement of the popup here, which leaves the signal.
        "updateAnchor" => {
            drop(w);
            host.emit(item, "anchoring", &[]);
            Ok(Value::Undefined)
        }
        _ => return None,
    })
}
