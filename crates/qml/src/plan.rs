//! Per-component creation plans: the first instance of a template is built by walking the
//! QML tree; later instances copy its freshly created (pre-finalize) state with ids remapped.
use crate::runtime::{Code, Ctx, DKey, Inst, Prop, Props, Sym, Template, World};
use crate::value::Value;
use rustc_hash::FxHashMap as HashMap;

/// A function or signal value rebuilt per instance (it captures the instance scope).
pub enum Rebuild {
    /// `script` is an owned clone: its `src` is shared with the AST it came from, so it keeps
    /// the script's identity (AOT entry) without pointing into a template that may be freed.
    Function {
        item: usize,
        ctx: usize,
        name: String,
        script: crate::Script,
    },
    Signal {
        item: usize,
        name: String,
    },
    /// `enum` objects hold only numbers: copied into a fresh object per instance.
    Enum {
        item: usize,
        name: String,
    },
    /// A literal `focus: true`: whether the item keeps it depends on what its focus scope
    /// holds when the instance is made (`World::literal_focus`).
    Focus {
        item: usize,
    },
    /// The JS files the document of context `ctx` imports that are no `.pragma library`:
    /// each instance runs them again, in its own context `script` (in Qt each instance's
    /// context has its own script values; the code is compiled once).
    Scripts {
        ctx: usize,
        script: usize,
        file: std::path::PathBuf,
    },
}

/// One created item's state, copied into each later instance.
pub type PlanItem = (
    Vec<crate::runtime::Sym>,
    Option<usize>,
    Vec<usize>,
    Props,
    HashMap<Sym, Vec<Code>>,
    bool,
    // `Inst::layout` and `Inst::class`.
    (Option<&'static [&'static str]>, Option<&'static str>),
    Option<u32>,
    (u32, u32),
);
/// One created scope context, copied into each later instance.
/// (ids, root object, parent, `Ctx::script`)
pub type PlanCtx = (Vec<(String, usize)>, usize, Option<usize>, bool);

pub struct Plan {
    pub items: Vec<PlanItem>,
    pub ctxs: Vec<PlanCtx>,
    pub n0: usize,
    pub c0: usize,
    pub rebuild: Vec<Rebuild>,
    /// Bound properties holding a JS object: re-evaluated per instance instead of shared.
    pub rebind: Vec<DKey>,
}

pub struct Map {
    pub n0: usize,
    pub n1: usize,
    pub m0: usize,
    pub c0: usize,
    pub c1: usize,
    pub k0: usize,
}

impl Map {
    pub fn item(&self, x: usize) -> usize {
        if (self.n0..self.n1).contains(&x) {
            x - self.n0 + self.m0
        } else {
            x
        }
    }
    pub fn ctx(&self, c: usize) -> usize {
        if (self.c0..self.c1).contains(&c) {
            c - self.c0 + self.k0
        } else {
            c
        }
    }
    pub fn key(&self, k: DKey) -> DKey {
        (self.item(k.0), k.1)
    }
    pub fn code(&self, c: &Code) -> Code {
        let mut c = c.clone();
        c.ctx = self.ctx(c.ctx);
        c
    }
}

fn rebuilt(rb: &[Rebuild], item: usize, name: &str) -> bool {
    rb.iter().any(|r| match r {
        Rebuild::Function {
            item: i, name: n, ..
        }
        | Rebuild::Signal { item: i, name: n }
        | Rebuild::Enum { item: i, name: n } => *i == item && n == name,
        Rebuild::Focus { .. } | Rebuild::Scripts { .. } => false,
    })
}

/// Snapshot items `[n0, n1)` and contexts `[c0, c1)` right after `create`; `None` when some
/// value cannot be copied safely (a JS object other than a rebuilt function).
pub fn capture(w: &World, n0: usize, c0: usize, rebuild: Vec<Rebuild>) -> Option<Plan> {
    let (n1, c1) = (w.items.len(), w.ctxs.len());
    // The JS files of a script context run again for each instance, with its own objects:
    // one made while nothing recorded it cannot be made again.
    let scripts = |c: usize| {
        rebuild
            .iter()
            .any(|r| matches!(r, Rebuild::Scripts { script, .. } if *script == c))
    };
    if (c0..c1).any(|c| w.ctxs.get(c).is_some_and(|x| x.script) && !scripts(c)) {
        return None;
    }
    let mut items = Vec::with_capacity(n1 - n0);
    let mut rebind = Vec::new();
    for i in n0..n1 {
        let it = &w.items[i];
        for (k, p) in it.props.iter() {
            if let Value::Obj(o) = &p.value
                && matches!(o.borrow().call, Some(crate::eval::Callable::Native(..)))
            {
                continue;
            }
            if matches!(p.value, Value::Obj(_)) && p.binding.is_some() {
                rebind.push((i, *k));
                continue;
            }
            if matches!(p.value, Value::Obj(_)) && !rebuilt(&rebuild, i, k.as_str()) {
                if crate::log_on!("ri::plandbg") {
                    tracing::debug!(
                        target: "ri::plandbg",
                        "planblock {:?} {} bound {}",
                        it.types.first().map(|t| t.as_str()),
                        k.as_str(),
                        p.binding.is_some()
                    );
                }
                return None;
            }
        }
        items.push((
            it.types.clone(),
            it.parent,
            it.children.clone(),
            it.props.clone(),
            it.handlers.clone(),
            it.proxy,
            (it.layout, it.class),
            it.ctx,
            it.at,
        ));
    }
    let ctxs = (c0..c1)
        .map(|c| {
            let x = &w.ctxs[c];
            (
                x.ids.iter().map(|(k, v)| (k.clone(), *v)).collect(),
                x.root,
                x.parent,
                x.script,
            )
        })
        .collect();
    Some(Plan {
        items,
        ctxs,
        n0,
        c0,
        rebuild,
        rebind,
    })
}

pub fn map_value(m: &Map, v: &Value) -> Value {
    match v {
        Value::Item(j) => Value::Item(m.item(*j)),
        v => v.clone(),
    }
}

pub fn map_prop(m: &Map, p: &Prop) -> Prop {
    let mut p = p.clone();
    p.value = map_value(m, &p.value);
    if let Some(c) = &mut p.binding {
        c.ctx = m.ctx(c.ctx);
    }
    if let Some(c) = &mut p.base {
        c.ctx = m.ctx(c.ctx);
    }
    for d in &mut p.deps {
        *d = m.key(*d);
    }
    if let Some(a) = &mut p.alias {
        a.ctx = m.ctx(a.ctx);
    }
    p
}

/// Lengths of the append-only World tables before `create`.
#[derive(Clone, Copy)]
pub struct Pre {
    pub touched: usize,
    pub restructured: usize,
    pub completed: usize,
    pub lrev_items: usize,
    pub resized: usize,
    pub owned_templates: usize,
    pub changed: usize,
    pub assigned_log: usize,
    pub required: usize,
}

pub fn pre(w: &World) -> Pre {
    Pre {
        touched: w.touched.len(),
        restructured: w.restructured.len(),
        completed: w.completed.len(),
        lrev_items: w.lrev_items.len(),
        resized: w.resized.len(),
        owned_templates: w.owned_templates.len(),
        changed: w.changed.len(),
        assigned_log: w.assigned_log.len(),
        required: w.required.len(),
    }
}

/// Per-instance entries `create` added to World side tables.
#[derive(Default)]
pub struct Tables {
    /// Queued bindings, each with whether it is pending (`DirtyQueue::push_pending`).
    pub dirty: Vec<(DKey, bool)>,
    pub fresh_bind: Vec<(DKey, u8)>,
    pub assigned: Vec<DKey>,
    /// `required` declarations inside the instance, checked when its creation ends.
    pub required: Vec<crate::runtime::Required>,
    pub completed: Vec<usize>,
    pub touched: Vec<usize>,
    pub restructured: Vec<usize>,
    pub lrev_items: Vec<usize>,
    /// Items a write asked a layout for (`World::resized`): a copied instance is laid out as
    /// the one walked from its QML is.
    pub resized: Vec<usize>,
    /// Images whose `source` was written (`World::sourced`).
    pub sourced: Vec<usize>,
    pub changed: Vec<DKey>,
    pub owned: Vec<(usize, Vec<usize>)>,
    pub root_ctxs: Vec<(usize, Vec<usize>)>,
    pub obj_refs: Vec<(usize, usize, String)>,
    pub ctx_js: Vec<(usize, HashMap<String, Value>)>,
    pub ctx_files: Vec<(usize, std::path::PathBuf)>,
    pub signals: Vec<(usize, HashMap<String, Vec<crate::runtime::SigParam>>)>,
    pub ipc_decls: Vec<(usize, Vec<crate::runtime::IpcDecl>)>,
    pub behaviors: Vec<(DKey, usize)>,
    pub deferred: Vec<(usize, Vec<DKey>)>,
    pub lazy_anims: Vec<(usize, crate::runtime::LazyAnim)>,
    pub deferred_rows: Vec<(usize, Vec<crate::runtime::DeferredRow>)>,
    pub lazy_ready: Vec<usize>,
    /// Items whose `layer.enabled` or `layer.effect` was written (`World::layer_ready`): each
    /// instance gets its own effect.
    pub layer_ready: Vec<usize>,
    pub state_hosts: Vec<usize>,
    /// `Synchronizer` objects inside the instance (`World::synchronizers`).
    pub synchronizers: Vec<usize>,
    /// Items whose aliases await resolution when the instance is complete.
    pub alias_pending: Vec<usize>,
    /// Grouped-property views (`World::groups`) created inside the instance.
    pub groups: Vec<(usize, (usize, String))>,
    /// Flickables inside the instance with their contentItems (`World::content_item_of`).
    pub content: Vec<(usize, usize)>,
    /// By value: the first instance's templates are released when it is destroyed.
    pub owned_templates: Vec<(usize, Template)>,
}

pub fn tables(w: &World, p: &Pre, n0: usize, c0: usize) -> Tables {
    let (n1, c1) = (w.items.len(), w.ctxs.len());
    let ri = |x: &usize| (n0..n1).contains(x);
    let ic = |x: &usize| (c0..c1).contains(x);
    Tables {
        dirty: w.dirty.entries().collect(),
        fresh_bind: w
            .fresh_bind
            .iter()
            .filter(|(k, _)| ri(&k.0))
            .map(|(k, v)| (*k, *v))
            .collect(),
        assigned: {
            let mut seen = std::collections::HashSet::new();
            w.assigned_log[p.assigned_log..]
                .iter()
                .filter(|k| ri(&k.0) && w.assigned.contains(*k) && seen.insert(**k))
                .copied()
                .collect()
        },
        required: w.required[p.required..]
            .iter()
            .filter(|r| ri(&r.key.0))
            .cloned()
            .collect(),
        completed: w.completed[p.completed..].to_vec(),
        touched: w.touched[p.touched..].to_vec(),
        restructured: w.restructured[p.restructured..].to_vec(),
        lrev_items: w.lrev_items[p.lrev_items..].to_vec(),
        resized: w.resized.get(p.resized..).unwrap_or_default().to_vec(),
        sourced: (n0..n1).filter(|i| w.sourced.contains(i)).collect(),
        changed: w.changed[p.changed..].iter().map(|c| c.0).collect(),
        owned: w
            .owned
            .iter()
            .filter(|(k, _)| ri(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        root_ctxs: w
            .root_ctxs
            .iter()
            .filter(|(k, _)| ri(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        obj_refs: w
            .obj_refs
            .iter()
            .flat_map(|(t, v)| v.iter().map(move |(i, n)| (*t, *i, n.clone())))
            .filter(|(t, i, _)| ri(t) || ri(i))
            .collect(),
        ctx_js: w
            .ctx_js
            .iter()
            .filter(|(k, _)| ic(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        ctx_files: w
            .ctx_files
            .iter()
            .filter(|(k, _)| ic(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        signals: w
            .signals
            .iter()
            .filter(|(k, _)| ri(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        ipc_decls: w
            .ipc_decls
            .iter()
            .filter(|(k, _)| ri(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        behaviors: w
            .behaviors
            .iter()
            .filter(|(k, _)| ri(&k.0))
            .map(|(k, v)| (*k, *v))
            .collect(),
        deferred: w
            .deferred
            .iter()
            .filter(|(k, _)| ri(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        lazy_anims: w
            .lazy_anims
            .iter()
            .filter(|(k, _)| ri(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        deferred_rows: w
            .deferred_rows
            .iter()
            .filter(|(k, _)| ri(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        lazy_ready: w.lazy_ready.iter().filter(|k| ri(k)).copied().collect(),
        layer_ready: w.layer_ready.iter().filter(|k| ri(k)).copied().collect(),
        state_hosts: w.state_hosts.iter().filter(|k| ri(k)).copied().collect(),
        synchronizers: {
            let mut v: Vec<usize> = w
                .synchronizers
                .keys()
                .chain(w.sync_new.iter())
                .copied()
                .filter(|k| ri(k))
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        },
        alias_pending: w.alias_pending.iter().copied().filter(|k| ri(k)).collect(),
        groups: w
            .groups
            .iter()
            .filter(|(g, _)| ri(g))
            .map(|(g, k)| (*g, k.clone()))
            .collect(),
        content: {
            let mut v: Vec<(usize, usize)> = w
                .content_item_of
                .iter()
                .filter(|(_, c)| ri(c))
                .map(|(f, c)| (*f, *c))
                .collect();
            v.sort_unstable();
            v
        },
        owned_templates: w.owned_templates[p.owned_templates..]
            .iter()
            .filter_map(|&(o, t)| w.templates.get(t).map(|t| (o, t.clone())))
            .collect(),
    }
}

impl Map {
    pub fn item_ext(&self, x: usize, ext: (Option<usize>, Option<usize>)) -> usize {
        if ext.0 == Some(x) {
            ext.1.unwrap_or(x)
        } else {
            self.item(x)
        }
    }
}

/// Copy `plan` as a new instance under `parent`, its first context being the fresh `k0`.
/// Returns the id map; functions, signals, enums and `rebind` are left to the caller.
pub fn replay(w: &mut World, plan: &Plan, t: &Tables, parent: Option<usize>, k0: usize) -> Map {
    let parent = w.item_parent(&plan.items[0].0, parent);
    let m = Map {
        n0: plan.n0,
        n1: plan.n0 + plan.items.len(),
        m0: w.items.len(),
        c0: plan.c0,
        c1: plan.c0 + plan.ctxs.len(),
        k0,
    };
    let ext = (plan.items[0].1, parent);
    let im = |x: usize| m.item_ext(x, ext);
    for (j, (types, par, children, props, handlers, proxy, layout, ctx, at)) in
        plan.items.iter().enumerate()
    {
        let mut it = Inst::default();
        it.types = types.clone();
        it.parent = if j == 0 { parent } else { par.map(im) };
        it.children = children.iter().map(|&c| m.item(c)).collect();
        let mut ps = props.clone();
        for p in ps.values_mut() {
            *p = map_prop(&m, p);
        }
        it.props = ps;
        it.handlers = handlers
            .iter()
            .map(|(k, v)| (*k, v.iter().map(|c| m.code(c)).collect()))
            .collect();
        it.proxy = *proxy;
        (it.layout, it.class) = *layout;
        it.ctx = ctx.map(|c| m.ctx(c as usize) as u32);
        it.at = *at;
        if it.types.iter().any(|t| t == "FontMetrics") {
            w.text_dirty.insert(m.m0 + j);
        }
        if w.text_measure.is_some() && it.types.iter().any(|t| t == "Text") {
            w.text_unfinished.insert(m.m0 + j, false);
        }
        w.items.push(it);
        if w.track_fresh {
            w.fresh.insert(m.m0 + j);
        }
    }
    for (j, (ids, root, par, script)) in plan.ctxs.iter().enumerate() {
        let c = Ctx {
            ids: Default::default(),
            root: if *root == usize::MAX {
                usize::MAX
            } else {
                m.item(*root)
            },
            // The instance's own context, made (and marked Bound) before the plan replays,
            // under the context its creation gave it (a delegate's is its model row's).
            parent: match j {
                0 => w.ctxs[k0].parent,
                _ => par.map(|p| m.ctx(p)),
            },
            bound: j == 0 && w.ctxs[k0].bound,
            document: j == 0 && w.ctxs[k0].document,
            script: *script,
            declared: j == 0 && w.ctxs[k0].declared,
        };
        let id = if j == 0 {
            w.ctxs[k0] = c;
            k0
        } else {
            w.ctxs.push(c)
        };
        for (k, v) in ids {
            w.ctxs[id].ids.insert(k.clone(), m.item(*v));
        }
    }
    w.revision += 1;
    let root = m.m0;
    if let Some(p) = parent {
        w.bump_tree(p);
        w.items[p].children.push(root);
        w.children_changed(p);
        w.owned.entry(p).or_default().push(root);
    }
    let inr = |x: &usize| (m.n0..m.n1).contains(x);
    let cause = w.sched.cur();
    for &(k, pending) in &t.dirty {
        if inr(&k.0) {
            if pending {
                w.dirty.push_pending(m.key(k), cause);
            } else {
                w.dirty.push(m.key(k), cause);
            }
        }
    }
    for (k, v) in &t.fresh_bind {
        w.fresh_bind.insert(m.key(*k), *v);
    }
    for k in &t.assigned {
        w.assign(m.key(*k));
    }
    w.required
        .extend(t.required.iter().map(|r| crate::runtime::Required {
            key: m.key(r.key),
            ctx: m.ctx(r.ctx),
            at: r.at,
        }));
    w.completed
        .extend(t.completed.iter().filter(|x| inr(x)).map(|&x| m.item(x)));
    w.touched
        .extend(t.touched.iter().filter(|x| inr(x)).map(|&x| m.item(x)));
    w.restructured
        .extend(t.restructured.iter().filter(|x| inr(x)).map(|&x| m.item(x)));
    w.lrev_items
        .extend(t.lrev_items.iter().filter(|x| inr(x)).map(|&x| m.item(x)));
    w.resized
        .extend(t.resized.iter().filter(|x| inr(x)).map(|&x| m.item(x)));
    w.sourced
        .extend(t.sourced.iter().filter(|x| inr(x)).map(|&x| m.item(x)));
    w.changed.extend(
        t.changed
            .iter()
            .filter(|k| inr(&k.0))
            .map(|&k| (m.key(k), cause)),
    );
    for (k, v) in &t.owned {
        w.owned
            .insert(m.item(*k), v.iter().map(|&x| m.item(x)).collect());
    }
    for (k, v) in &t.root_ctxs {
        w.root_ctxs
            .insert(m.item(*k), v.iter().map(|&c| m.ctx(c)).collect());
    }
    for (tg, i, n) in &t.obj_refs {
        w.obj_refs
            .entry(im(*tg))
            .or_default()
            .push((im(*i), n.clone()));
    }
    for (k, v) in &t.ctx_js {
        w.ctx_js.insert(m.ctx(*k), v.clone());
    }
    crate::runtime::bump_scope();
    for (k, v) in &t.ctx_files {
        w.ctx_files.insert(m.ctx(*k), v.clone());
    }
    for (k, v) in &t.signals {
        w.signals.insert(m.item(*k), v.clone());
    }
    for (k, v) in &t.ipc_decls {
        w.ipc_decls.insert(m.item(*k), v.clone());
    }
    for (k, v) in &t.behaviors {
        w.behaviors.insert(m.key(*k), m.item(*v));
    }
    for (k, v) in &t.deferred {
        w.deferred
            .insert(m.item(*k), v.iter().map(|&d| m.key(d)).collect());
    }
    for (k, a) in &t.lazy_anims {
        let mut a = a.clone();
        a.ctx = m.ctx(a.ctx);
        w.lazy_anims.insert(m.item(*k), a);
    }
    for (k, rows) in &t.deferred_rows {
        let mut rows = rows.clone();
        for r in &mut rows {
            r.ctx = m.ctx(r.ctx);
        }
        w.deferred_rows.insert(m.item(*k), rows);
    }
    w.lazy_ready.extend(t.lazy_ready.iter().map(|&k| m.item(k)));
    w.layer_ready
        .extend(t.layer_ready.iter().map(|&k| m.item(k)));
    w.state_hosts
        .extend(t.state_hosts.iter().map(|&k| m.item(k)));
    w.sync_new
        .extend(t.synchronizers.iter().map(|&k| m.item(k)));
    w.alias_pending
        .extend(t.alias_pending.iter().map(|&k| m.item(k)));
    for (g, (o, n)) in &t.groups {
        let (g, key) = (m.item(*g), (m.item(*o), n.clone()));
        w.group_of.insert(key.clone(), g);
        w.groups.insert(g, key);
    }
    for &(f, c) in &t.content {
        w.adopt_content_item(m.item(f), m.item(c));
    }
    for (owner, src) in &t.owned_templates {
        let owner = *owner;
        let nt = Template {
            object: src.object.clone(),
            ctx: m.ctx(src.ctx),
            file: src.file.clone(),
            url: src.url.clone(),
            vis: src.vis.clone(),
            owner: m.item(src.owner),
        };
        let nid = w.templates.push(nt);
        let o = m.item(owner);
        w.owned_templates.push((o, nid));
        if let Some(p) = w.items[o].props.get_mut("\x01template") {
            p.value = Value::Num(nid as f64);
        }
    }
    m
}

fn csig(m: &Map, c: &Code) -> (usize, usize) {
    (
        m.ctx(c.ctx),
        c.compiled.map_or(0, |e| e as *const _ as usize),
    )
}

/// `RI_DEBUG=plancheck`: diffs between plan `a` replayed at `b`'s ids and `b`, an ordinary create.
pub fn check(a: &Plan, ta: &Tables, b: &Plan, tb: &Tables) -> Vec<String> {
    let mut d = Vec::new();
    if a.items.len() != b.items.len() || a.ctxs.len() != b.ctxs.len() {
        d.push(format!(
            "size items {}/{} ctxs {}/{}",
            a.items.len(),
            b.items.len(),
            a.ctxs.len(),
            b.ctxs.len()
        ));
        return d;
    }
    let m = Map {
        n0: a.n0,
        n1: a.n0 + a.items.len(),
        m0: b.n0,
        c0: a.c0,
        c1: a.c0 + a.ctxs.len(),
        k0: b.c0,
    };
    let id = Map {
        n0: 0,
        n1: 0,
        m0: 0,
        c0: 0,
        c1: 0,
        k0: 0,
    };
    let ext = (a.items[0].1, b.items[0].1);
    let im = |x: usize| m.item_ext(x, ext);
    let rb: Vec<(usize, String)> = b
        .rebuild
        .iter()
        .map(|r| match r {
            Rebuild::Function { item, name, .. }
            | Rebuild::Signal { item, name }
            | Rebuild::Enum { item, name } => (*item, name.clone()),
            Rebuild::Focus { item } => (*item, "focus".into()),
            Rebuild::Scripts { ctx, .. } => (*ctx, "scripts".into()),
        })
        .collect();
    for (j, (x, y)) in a.items.iter().zip(&b.items).enumerate() {
        let i = b.n0 + j;
        let ty = y.0.first().map(|t| t.as_str()).unwrap_or("");
        if x.0 != y.0 {
            d.push(format!("{i} {ty} types"));
        }
        if j > 0 && x.1.map(im) != y.1 {
            d.push(format!("{i} {ty} parent"));
        }
        if x.2.iter().map(|&c| m.item(c)).collect::<Vec<_>>() != y.2 {
            d.push(format!("{i} {ty} children"));
        }
        if x.5 != y.5
            || x.6.0.map(|l| l.as_ptr()) != y.6.0.map(|l| l.as_ptr())
            || x.6.1 != y.6.1
            || x.8 != y.8
        {
            d.push(format!("{i} {ty} proxy/layout"));
        }
        let hs = |h: &HashMap<Sym, Vec<Code>>, m: &Map| {
            let mut v: Vec<(String, Vec<(usize, usize)>)> = h
                .iter()
                .map(|(k, c)| (k.to_string(), c.iter().map(|c| csig(m, c)).collect()))
                .collect();
            v.sort();
            v
        };
        if hs(&x.4, &m) != hs(&y.4, &id) {
            d.push(format!("{i} {ty} handlers"));
        }
        let mut ka: Vec<&str> = x.3.keys().map(|k| k.as_str()).collect();
        let mut kb: Vec<&str> = y.3.keys().map(|k| k.as_str()).collect();
        ka.sort_unstable();
        kb.sort_unstable();
        if ka != kb {
            d.push(format!(
                "{i} {ty} keys {:?} / {:?}",
                ka.iter().filter(|k| !kb.contains(k)).collect::<Vec<_>>(),
                kb.iter().filter(|k| !ka.contains(k)).collect::<Vec<_>>()
            ));
            continue;
        }
        for k in kb {
            let (p, q) = (map_prop(&m, x.3.get(k).unwrap()), y.3.get(k).unwrap());
            let mut w = Vec::new();
            if p.ty != q.ty {
                w.push("ty");
            }
            if p.binding.as_ref().map(|c| csig(&id, c)) != q.binding.as_ref().map(|c| csig(&id, c))
                || p.base.as_ref().map(|c| csig(&id, c)) != q.base.as_ref().map(|c| csig(&id, c))
            {
                w.push("binding");
            }
            if p.deps != q.deps {
                w.push("deps");
            }
            if p.alias != q.alias {
                w.push("alias");
            }
            match (&p.value, &q.value) {
                (Value::Obj(_), Value::Obj(_)) => {}
                (Value::Obj(_), _) | (_, Value::Obj(_)) => w.push("value-kind"),
                (u, v) => {
                    let rebuilt = k == "\x01template" || rb.iter().any(|r| r.0 == i && r.1 == k);
                    if !rebuilt && format!("{u:?}") != format!("{v:?}") {
                        w.push("value");
                    }
                }
            }
            if !w.is_empty() {
                d.push(format!(
                    "{i} {ty}.{k} {}: {:?} / {:?}",
                    w.join(","),
                    p.value,
                    q.value
                ));
            }
        }
    }
    for (j, (x, y)) in a.ctxs.iter().zip(&b.ctxs).enumerate() {
        let mut ia: Vec<(String, usize)> =
            x.0.iter().map(|(k, v)| (k.clone(), m.item(*v))).collect();
        let mut ib = y.0.clone();
        ia.sort();
        ib.sort();
        let ra = if x.1 == usize::MAX { x.1 } else { m.item(x.1) };
        // The instance's own context hangs where its creation put it.
        let pa = j == 0 || x.2.map(|c| m.ctx(c)) == y.2;
        if ia != ib || ra != y.1 || !pa || x.3 != y.3 {
            d.push(format!("ctx {} ids/root/parent", b.c0 + j));
        }
    }
    if a.rebuild.len() != b.rebuild.len() || a.rebind.len() != b.rebind.len() {
        d.push("rebuild/rebind count".into());
    }
    let inr = |x: &usize| (b.n0..b.n0 + b.items.len()).contains(x);
    let inra = |x: &usize| (m.n0..m.n1).contains(x);
    macro_rules! cmp {
        ($name:literal, $a:expr, $b:expr) => {{
            let (mut a, mut b) = ($a, $b);
            a.sort();
            b.sort();
            if a != b {
                d.push(format!("table {} {:?} / {:?}", $name, a.len(), b.len()));
            }
        }};
    }
    let ks = |v: &[DKey]| {
        v.iter()
            .map(|k| (k.0, k.1.as_str().to_string()))
            .collect::<Vec<_>>()
    };
    cmp!(
        "dirty",
        ks(&ta
            .dirty
            .iter()
            .filter(|k| inra(&k.0.0))
            .map(|&(k, _)| m.key(k))
            .collect::<Vec<_>>()),
        ks(&tb
            .dirty
            .iter()
            .filter(|k| inr(&k.0.0))
            .map(|&(k, _)| k)
            .collect::<Vec<_>>())
    );
    cmp!(
        "fresh_bind",
        ta.fresh_bind
            .iter()
            .map(|(k, v)| (m.item(k.0), k.1.as_str().to_string(), *v))
            .collect::<Vec<_>>(),
        tb.fresh_bind
            .iter()
            .map(|(k, v)| (k.0, k.1.as_str().to_string(), *v))
            .collect::<Vec<_>>()
    );
    cmp!(
        "assigned",
        ks(&ta.assigned.iter().map(|&k| m.key(k)).collect::<Vec<_>>()),
        ks(&tb.assigned)
    );
    let sl = |v: &[usize], f: &dyn Fn(&usize) -> bool, mm: &dyn Fn(usize) -> usize| {
        v.iter()
            .filter(|x| f(x))
            .map(|&x| mm(x))
            .collect::<Vec<_>>()
    };
    let mi = |x: usize| m.item(x);
    let idm = |x: usize| x;
    cmp!(
        "completed",
        sl(&ta.completed, &inra, &mi),
        sl(&tb.completed, &inr, &idm)
    );
    cmp!(
        "touched",
        sl(&ta.touched, &inra, &mi),
        sl(&tb.touched, &inr, &idm)
    );
    cmp!(
        "restructured",
        sl(&ta.restructured, &inra, &mi),
        sl(&tb.restructured, &inr, &idm)
    );
    cmp!(
        "lrev_items",
        sl(&ta.lrev_items, &inra, &mi),
        sl(&tb.lrev_items, &inr, &idm)
    );
    cmp!(
        "resized",
        sl(&ta.resized, &inra, &mi),
        sl(&tb.resized, &inr, &idm)
    );
    cmp!(
        "sourced",
        sl(&ta.sourced, &inra, &mi),
        sl(&tb.sourced, &inr, &idm)
    );
    cmp!(
        "changed",
        ks(&ta
            .changed
            .iter()
            .filter(|k| inra(&k.0))
            .map(|&k| m.key(k))
            .collect::<Vec<_>>()),
        ks(&tb
            .changed
            .iter()
            .filter(|k| inr(&k.0))
            .copied()
            .collect::<Vec<_>>())
    );
    cmp!(
        "owned",
        ta.owned
            .iter()
            .map(|(k, v)| (m.item(*k), v.iter().map(|&x| m.item(x)).collect::<Vec<_>>()))
            .collect::<Vec<_>>(),
        tb.owned.clone()
    );
    cmp!(
        "root_ctxs",
        ta.root_ctxs
            .iter()
            .map(|(k, v)| (m.item(*k), v.iter().map(|&c| m.ctx(c)).collect::<Vec<_>>()))
            .collect::<Vec<_>>(),
        tb.root_ctxs.clone()
    );
    cmp!(
        "obj_refs",
        ta.obj_refs
            .iter()
            .map(|(t, i, n)| (im(*t), im(*i), n.clone()))
            .collect::<Vec<_>>(),
        tb.obj_refs.clone()
    );
    cmp!(
        "ctx_js",
        ta.ctx_js
            .iter()
            .map(|(k, v)| (m.ctx(*k), v.len()))
            .collect::<Vec<_>>(),
        tb.ctx_js
            .iter()
            .map(|(k, v)| (*k, v.len()))
            .collect::<Vec<_>>()
    );
    cmp!(
        "ctx_files",
        ta.ctx_files
            .iter()
            .map(|(k, v)| (m.ctx(*k), v.clone()))
            .collect::<Vec<_>>(),
        tb.ctx_files.clone()
    );
    cmp!(
        "signals",
        ta.signals
            .iter()
            .map(|(k, v)| (m.item(*k), v.len()))
            .collect::<Vec<_>>(),
        tb.signals
            .iter()
            .map(|(k, v)| (*k, v.len()))
            .collect::<Vec<_>>()
    );
    cmp!(
        "behaviors",
        ta.behaviors
            .iter()
            .map(|(k, v)| (m.item(k.0), k.1.as_str().to_string(), m.item(*v)))
            .collect::<Vec<_>>(),
        tb.behaviors
            .iter()
            .map(|(k, v)| (k.0, k.1.as_str().to_string(), *v))
            .collect::<Vec<_>>()
    );
    cmp!(
        "deferred",
        ta.deferred
            .iter()
            .map(|(k, v)| (
                m.item(*k),
                ks(&v.iter().map(|&x| m.key(x)).collect::<Vec<_>>())
            ))
            .collect::<Vec<_>>(),
        tb.deferred
            .iter()
            .map(|(k, v)| (*k, ks(v)))
            .collect::<Vec<_>>()
    );
    cmp!(
        "lazy_anims",
        ta.lazy_anims
            .iter()
            .map(|(k, a)| (m.item(*k), m.ctx(a.ctx)))
            .collect::<Vec<_>>(),
        tb.lazy_anims
            .iter()
            .map(|(k, a)| (*k, a.ctx))
            .collect::<Vec<_>>()
    );
    cmp!(
        "lazy_ready",
        ta.lazy_ready.iter().map(|&k| m.item(k)).collect::<Vec<_>>(),
        tb.lazy_ready.clone()
    );
    cmp!(
        "layer_ready",
        ta.layer_ready
            .iter()
            .map(|&k| m.item(k))
            .collect::<Vec<_>>(),
        tb.layer_ready.clone()
    );
    cmp!(
        "state_hosts",
        ta.state_hosts
            .iter()
            .map(|&k| m.item(k))
            .collect::<Vec<_>>(),
        tb.state_hosts.clone()
    );
    cmp!(
        "synchronizers",
        ta.synchronizers
            .iter()
            .map(|&k| m.item(k))
            .collect::<Vec<_>>(),
        tb.synchronizers.clone()
    );
    cmp!(
        "alias_pending",
        ta.alias_pending
            .iter()
            .map(|&k| m.item(k))
            .collect::<Vec<_>>(),
        tb.alias_pending.clone()
    );
    cmp!(
        "groups",
        ta.groups
            .iter()
            .map(|(g, (o, n))| (m.item(*g), m.item(*o), n.clone()))
            .collect::<Vec<_>>(),
        tb.groups
            .iter()
            .map(|(g, (o, n))| (*g, *o, n.clone()))
            .collect::<Vec<_>>()
    );
    cmp!(
        "content",
        ta.content
            .iter()
            .map(|&(f, c)| (m.item(f), m.item(c)))
            .collect::<Vec<_>>(),
        tb.content.clone()
    );
    cmp!(
        "owned_templates",
        ta.owned_templates
            .iter()
            .map(|(o, _)| m.item(*o))
            .collect::<Vec<_>>(),
        tb.owned_templates
            .iter()
            .map(|(o, _)| *o)
            .collect::<Vec<_>>()
    );
    d
}

/// Post-settle state of the bindings whose final value and dependencies cannot differ
/// between instances: they only read other such bindings of the same instance.
#[derive(Default)]
pub struct Post {
    pub props: Vec<(DKey, Value, Vec<DKey>)>,
    pub deps: Vec<(DKey, Vec<DKey>)>,
    pub text: Vec<usize>,
    /// Singleton-tree keys read by copied bindings, with their value at capture.
    pub ext: Vec<(DKey, Option<Value>, Vec<u32>)>,
    /// For each copied binding, the copied bindings that read it.
    pub rdeps: Vec<Vec<u32>>,
}

pub fn post_capture(w: &World, n0: usize, n1: usize, srcs: &[crate::runtime::Sym]) -> Post {
    use std::collections::HashSet;
    let inr = |j: usize| (n0..n1).contains(&j);
    let mut taint: HashSet<DKey> = srcs.iter().map(|s| (n0, *s)).collect();
    let mut hot: HashSet<usize> = HashSet::new();
    for i in n0..n1 {
        let it = &w.items[i];
        if it.proxy {
            hot.insert(i);
        }
        if it.types.iter().any(|t| t == "Connections") {
            hot.extend(it.parent);
            if let Some(Value::Item(t)) = it.props.get("target").map(|p| &p.value) {
                hot.insert(*t);
            }
        }
    }
    let singles: HashSet<usize> = w.singletons.values().copied().collect();
    let mut gmemo: HashMap<usize, bool> = HashMap::default();
    let mut global = |mut i: usize| {
        let i0 = i;
        if let Some(&g) = gmemo.get(&i0) {
            return g;
        }
        let g = loop {
            if singles.contains(&i) {
                break true;
            }
            match w.items[i].parent {
                Some(p) => i = p,
                None => break false,
            }
        };
        gmemo.insert(i0, g);
        g
    };
    let mut bound = Vec::new();
    for i in n0..n1 {
        let it = &w.items[i];
        for (k, p) in it.props.iter() {
            if p.binding.is_none() {
                continue;
            }
            let key = (i, *k);
            bound.push(key);
            let name = k.as_str();
            let bad = hot.contains(&i)
                || p.alias.is_some()
                || p.base.is_some()
                || name.contains('.')
                || name.starts_with('\x01')
                || matches!(
                    name,
                    "focus"
                        | "activeFocus"
                        | "active"
                        | "enabled"
                        | "children"
                        | "data"
                        | "contentChildren"
                )
                // QQuickImageBase::setSourceSize keeps the size asked for apart from the
                // property.
                || (name == "sourceSize" && w.image_base(i))
                // A LazyLoader's setters, like `active`.
                || (matches!(name, "loading" | "activeAsync")
                    && it.types.iter().any(|t| t == "LazyLoader"))
                || it
                    .handlers
                    .contains_key(&crate::runtime::changed_handler(name))
                || match &p.value {
                    Value::Item(_) => true,
                    Value::Obj(o) => {
                        !matches!(o.borrow().call, Some(crate::eval::Callable::Native(..)))
                    }
                    _ => false,
                }
                || p.deps
                    .iter()
                    .any(|d| !inr(d.0) && (w.dead.contains(&d.0) || !global(d.0)))
                // A Text is laid out in its componentComplete(), after this capture: what
                // read its size has yet to follow it.
                || p.deps.iter().any(|d| {
                    w.text_unfinished.contains_key(&d.0)
                        && crate::runtime::text_metric(d.1.as_str())
                });
            if bad {
                taint.insert(key);
            }
        }
    }
    loop {
        let mut grew = false;
        for k in &bound {
            if !taint.contains(k)
                && w.items[k.0]
                    .props
                    .get_sym(k.1)
                    .is_some_and(|p| p.deps.iter().any(|d| taint.contains(d)))
            {
                taint.insert(*k);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let mut post = Post::default();
    let mut seen: HashSet<DKey> = HashSet::new();
    let mut text: HashSet<usize> = HashSet::new();
    for k in bound.into_iter().filter(|k| !taint.contains(k)) {
        let it = &w.items[k.0];
        let p = it.props.get_sym(k.1).unwrap();
        for d in &p.deps {
            if seen.insert(*d) {
                let ds: Vec<DKey> = w
                    .dependents
                    .get(d)
                    .map(|s| s.connected().filter(|x| inr(x.0)).copied().collect())
                    .unwrap_or_default();
                post.deps.push((*d, ds));
            }
        }
        if crate::runtime::text_input(k.1.as_str())
            && it.types.iter().any(|t| crate::runtime::measured_text(t))
        {
            text.insert(k.0);
        }
        post.props.push((k, p.value.clone(), p.deps.to_vec()));
    }
    // Dependents of a key keep their order; drop tainted ones, they register on evaluation.
    let copied: HashSet<DKey> = post.props.iter().map(|p| p.0).collect();
    let idx: HashMap<DKey, u32> = post
        .props
        .iter()
        .enumerate()
        .map(|(j, p)| (p.0, j as u32))
        .collect();
    let mut eidx: HashMap<DKey, usize> = HashMap::default();
    post.rdeps = vec![Vec::new(); post.props.len()];
    for (j, (_, _, deps)) in post.props.iter().enumerate() {
        for d in deps {
            if let Some(&u) = idx.get(d) {
                post.rdeps[u as usize].push(j as u32);
            } else if !inr(d.0) {
                let e = *eidx.entry(*d).or_insert_with(|| {
                    let v = w.items[d.0].props.get_sym(d.1).map(|p| p.value.clone());
                    post.ext.push((*d, v, Vec::new()));
                    post.ext.len() - 1
                });
                post.ext[e].2.push(j as u32);
            }
        }
    }
    for (_, ds) in post.deps.iter_mut() {
        ds.retain(|x| copied.contains(x));
    }
    post.text = text.into_iter().collect();
    post.text.sort_unstable();
    post
}

pub fn post_apply(w: &mut World, m: &Map, post: &Post) {
    let mut skip: rustc_hash::FxHashSet<DKey> = Default::default();
    skip.reserve(post.props.len());
    // A singleton value read by a copied binding changed since capture: that binding and
    // the copied ones reading it evaluate normally.
    let mut bad = vec![false; post.props.len()];
    let mut todo: Vec<u32> = Vec::new();
    for (d, v0, users) in &post.ext {
        let cur = (!w.dead.contains(&d.0))
            .then(|| w.items[d.0].props.get_sym(d.1).map(|p| &p.value))
            .flatten();
        let same = match (v0, cur) {
            (None, None) => !w.dead.contains(&d.0),
            (Some(a), Some(b)) => crate::builtins::same_value(a, b),
            _ => false,
        };
        if !same {
            todo.extend(users);
        }
    }
    while let Some(j) = todo.pop() {
        if !std::mem::replace(&mut bad[j as usize], true) {
            todo.extend(&post.rdeps[j as usize]);
        }
    }
    for ((k, v, deps), _) in post.props.iter().zip(&bad).filter(|x| !*x.1) {
        let nk = m.key(*k);
        if let Some(p) = w.items[nk.0].props.get_mut_sym(k.1) {
            p.value = map_value(m, v);
            p.deps = deps.iter().map(|d| m.key(*d)).collect();
            skip.insert(nk);
        }
    }
    for (d, ds) in &post.deps {
        let d = m.key(*d);
        let newest_first = w.bindable_dep(&d);
        for x in ds {
            let x = m.key(*x);
            if skip.contains(&x) {
                let stamp = w.conn_stamp();
                w.dependents
                    .entry(d)
                    .or_default()
                    .insert(x, stamp, newest_first);
            }
        }
    }
    if w.text_measure.is_some() {
        for &i in &post.text {
            w.text_dirty.insert(m.item(i));
        }
    }
    w.fresh_bind.retain(|k, _| !skip.contains(k));
    for k in &skip {
        w.dirty.remove(k);
    }
    w.revision += 1;
}
