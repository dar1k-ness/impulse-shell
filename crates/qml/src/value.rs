//! Dynamic JS value model for binding evaluation (ECMAScript conversion semantics).
use std::cell::RefCell;
use std::rc::{Rc, Weak};

pub type Obj = Rc<RefCell<JsObject>>;

/// Live-instance tally (leak diagnostics): index 0 = JS frames, 1 = JS objects.
pub static LIVE: [std::sync::atomic::AtomicIsize; 2] = [
    std::sync::atomic::AtomicIsize::new(0),
    std::sync::atomic::AtomicIsize::new(0),
];

thread_local! {
    /// Every JS object (weakly), for the cycle collector (`eval::collect_step`).
    pub static HEAP: RefCell<Heap> = const { RefCell::new(Heap::new()) };
}

/// The entries `Heap::objs` holds before freed ones are dropped from it for the first time.
/// A freed object's entry pins its allocation, so a loop of temporaries holds this many dead
/// objects at most; measured against 16384 and 65536, the loop and the shell's start take
/// the same time.
const LIST_MIN: usize = 4096;

/// The cycle collector's view of the JS heap: every object in allocation order, split into
/// generations (as CPython's `gc`). `objs[young..]` were allocated after the collector last
/// examined the young generation; `objs[..young]` are old. An old-generation pass ("sweep")
/// walks `objs[..sweep_end]` from `cursor` over several increments; an object whose
/// `JsObject::gc` stamp equals `epoch` was already examined in the current sweep (or, with
/// no sweep running, since the last one began).
#[derive(Debug)]
pub struct Heap {
    pub objs: Vec<std::rc::Weak<RefCell<JsObject>>>,
    pub young: usize,
    pub cursor: usize,
    /// The sweep compacts `objs[..cursor]` into `objs[..kept]` as it walks (freed objects'
    /// entries pin their allocations); the gap is closed when it ends.
    pub kept: usize,
    pub sweep_end: usize,
    pub sweeping: bool,
    pub epoch: u32,
    /// Young survivors moved to the old generation since the last sweep began.
    pub promoted: usize,
    /// Old objects found live by the last finished sweep.
    pub old_live: usize,
    /// Survivors counted so far in the running sweep.
    pub sweep_live: usize,
    cap: usize,
}

impl Heap {
    const fn new() -> Heap {
        Heap {
            objs: Vec::new(),
            young: 0,
            cursor: 0,
            kept: 0,
            sweep_end: 0,
            sweeping: false,
            epoch: 1,
            promoted: 0,
            old_live: 0,
            sweep_live: 0,
            cap: LIST_MIN,
        }
    }

    /// Drop the entries of freed objects (each pins its allocation), keeping the positions
    /// of `young`, `cursor`, `kept` and `sweep_end` on the same objects.
    pub fn compact(&mut self) {
        let (young, cursor, kept_at, end) = (self.young, self.cursor, self.kept, self.sweep_end);
        let (mut i, mut kept) = (0, 0);
        self.objs.retain(|w| {
            let keep = w.strong_count() > 0;
            if i < young {
                self.young -= !keep as usize;
            }
            if i < cursor {
                self.cursor -= !keep as usize;
            }
            if i < kept_at {
                self.kept -= !keep as usize;
            }
            if i < end {
                self.sweep_end -= !keep as usize;
            }
            i += 1;
            kept += keep as usize;
            keep
        });
        self.cap = (kept * 2).max(LIST_MIN);
    }
}

pub fn reg(o: Obj) -> Obj {
    HEAP.with(|h| {
        let mut h = h.borrow_mut();
        h.objs.push(Rc::downgrade(&o));
        // Compaction is amortized: the threshold doubles with the live count.
        if h.objs.len() >= h.cap {
            h.compact();
        }
    });
    o
}

/// Every live JS object (diagnostics).
pub fn live_objects() -> Vec<Obj> {
    HEAP.with(|h| h.borrow().objs.iter().filter_map(|w| w.upgrade()).collect())
}

#[derive(Debug)]
pub struct Tally<const K: usize>;
impl<const K: usize> Default for Tally<K> {
    fn default() -> Self {
        LIVE[K].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Tally
    }
}
impl<const K: usize> Drop for Tally<K> {
    fn drop(&mut self) {
        LIVE[K].fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
pub struct JsObject {
    /// Insertion-ordered own properties.
    pub keys: Vec<Rc<str>>,
    pub props: rustc_hash::FxHashMap<Rc<str>, Value>,
    pub array: Option<Vec<Value>>,
    /// Indices of `array`'s holes, elements that do not exist (`Array(n)`, `[1, , 2]`, growth
    /// through `length` or a far index; ECMA-262 §10.4.2). A hole's slot holds `undefined`,
    /// so reads need no check; `HasProperty`-based code (`in`, `forEach`, `Object.keys`, ...)
    /// skips them. `None` when the array is dense.
    pub holes: Option<Box<Holes>>,
    /// How many elements an array has after `array`'s last one: its `length` is
    /// `array.len() + tail`. They take no storage, so `new Array(4294967295)` and
    /// `a[4294967294] = 1` allocate nothing: each is a hole, unless an ordinary property
    /// holds it (as [`JsObject::make_slow_element`] does). `holes` is `Some` while there is
    /// a tail, so the code that reads `array` alone when it has no holes leaves such an
    /// array to the code that reads properties.
    pub tail: usize,
    pub proto: Option<Obj>,
    /// `[[Prototype]]` is null (`Object.create(null)`): without it a `None` `proto` is
    /// %Object.prototype% ([`crate::eval::Interp::proto_of`]).
    pub null_proto: bool,
    pub call: Option<crate::eval::Callable>,
    /// QML sequence property this array was read from: in-place mutation notifies it.
    pub owner: Option<(usize, Rc<str>)>,
    /// Own accessor properties (ECMA-262 §6.1.7.1), by name; an enumerable one is in `keys`.
    pub accessors: Option<Box<rustc_hash::FxHashMap<Rc<str>, Accessor>>>,
    /// Own properties whose attributes (ECMA-262 §6.1.7.1) are not the defaults of a property
    /// made by assignment: [`READONLY`] and/or [`PERMANENT`] bits, by name (array elements by
    /// their index string). Enumerability is membership of `keys`.
    pub attrs: Option<Box<rustc_hash::FxHashMap<Rc<str>, u8>>>,
    /// `[[Extensible]]` is false (`Object.preventExtensions`, `seal`, `freeze`).
    pub non_extensible: bool,
    /// A built-in object of a QML engine ([`crate::builtins::install`] locks them, as V4's
    /// `ExecutionEngine::lockObject`): its prototype cannot be changed.
    pub locked: bool,
    /// A built-in iterator prototype (%ArrayIteratorPrototype% and the like) whose own `next`
    /// is still the built-in one. Any write, definition or deletion of its `next` clears it
    /// for good: for-of and spread iterate built-in iterables natively only while it is set.
    pub builtin_next: bool,
    /// A built-in function that is a constructor (ECMA-262 §10.3: only those the spec makes
    /// one have [[Construct]]); see [`crate::proxy::is_constructor`]. Also set on QV4's
    /// %TypedArray%, an object of class Function that is not callable.
    pub builtin_ctor: bool,
    /// An array QV4 gave element storage (`arrayData`) that may hold nothing by now: one
    /// made by `Array(n)` or filled by a built-in, or emptied by `pop`, `shift` or `length`.
    /// `[]` and an empty `slice` have none, and `freeze` and `seal` give attribute bytes
    /// only to an array that has storage (see [`JsObject::ensure_bytes`]).
    pub cells: bool,
    /// Internal slots of a built-in object (never visible as properties).
    pub internal: Internal,
    /// Rust data behind a natively implemented object (e.g. a Material Shapes morph).
    pub native: Option<Rc<dyn std::any::Any>>,
    /// Symbol-keyed properties and the WeakMap/WeakSet entries keyed by this object.
    pub ext: Option<Box<Ext>>,
    /// Cycle-collector epoch this object was last examined in (see `Heap`).
    pub gc: std::cell::Cell<u32>,
    pub _n: Tally<1>,
}

bits::bit_flags! {
/// [[Writable]] is false (a data property).
pub const READONLY: u8 = 1;
/// [[Configurable]] is false.
pub const PERMANENT: u8 = 2;
/// The function's built-in own `length`/`name` was deleted.
pub const ABSENT: u8 = 4;
/// An array element QV4 holds with a zero attribute byte: stored past the cells the array
/// had when it got its attribute bytes (`SimpleArrayData::putArray`, `ArrayData::insert`
/// write no byte). It is read by index and listed by `getOwnPropertyNames`, but is no own
/// property to `in`, `hasOwnProperty`, a descriptor or `Object.keys`, and cannot be deleted.
pub const HIDDEN: u8 = 8;
/// An array position past [`Holes::known`] that has an attribute byte all the same
/// (`defineProperty` wrote it): the value stored there is an ordinary property.
pub const BYTE: u8 = 16;
}

impl JsObject {
    /// The non-default attribute bits of own property `k`.
    pub fn attr(&self, k: &str) -> u8 {
        self.attrs
            .as_ref()
            .and_then(|m| m.get(k))
            .copied()
            .unwrap_or(0)
    }

    /// Makes own property `k` enumerable or not (listed in `keys` or not), keeping its place
    /// in creation order (OrdinaryOwnPropertyKeys, ECMA-262 §10.1.11.1).
    pub fn set_enumerable(&mut self, k: &Rc<str>, enumerable: bool) {
        let listed = self.keys.contains(k);
        if crate::eval::array_index(k).is_some() {
            if enumerable && !listed {
                crate::eval::push_key(&mut self.keys, k.clone());
            } else if !enumerable && listed {
                self.keys.retain(|x| x != k);
            }
            return;
        }
        if !enumerable {
            self.record_order(k);
            if listed {
                self.keys.retain(|x| x != k);
            }
        } else if !listed {
            if self.ext.as_ref().is_none_or(|e| e.order.is_none()) {
                self.keys.push(k.clone());
                return;
            }
            self.record_order(k);
            let order = self.ext.as_ref().and_then(|e| e.order.as_ref()).unwrap();
            let listed: rustc_hash::FxHashSet<&Rc<str>> =
                self.keys.iter().chain(std::iter::once(k)).collect();
            let mut keys: Vec<Rc<str>> = (self.keys.iter())
                .filter(|x| crate::eval::array_index(x).is_some())
                .cloned()
                .collect();
            keys.extend(order.iter().filter(|x| listed.contains(x)).cloned());
            self.keys = keys;
        }
    }

    /// Records the creation order so far, then `k` after it if it is new (array indices are
    /// ordered by value, not here).
    pub fn record_order(&mut self, k: &Rc<str>) {
        let JsObject { keys, ext, .. } = self;
        let order = ext
            .get_or_insert_default()
            .order
            .get_or_insert_with(Vec::new);
        let known: rustc_hash::FxHashSet<Rc<str>> = order.iter().cloned().collect();
        order.extend(
            (keys.iter())
                .filter(|x| crate::eval::array_index(x).is_none() && !known.contains(*x))
                .cloned(),
        );
        if crate::eval::array_index(k).is_none() && !order.contains(k) {
            order.push(k.clone());
        }
    }

    /// Sets the creation order of the own named keys to `order`, then the keys it lacks in
    /// the order they had.
    pub fn set_order(&mut self, order: &[&str]) {
        let mut all: Vec<Rc<str>> = order.iter().map(|k| Rc::from(*k)).collect();
        let known: rustc_hash::FxHashSet<Rc<str>> = all.iter().cloned().collect();
        all.extend(self.named_keys().into_iter().filter(|k| !known.contains(k)));
        let ext = self.ext.get_or_insert_default();
        ext.order = Some(all);
        ext.order_leads = true;
    }

    /// Records `k` first in creation order: a property made with the object, before any
    /// other named key.
    pub fn record_order_first(&mut self, k: &Rc<str>) {
        self.record_order(k);
        let order = self.ext.as_mut().and_then(|e| e.order.as_mut()).unwrap();
        if let Some(i) = order.iter().position(|x| x == k) {
            let k = order.remove(i);
            order.insert(0, k);
        }
    }

    /// Drops deleted own property `k` from the creation order.
    pub fn forget_order(&mut self, k: &str) {
        if let Some(order) = self.ext.as_mut().and_then(|e| e.order.as_mut()) {
            order.retain(|x| **x != *k);
        }
    }

    /// The own named string keys in creation order: enumerable ones and those made
    /// non-enumerable by script (the engine's own non-enumerable keys are not ordered).
    pub fn named_keys(&self) -> Vec<Rc<str>> {
        let named = self
            .keys
            .iter()
            .filter(|k| crate::eval::array_index(k).is_none());
        let Some(order) = self.ext.as_ref().and_then(|e| e.order.as_ref()) else {
            return named.cloned().collect();
        };
        let exists = |k: &str| {
            self.props.contains_key(k) || self.accessors.as_ref().is_some_and(|m| m.contains_key(k))
        };
        let known: rustc_hash::FxHashSet<&Rc<str>> = order.iter().collect();
        let mut out: Vec<Rc<str>> = order.iter().filter(|k| exists(k)).cloned().collect();
        out.extend(named.filter(|k| !known.contains(k)).cloned());
        out
    }

    /// Replaces the non-default attribute bits of own property `k`.
    pub fn set_attr(&mut self, k: &str, bits: u8) {
        // A deleted built-in `length` or `name` stays deleted when the key is redefined.
        let bits = bits | (self.attr(k) & ABSENT);
        if bits == 0 {
            if let Some(m) = &mut self.attrs {
                m.remove(k);
            }
            return;
        }
        let m = self.attrs.get_or_insert_with(Default::default);
        match m.get_mut(k) {
            Some(b) => *b = bits,
            None => {
                m.insert(k.into(), bits);
            }
        }
    }

    /// A Proxy exotic object (ECMA-262 §10.5), revoked or not.
    pub fn is_proxy(&self) -> bool {
        matches!(self.internal, Internal::Proxy(_))
    }

    /// Some own property may be read-only or non-configurable, or none may be added: the
    /// fast paths that write without looking at attributes must take the checked one.
    pub fn restricted(&self) -> bool {
        self.non_extensible
            || self.attrs.as_ref().is_some_and(|m| !m.is_empty())
            || (self.holes.as_ref()).is_some_and(|h| h.known.is_some() || h.sparse)
    }
}

/// A Symbol value (ECMA-262 §6.1.5.1): every `Rc` is a distinct symbol.
#[derive(Debug)]
pub struct JsSymbol {
    /// [[Description]].
    pub description: Option<Rc<str>>,
    /// Its key in the GlobalSymbolRegistry (`Symbol.for`), which is `description`.
    pub registered: bool,
}

impl JsSymbol {
    pub fn new(description: Option<Rc<str>>) -> Rc<JsSymbol> {
        Rc::new(JsSymbol {
            description,
            registered: false,
        })
    }

    /// SymbolDescriptiveString (ECMA-262 §20.4.3.3.1).
    pub fn descriptive(&self) -> String {
        format!("Symbol({})", self.description.as_deref().unwrap_or(""))
    }
}

/// The well-known symbols (ECMA-262 §6.1.5.1) V4 of Qt 6.12 has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WellKnown {
    HasInstance,
    IsConcatSpreadable,
    Iterator,
    Match,
    MatchAll,
    Replace,
    Search,
    Species,
    Split,
    ToPrimitive,
    ToStringTag,
    Unscopables,
}

impl WellKnown {
    pub const ALL: [WellKnown; 12] = [
        WellKnown::HasInstance,
        WellKnown::IsConcatSpreadable,
        WellKnown::Iterator,
        WellKnown::Match,
        WellKnown::MatchAll,
        WellKnown::Replace,
        WellKnown::Search,
        WellKnown::Species,
        WellKnown::Split,
        WellKnown::ToPrimitive,
        WellKnown::ToStringTag,
        WellKnown::Unscopables,
    ];

    /// The property of the `Symbol` constructor holding it.
    pub fn name(self) -> &'static str {
        match self {
            WellKnown::HasInstance => "hasInstance",
            WellKnown::IsConcatSpreadable => "isConcatSpreadable",
            WellKnown::Iterator => "iterator",
            WellKnown::Match => "match",
            WellKnown::MatchAll => "matchAll",
            WellKnown::Replace => "replace",
            WellKnown::Search => "search",
            WellKnown::Species => "species",
            WellKnown::Split => "split",
            WellKnown::ToPrimitive => "toPrimitive",
            WellKnown::ToStringTag => "toStringTag",
            WellKnown::Unscopables => "unscopables",
        }
    }

    /// The symbol, shared by every realm of the thread (ECMA-262 §6.1.5.1).
    pub fn sym(self) -> Rc<JsSymbol> {
        thread_local! {
            static SYMS: Vec<Rc<JsSymbol>> = WellKnown::ALL
                .iter()
                .map(|w| JsSymbol::new(Some(format!("Symbol.{}", w.name()).into())))
                .collect();
        }
        SYMS.with(|s| s[self as usize].clone())
    }

    pub fn is(self, s: &Rc<JsSymbol>) -> bool {
        Rc::ptr_eq(s, &self.sym())
    }
}

/// `Symbol.for(key)` (ECMA-262 §20.4.2.2): the registry's symbol for `key`.
pub fn symbol_for(key: &str) -> Rc<JsSymbol> {
    thread_local! {
        static REGISTRY: RefCell<rustc_hash::FxHashMap<Rc<str>, Rc<JsSymbol>>> =
            RefCell::new(Default::default());
    }
    REGISTRY.with(|r| {
        r.borrow_mut()
            .entry(Rc::from(key))
            .or_insert_with(|| {
                Rc::new(JsSymbol {
                    description: Some(Rc::from(key)),
                    registered: true,
                })
            })
            .clone()
    })
}

/// What an object holds besides string-keyed properties (boxed: most objects have neither).
#[derive(Debug, Default)]
pub struct Ext {
    /// Own symbol-keyed properties, in creation order.
    pub syms: Vec<SymProp>,
    /// The entries of WeakMaps/WeakSets with this object as key: the collection (identified
    /// by its address, which [`WeakColl::id`] keeps) and the value. Held by the key, so the
    /// cycle collector sees an entry as an edge from the key only, and a collection never
    /// keeps its keys alive (ECMA-262 §24.3, §9.10.3).
    pub weak: Vec<(Weak<RefCell<JsObject>>, Value)>,
    /// The creation order of the own named (non-index) string keys, enumerable or not, once a
    /// key has been made non-enumerable by script; until then `JsObject::keys` is that order.
    /// Keys listed in `keys` but missing here were created after the last key recorded here.
    pub order: Option<Vec<Rc<str>>>,
    /// `order` also places the keys an object is made with (a function's `prototype`,
    /// `name` and `length`, an array's `length`): a built-in listed in
    /// [`crate::key_order::KEY_ORDER`].
    pub order_leads: bool,
    /// What script defined on a value type besides the type's properties
    /// ([`crate::valuetypes::members`]).
    pub members: Option<Obj>,
    /// `Object.seal` or `Object.freeze` was called on this value type.
    pub sealed: bool,
}

/// An own symbol-keyed property.
#[derive(Debug, Clone)]
pub struct SymProp {
    pub key: Rc<JsSymbol>,
    pub value: Value,
    /// An accessor property's functions (`value` is then unused).
    pub accessor: Option<Box<Accessor>>,
    pub enumerable: bool,
    /// [`READONLY`] and [`PERMANENT`], as [`JsObject::attr`] gives them for string keys.
    pub attrs: u8,
}

/// WeakMap [[WeakMapData]] / WeakSet [[WeakSetData]]: entries with object keys live on the
/// key ([`Ext::weak`]); this lists those keys (to drop the entries with the collection) and
/// holds the entries keyed by QML objects and other host values, which the JS heap does not
/// own.
#[derive(Debug, Default)]
pub struct WeakColl {
    /// The address of the collection's object, which its key-side entries name.
    pub id: usize,
    pub keys: Vec<Weak<RefCell<JsObject>>>,
    pub others: Vec<(Value, Value)>,
}

impl Drop for WeakColl {
    fn drop(&mut self) {
        let mut o = Orphans::default();
        for k in self.keys.drain(..) {
            if let Some(k) = k.upgrade()
                && let Ok(mut b) = k.try_borrow_mut()
                && let Some(e) = b.ext.as_deref_mut()
            {
                let id = self.id;
                e.weak.retain_mut(|(m, v)| {
                    let keep = m.strong_count() > 0 && Weak::as_ptr(m) as usize != id;
                    if !keep {
                        o.value(v);
                    }
                    keep
                });
            }
        }
        for (k, v) in &mut self.others {
            o.value(k);
            o.value(v);
        }
    }
}

/// Which items a built-in iterator yields (ECMA-262 CreateArrayIterator `kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IterKind {
    Keys,
    Values,
    Entries,
}

/// A built-in iterator's state (%ArrayIteratorPrototype% and kin, ECMA-262 §23.1.5).
#[derive(Debug)]
pub struct ListIter {
    /// The array-like iterated live (its `length` read at each step), or `Undefined` when
    /// iterating the snapshot `items` (String iterators).
    pub src: Value,
    pub items: Vec<Value>,
    pub kind: IterKind,
    pub pos: usize,
    pub done: bool,
}

/// A Map or Set iterator's state (%MapIteratorPrototype%, %SetIteratorPrototype%, ECMA-262
/// §24.1.5, §24.2.5): the collection store it walks (`None` once done), whether a Map's, what
/// it yields, and the id of the next entry to visit, so entries added while it runs are seen
/// and deleted ones skipped.
#[derive(Debug)]
pub struct CollIter {
    pub store: Option<Obj>,
    pub map: bool,
    pub kind: IterKind,
    pub cursor: u64,
}

/// A URLSearchParams iterator's state (a WebIDL default iterator object, WebIDL §3.7.10.1):
/// its target, what it yields, and the index of the next pair. The target's list is read at
/// each step, and an iterator past the end yields again once the list grows (WebIDL keeps no
/// done flag).
#[derive(Debug)]
pub struct ParamsIter {
    pub params: Obj,
    pub kind: IterKind,
    pub index: usize,
}

/// A RegExp String Iterator's state (%RegExpStringIteratorPrototype%, ECMA-262 §22.2.9): its
/// matcher (`Undefined` once done), the subject, and the `global` and full-Unicode flags.
#[derive(Debug)]
pub struct RegExpStringIter {
    pub matcher: Value,
    pub s: Value,
    pub s16: Rc<[u16]>,
    pub global: bool,
    pub unicode: bool,
}

/// The functions of an accessor property: its [[Get]] and [[Set]] attributes.
#[derive(Debug, Clone, Default)]
pub struct Accessor {
    pub get: Option<Value>,
    pub set: Option<Value>,
}

/// Internal slots (ECMA-262 §6.1.7.2) that make an object a built-in of some kind.
#[derive(Debug, Default)]
pub enum Internal {
    #[default]
    Ordinary,
    /// Date: [[DateValue]], ms since the epoch (NaN when invalid).
    Date(f64),
    /// RegExp: [[OriginalSource]] and [[OriginalFlags]], which [[RegExpMatcher]] is compiled
    /// from.
    RegExp { source: Rc<str>, flags: Rc<str> },
    /// Error: [[ErrorData]]; stringifies as `name: message`.
    Error,
    /// Map: [[MapData]], an array of `[key, value]` entry arrays.
    Map(Obj),
    /// Set: [[SetData]], an array of values.
    Set(Obj),
    /// `Qt.locale()`: index of its date/time data, and whether it is QLocale::system() (made
    /// without a name).
    Locale(u16, bool),
    /// An arguments object (ECMA-262 §10.4.4): with the [[ParameterMap]] of a mapped one.
    Arguments(Option<Box<ArgsMap>>),
    /// A Number, Boolean or String object: its [[NumberData]], [[BooleanData]] or
    /// [[StringData]] (ECMA-262 §21.1.4, §20.3.4, §22.1.4), always a primitive of that type.
    Primitive(Value),
    /// `Number`, `Boolean` and `String`: [[Construct]] wraps what [[Call]] returns in a new
    /// object with that internal slot (ECMA-262 §21.1.1.1, §20.3.1.1, §22.1.1.1).
    WrapperConstructor,
    /// A generator object (ECMA-262 §27.5): its `ctx` object, which holds its state.
    Generator(Obj),
    /// The `ctx` of a generator: its [[GeneratorState]] and resumption state.
    GenCtx(Box<crate::generator::GenCore>),
    /// A generator function's `prototype` object, which inherits %GeneratorPrototype%.
    GeneratorPrototype,
    /// A list of objects read from a QQmlListProperty (QV4::QmlListWrapper): it has an array's
    /// elements and methods and is no Array.
    QmlList,
    /// The Iterator Record a lowered generator body loops over (`for-of`), and its current
    /// value.
    IterRecord(Box<(crate::eval::IterSrc, Value)>),
    /// A Symbol object: its [[SymbolData]] (ECMA-262 §20.4.3).
    Symbol(Rc<JsSymbol>),
    /// WeakMap: [[WeakMapData]].
    WeakMap(Box<WeakColl>),
    /// WeakSet: [[WeakSetData]].
    WeakSet(Box<WeakColl>),
    /// A built-in iterator (%ArrayIteratorPrototype% and kin).
    Iter(Box<ListIter>),
    /// A RegExp String Iterator (`matchAll`).
    RegExpStringIter(Box<RegExpStringIter>),
    /// A scope object of a generator's split block (`ir::genlower`): its bindings are its
    /// properties, `Value::Empty` while in their temporal dead zone; the `const` ones listed.
    GenScope(Box<Vec<Rc<str>>>),
    /// ArrayBuffer: [[ArrayBufferData]], shared with the views on it.
    ArrayBuffer(crate::typedarray::Bytes),
    /// An ArrayBuffer whose [[ArrayBufferData]] is null (IsDetachedBuffer, ECMA-262
    /// §25.1.3.3): Qt hands one out for an `XMLHttpRequest` response without a body.
    DetachedArrayBuffer,
    /// SharedArrayBuffer: its [[ArrayBufferData]] (this runtime has one agent).
    SharedArrayBuffer(crate::typedarray::Bytes),
    /// A typed array (ECMA-262 §10.4.5): its window on a buffer.
    TypedArray(Box<crate::typedarray::View>),
    /// DataView: its byte window on a buffer.
    DataView(Box<crate::typedarray::View>),
    /// A Proxy exotic object (ECMA-262 §10.5): its [[ProxyTarget]] and [[ProxyHandler]],
    /// `None` once revoked.
    Proxy(Option<Box<(Obj, Obj)>>),
    /// A `Proxy.revocable` revocation function: its [[RevocableProxy]], `None` once called.
    RevocableProxy(Option<Obj>),
    /// `Qt.binding(f)` (QQmlBindingFunction): assigned to a QML property, `f` becomes that
    /// property's binding.
    Binding(Value),
    /// Promise: [[PromiseState]], [[PromiseResult]] and its reactions.
    Promise(Box<crate::promise::Slot>),
    /// A QtQuick value type (`font`, `matrix4x4`, `quaternion`, `vector2d/3d/4d`): its
    /// properties are its own data properties ([`crate::valuetypes`]).
    ValueType(crate::valuetypes::Vt),
    /// A `url` value (a QUrl in a QV4::VariantObject): its string form, and the string it was
    /// made of when QUrl calls that invalid (such a url prints as nothing and still holds what
    /// was parsed, which is what `==` compares).
    Url(Rc<str>, Option<Rc<str>>),
    /// A QVariantMap a property of a value type holds (`font.features`): a
    /// QV4::ReferenceObject, which is what Object.prototype.toString calls it.
    VariantMap,
    /// A QQuickAnchorLine (`parent.left`): the item and which of its lines, `None` for the
    /// line of an anchor nothing set (QQuickAnchorLine(): no item).
    AnchorLine(Option<(usize, &'static str)>),
    /// A QQmlScriptString (`anchors.left` of an AnchorChanges): the object and property whose
    /// expression it is, `None` for the one of a property nothing set.
    ScriptString(Option<(usize, crate::runtime::Sym)>),
    /// A Map or Set iterator.
    CollIter(Box<CollIter>),
    /// A URLSearchParams iterator.
    ParamsIter(Box<ParamsIter>),
}

/// A String value's code units: the UTF-8 text of a well-formed one or the UTF-16 units of one
/// with an unpaired surrogate.
#[derive(Debug, Clone, Copy)]
pub enum JsStr<'a> {
    Utf8(&'a str),
    Utf16(&'a [u16]),
}

impl<'a> JsStr<'a> {
    /// The UTF-16 code units.
    pub fn units(self) -> std::borrow::Cow<'a, [u16]> {
        match self {
            JsStr::Utf8(s) => {
                crate::utf16::reading(s);
                s.encode_utf16().collect::<Vec<_>>().into()
            }
            JsStr::Utf16(u) => u.into(),
        }
    }

    /// The code units, without collecting them.
    pub fn code_units(self) -> impl Iterator<Item = u16> + 'a {
        let (a, b) = match self {
            JsStr::Utf8(s) => (Some(s.encode_utf16()), None),
            JsStr::Utf16(u) => (None, Some(u.iter().copied())),
        };
        a.into_iter().flatten().chain(b.into_iter().flatten())
    }

    /// Code-unit order (IsLessThan of two strings, ECMA-262 §7.2.13).
    pub fn cmp_units(self, other: JsStr<'_>) -> std::cmp::Ordering {
        self.code_units().cmp(other.code_units())
    }

    /// The number of code units (`length`).
    pub fn len(self) -> usize {
        match self {
            JsStr::Utf8(s) => {
                crate::utf16::reading(s);
                if s.is_ascii() {
                    s.len()
                } else {
                    s.encode_utf16().count()
                }
            }
            JsStr::Utf16(u) => u.len(),
        }
    }

    pub fn is_empty(self) -> bool {
        match self {
            JsStr::Utf8(s) => s.is_empty(),
            JsStr::Utf16(u) => u.is_empty(),
        }
    }

    /// The text, each unpaired surrogate U+FFFD: what a host (Qt's `QString::toUtf8`) sees.
    pub fn lossy(self) -> std::borrow::Cow<'a, str> {
        match self {
            JsStr::Utf8(s) => s.into(),
            JsStr::Utf16(u) => String::from_utf16_lossy(u).into(),
        }
    }
}

/// A String being built from parts (concatenation, `join`, templates): UTF-8 until a part with
/// an unpaired surrogate arrives, then UTF-16.
#[derive(Debug, Default)]
pub struct JsBuf {
    s: String,
    w: Option<Vec<u16>>,
}

impl JsBuf {
    pub fn new() -> Self {
        JsBuf::default()
    }

    pub fn push_str(&mut self, t: &str) {
        match &mut self.w {
            None => self.s.push_str(t),
            Some(w) => w.extend(t.encode_utf16()),
        }
    }

    pub fn push(&mut self, t: JsStr<'_>) {
        match (t, &mut self.w) {
            (JsStr::Utf8(t), _) => self.push_str(t),
            (JsStr::Utf16(u), Some(w)) => w.extend_from_slice(u),
            (JsStr::Utf16(u), None) => {
                let mut w: Vec<u16> = self.s.encode_utf16().collect();
                w.extend_from_slice(u);
                self.w = Some(w);
            }
        }
    }

    /// Appends a String value (anything else as its [`Value::to_js_string`]).
    pub fn push_value(&mut self, v: &Value) {
        match v.js_str() {
            Some(t) => self.push(t),
            None => self.push_str(&v.to_js_string()),
        }
    }

    /// The String; halves of a pair pushed apart join into its code point.
    pub fn finish(self) -> Value {
        match self.w {
            None => Value::Str(Rc::from(self.s)),
            Some(w) => Value::from_utf16(&w),
        }
    }
}

/// The primitive in the [[NumberData]], [[BooleanData]] or [[StringData]] of `o`.
pub fn wrapped(o: &Obj) -> Option<Value> {
    match &o.borrow().internal {
        Internal::Primitive(p) => Some(p.clone()),
        _ => None,
    }
}

/// A mapped arguments object's [[ParameterMap]] (ECMA-262 §10.4.4.7), and the storage of the
/// function's formal parameter bindings: formal `k` reads and writes `arguments[k]` while the
/// two are mapped, and its own cell once `delete arguments[k]` unmapped them (or when it never
/// had an argument).
#[derive(Debug)]
pub struct ArgsMap {
    /// The binding of each formal (the interpreter's frame slot, `k` in compiled code);
    /// `u32::MAX` for a formal whose name a later formal rebinds.
    pub ids: Box<[u32]>,
    /// The property key `"k"` of each formal.
    pub keys: Box<[Rc<str>]>,
    /// `None` while formal `k` is mapped to `arguments[k]`, else its binding's value.
    pub cells: Box<[Option<Value>]>,
}

impl Internal {
    /// The entry store of a Map or Set.
    pub fn collection(&self) -> Option<&Obj> {
        match self {
            Internal::Map(s) | Internal::Set(s) => Some(s),
            _ => None,
        }
    }

    /// The objects these slots hold (the cycle collector's edges), besides an arguments
    /// object's values.
    pub fn objs(&self) -> impl Iterator<Item = &Obj> {
        let (a, b) = match self {
            Internal::Map(s) | Internal::Set(s) | Internal::RevocableProxy(Some(s)) => {
                (Some(s), None)
            }
            Internal::Proxy(Some(th)) => (Some(&th.0), Some(&th.1)),
            _ => (None, None),
        };
        a.into_iter().chain(b)
    }
}

/// The most holes one write stores in an array (`new Array(n)`, `a.length = n`, `a[i] = v`
/// past the end): a longer run becomes the array's [`JsObject::tail`]. 64 KiB of holes
/// cost 1 MiB.
pub const SPARSE_GAP: usize = 1 << 16;

/// A bit set of array indices (see [`JsObject::holes`]), with what QV4's array storage
/// (`SimpleArrayData`, qv4arraydata.cpp) tells apart about them.
#[derive(Debug, Clone, PartialEq)]
pub struct Holes {
    bits: Vec<u64>,
    /// QV4's `values.size`, at least: the positions below it have a cell, so a hole there
    /// is an empty cell that `a[i] = v` fills whatever the array's integrity level is, and
    /// a hole at or past it has none. `usize::MAX`: every position has one. The cells reach
    /// the last element all the same (see [`JsObject::slots`]).
    pub slots: usize,
    /// The array has QV4's attribute bytes (`ArrayData::ensureAttributes`: after `freeze`,
    /// `seal` or a `defineProperty` that changes attributes), and the positions below this
    /// had a cell then. A value stored later at or past it has a zero byte: [`HIDDEN`].
    pub known: Option<usize>,
    /// QV4 holds the elements in its sparse storage (`SparseArrayData`), where every write
    /// looks at the attributes as ECMA-262 says: one of them is, or was, an accessor.
    pub sparse: bool,
}

impl Default for Holes {
    fn default() -> Self {
        Holes {
            bits: Vec::new(),
            slots: usize::MAX,
            known: None,
            sparse: false,
        }
    }
}

impl Holes {
    pub fn contains(&self, i: usize) -> bool {
        self.bits
            .get(i / 64)
            .is_some_and(|w| w >> (i % 64) & 1 != 0)
    }

    pub fn insert(&mut self, i: usize) {
        if self.bits.len() <= i / 64 {
            self.bits.resize(i / 64 + 1, 0);
        }
        self.bits[i / 64] |= 1 << (i % 64);
    }

    pub fn remove(&mut self, i: usize) {
        if let Some(w) = self.bits.get_mut(i / 64) {
            *w &= !(1 << (i % 64));
        }
    }

    /// The last index in `from..len` that is not a hole.
    fn last_present(&self, from: usize, len: usize) -> Option<usize> {
        let mut end = len;
        while end > from {
            let wi = (end - 1) / 64;
            let start = from.max(wi * 64);
            let Some(&w) = self.bits.get(wi) else {
                return Some(end - 1);
            };
            let above = end - wi * 64;
            let mut present = !w & (u64::MAX << (start - wi * 64));
            if above < 64 {
                present &= (1 << above) - 1;
            }
            if present != 0 {
                return Some(wi * 64 + 63 - present.leading_zeros() as usize);
            }
            end = start;
        }
        None
    }

    /// No hole and nothing of QV4's storage to remember: the array needs no `Holes`.
    fn is_plain(&self) -> bool {
        self.known.is_none()
            && !self.sparse
            && self.slots == usize::MAX
            && self.bits.iter().all(|&w| w == 0)
    }

    /// Forgets the holes, keeping what is known of QV4's storage.
    pub fn clear(&mut self) {
        self.bits.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits.iter().enumerate().flat_map(|(wi, &w)| {
            let mut w = w;
            std::iter::from_fn(move || {
                (w != 0).then(|| {
                    let b = w.trailing_zeros() as usize;
                    w &= w - 1;
                    wi * 64 + b
                })
            })
        })
    }
}

impl JsObject {
    /// Array element `i` is a hole.
    pub fn is_hole(&self, i: usize) -> bool {
        self.holes.as_ref().is_some_and(|h| h.contains(i))
    }

    pub fn add_holes(&mut self, holes: impl IntoIterator<Item = usize>) {
        let mut h = self.holes.take().unwrap_or_default();
        holes.into_iter().for_each(|i| h.insert(i));
        if self.tail > 0 || !h.is_plain() {
            self.holes = Some(h);
        }
    }

    /// The `length` of an array: its stored elements and its tail.
    pub fn array_length(&self) -> usize {
        self.array.as_ref().map_or(0, Vec::len) + self.tail
    }

    /// Sets the `length` of an array to `n` (ArraySetLength, ECMA-262 §10.4.2.4, of an array
    /// without restricted elements): the elements at `n..` are deleted; new ones are holes,
    /// stored when at most [`SPARSE_GAP`] are added at once and a tail otherwise.
    pub fn set_array_length(&mut self, n: usize) {
        let Some(a) = &mut self.array else {
            return;
        };
        let stored = a.len();
        if n <= stored {
            a.truncate(n);
            self.tail = 0;
        } else if self.tail == 0 && n - stored <= SPARSE_GAP {
            a.resize(n, Value::Undefined);
            self.add_holes(stored..n);
        } else {
            self.tail = n - stored;
        }
        // Keeps `holes` while there is a tail, and drops the holes past the new length.
        self.add_holes([]);
        self.remap_holes(Some);
        self.drop_slow_elements(n);
    }

    /// Writes element `i` of an array as an ordinary data property would be written,
    /// growing the array past it (ECMA-262 §10.4.2.1). An element more than [`SPARSE_GAP`]
    /// past the stored ones, or anywhere in a tail but at its start, is held as an ordinary
    /// property, so the gap before it takes no storage.
    pub fn store_element(&mut self, i: usize, val: Value) {
        let Some(a) = &mut self.array else {
            return;
        };
        let stored = a.len();
        if i < stored {
            a[i] = val;
            self.fill_hole(i);
        } else if i == stored {
            a.push(val);
            if self.tail > 0 {
                self.tail -= 1;
                self.add_holes([]);
            }
        } else if self.tail == 0 && i - stored <= SPARSE_GAP {
            a.resize(i + 1, Value::Undefined);
            a[i] = val;
            self.add_holes(stored..i);
        } else {
            let k: Rc<str> = i.to_string().into();
            if self.props.insert(k.clone(), val).is_none() {
                crate::eval::push_key(&mut self.keys, k);
            }
            self.tail = self.tail.max(i + 1 - stored);
            self.add_holes([]);
        }
    }

    /// Moves the holes after `array` changed: `f` maps an old index to its new one (`None`:
    /// removed or now present); holes past the new length are dropped.
    pub fn remap_holes(&mut self, f: impl Fn(usize) -> Option<usize>) {
        if let Some(mut h) = self.holes.take() {
            let len = self.array.as_ref().map_or(0, Vec::len);
            let moved: Vec<usize> = h.iter().filter_map(f).filter(|&i| i < len).collect();
            h.clear();
            self.holes = Some(h);
            self.add_holes(moved);
        }
    }

    /// QV4's `values.size` of this array: the positions below it have a cell. A stored
    /// element has one, and so has every position before it.
    pub fn slots(&self) -> usize {
        let len = self.array.as_ref().map_or(0, Vec::len);
        match &self.holes {
            Some(h) if h.slots < len => h.last_present(h.slots, len).map_or(h.slots, |i| i + 1),
            _ => len,
        }
    }

    /// QV4 holds this array's elements in its sparse storage (see [`Holes::sparse`]).
    pub fn sparse(&self) -> bool {
        self.holes.as_ref().is_some_and(|h| h.sparse)
    }

    pub fn set_sparse(&mut self) {
        if self.array.is_some() {
            self.holes.get_or_insert_with(Default::default).sparse = true;
        }
    }

    /// The array has cells for its first `n` positions only (see [`Holes::slots`]).
    pub fn set_slots(&mut self, n: usize) {
        let len = self.array.as_ref().map_or(0, Vec::len);
        let n = if n >= len { usize::MAX } else { n };
        match &mut self.holes {
            Some(h) => h.slots = n,
            None if n != usize::MAX => {
                self.holes = Some(Box::new(Holes {
                    slots: n,
                    ..Holes::default()
                }));
            }
            None => {}
        }
        if self.holes.as_ref().is_some_and(|h| h.is_plain()) {
            self.holes = None;
        }
    }

    /// The array's length is about to grow with no cell added (`length = n`, a slow
    /// element past the end): its cells stay the ones it has.
    pub fn keep_slots(&mut self) {
        let n = self.slots();
        self.holes.get_or_insert_with(Default::default).slots = n;
    }

    /// The array's cells moved by `by` positions (`shift`, `unshift`).
    pub fn shift_slots(&mut self, by: isize) {
        if let Some(h) = &mut self.holes
            && h.slots != usize::MAX
        {
            h.slots = h.slots.saturating_add_signed(by);
        }
    }

    /// Array element `k` is [`HIDDEN`] and no property at all.
    pub fn hidden(&self, k: &str) -> bool {
        self.attrs.is_some() && self.attr(k) & (HIDDEN | PERMANENT) == HIDDEN
    }

    /// The positions below `n` have a cell from now on.
    pub fn grow_slots(&mut self, n: usize) {
        if let Some(h) = &mut self.holes
            && h.slots < n
        {
            h.slots = n;
        }
    }

    /// Whether the array has QV4's attribute bytes (see [`Holes::known`]).
    pub fn has_bytes(&self) -> bool {
        self.holes.as_ref().is_some_and(|h| h.known.is_some())
    }

    /// QV4's `ArrayData::ensureAttributes`: the cells the array has now get a byte.
    pub fn ensure_bytes(&mut self) {
        let stored = self.cells || self.array.as_ref().is_some_and(|a| !a.is_empty());
        if stored && !self.has_bytes() {
            let n = self.slots();
            let h = self.holes.get_or_insert_with(Default::default);
            h.slots = n;
            h.known = Some(n);
        }
    }

    /// The attribute bits a position of this array has once nothing is stored at it or its
    /// property is deleted: QV4 writes a plain data byte (`SimpleArrayData::del`), which
    /// past [`Holes::known`] is not the zero the position had.
    pub fn cleared_attr(&self, i: usize) -> u8 {
        match &self.holes {
            Some(h) if !h.sparse && h.known.is_some_and(|known| i >= known) => BYTE,
            _ => 0,
        }
    }

    /// A value was stored at position `i`, which held none, by a write that sets no
    /// attribute byte: past the bytes the array has it is [`HIDDEN`].
    pub fn stored_without_byte(&mut self, i: usize) {
        if let Some(h) = &self.holes
            && !h.sparse
            && h.known.is_some_and(|known| i >= known)
        {
            let k = i.to_string();
            if self.attr(&k) == 0 {
                self.set_attr(&k, HIDDEN);
            }
        }
    }

    /// Element `i` exists from now on (it was written).
    pub fn fill_hole(&mut self, i: usize) {
        if let Some(h) = &mut self.holes {
            h.remove(i);
        }
    }

    /// Element `i` of an array that is not a QML sequence, if present in its dense storage:
    /// the fast path of an indexed read, which otherwise goes through [[Get]].
    #[inline]
    pub fn dense_element(&self, i: usize) -> Option<Value> {
        let a = self.array.as_ref()?;
        (i < a.len() && self.owner.is_none() && !self.is_hole(i)).then(|| a[i].clone())
    }

    /// Array index `i` of an array is held as an ordinary property (an accessor or a
    /// non-enumerable value) from now on: its dense slot becomes a hole, growing the array
    /// past it if needed (ArraySetLength, ECMA-262 §10.4.2.4).
    pub fn make_slow_element(&mut self, i: usize) {
        let len = self.array.as_ref().map_or(0, Vec::len);
        if i < len || (self.tail == 0 && i - len < SPARSE_GAP) {
            // QV4 gives the positions up to it cells.
            if let Some(a) = &mut self.array
                && i >= len
            {
                a.resize(i + 1, Value::Undefined);
            } else if let Some(a) = &mut self.array {
                a[i] = Value::Undefined;
            }
            self.add_holes((len..i).chain([i]));
        } else {
            self.tail = self.tail.max(i + 1 - len);
            self.add_holes([]);
        }
    }

    /// Whether an array holds an element as an ordinary property (see
    /// [`JsObject::make_slow_element`]), which its dense fast paths do not see.
    pub fn has_slow_elements(&self) -> bool {
        self.tail > 0
            || self.holes.is_some()
                && (self.props.keys())
                    .chain(self.accessors.iter().flat_map(|m| m.keys()))
                    .any(|k| crate::eval::array_index(k).is_some())
    }

    /// Drops the elements at indices `n..` held as ordinary properties, as truncating an
    /// array's `length` to `n` deletes them.
    pub fn drop_slow_elements(&mut self, n: usize) {
        let gone = |k: &str| crate::eval::array_index(k).is_some_and(|i| i >= n);
        self.props.retain(|k, _| !gone(k));
        if let Some(m) = &mut self.accessors {
            m.retain(|k, _| !gone(k));
        }
        self.keys.retain(|k| !gone(k));
        // QV4's attribute bytes stay with the position (`SimpleArrayData::truncate`).
        if self.has_bytes() && !self.sparse() {
            return;
        }
        if let Some(m) = &mut self.attrs {
            m.retain(|k, _| !gone(k));
        }
    }
}

/// The last reference to a JS heap node, taken out of a dropping parent so nested values are
/// freed by the loop in [`sweep`] instead of by recursion: dropping a 100k-deep `[[[…]]]`, a long
/// prototype chain or a closure chain would otherwise overflow the native stack.
enum Garbage {
    Obj(Obj),
    Scope(crate::eval::Scope),
    Slots(Rc<crate::jsrt::Slots>),
}

thread_local! {
    static GARBAGE: RefCell<Vec<Garbage>> = const { RefCell::new(Vec::new()) };
    static SWEEPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Collects the children a dropping node holds the last reference to; frees them on drop.
#[derive(Default)]
pub struct Orphans(bool);

impl Orphans {
    fn defer(&mut self, g: Garbage) {
        // During thread-local teardown the closure (and `g` in it) just drops recursively.
        if GARBAGE.try_with(|q| q.borrow_mut().push(g)).is_ok() {
            self.0 = true;
        }
    }

    pub fn value(&mut self, v: &mut Value) {
        if let Value::Obj(o) = v
            && Rc::strong_count(o) == 1
            && let Value::Obj(o) = std::mem::take(v)
        {
            self.defer(Garbage::Obj(o));
        }
    }

    pub fn values<'a>(&mut self, vs: impl IntoIterator<Item = &'a mut Value>) {
        vs.into_iter().for_each(|v| self.value(v));
    }

    pub fn obj(&mut self, o: Option<Obj>) {
        if let Some(o) = o.filter(|o| Rc::strong_count(o) == 1) {
            self.defer(Garbage::Obj(o));
        }
    }

    pub fn scope(&mut self, s: Option<crate::eval::Scope>) {
        if let Some(s) = s.filter(|s| Rc::strong_count(s) == 1) {
            self.defer(Garbage::Scope(s));
        }
    }

    pub fn slots(&mut self, f: Option<Rc<crate::jsrt::Slots>>) {
        if let Some(f) = f.filter(|f| Rc::strong_count(f) == 1) {
            self.defer(Garbage::Slots(f));
        }
    }

    pub fn callable(&mut self, c: Option<crate::eval::Callable>) {
        use crate::eval::Callable;
        match c {
            #[cfg(feature = "interp")]
            Some(Callable::Closure(_, s) | Callable::Class(_, s)) => self.scope(Some(s)),
            Some(Callable::Compiled(_, f, _)) => self.slots(Some(f)),
            Some(Callable::Bound(b)) => {
                if let Ok(crate::eval::BoundFn {
                    mut target,
                    mut this,
                    mut args,
                    ..
                }) = Rc::try_unwrap(b)
                {
                    self.value(&mut target);
                    self.value(&mut this);
                    self.values(args.iter_mut());
                }
            }
            _ => {}
        }
    }
}

impl Drop for Orphans {
    fn drop(&mut self) {
        if self.0 {
            sweep();
        }
    }
}

/// Frees queued garbage until none is left; the outermost dropping node runs the loop and every
/// node freed by it only queues its own orphans.
fn sweep() {
    if SWEEPING.try_with(|s| s.replace(true)).unwrap_or(true) {
        return;
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            let _ = SWEEPING.try_with(|s| s.set(false));
        }
    }
    let _done = Done;
    while let Some(g) = GARBAGE.with(|q| q.borrow_mut().pop()) {
        match g {
            Garbage::Obj(o) => drop(o),
            Garbage::Scope(s) => drop(s),
            Garbage::Slots(f) => drop(f),
        }
    }
}

impl Drop for JsObject {
    fn drop(&mut self) {
        let mut o = Orphans::default();
        o.values(self.props.values_mut());
        o.values(self.array.iter_mut().flatten());
        o.obj(self.proto.take());
        o.callable(self.call.take());
        if let Some(a) = self.accessors.as_deref_mut() {
            for acc in a.values_mut() {
                o.values(acc.get.iter_mut().chain(acc.set.iter_mut()));
            }
        }
        if let Some(e) = self.ext.as_deref_mut() {
            for p in &mut e.syms {
                o.value(&mut p.value);
                if let Some(a) = p.accessor.as_deref_mut() {
                    o.values(a.get.iter_mut().chain(a.set.iter_mut()));
                }
            }
            o.values(e.weak.iter_mut().map(|(_, v)| v));
            o.obj(e.members.take());
        }
        match std::mem::take(&mut self.internal) {
            Internal::Map(s) | Internal::Set(s) | Internal::RevocableProxy(Some(s)) => {
                o.obj(Some(s))
            }
            Internal::Proxy(Some(th)) => {
                let (t, h) = *th;
                o.obj(Some(t));
                o.obj(Some(h));
            }
            Internal::CollIter(mut c) => o.obj(c.store.take()),
            Internal::ParamsIter(p) => o.obj(Some(p.params)),
            Internal::RegExpStringIter(mut r) => o.value(&mut r.matcher),
            Internal::Iter(mut l) => {
                o.value(&mut l.src);
                o.values(l.items.iter_mut());
            }
            Internal::Arguments(Some(mut m)) => o.values(m.cells.iter_mut().flatten()),
            Internal::Generator(c) => o.obj(Some(c)),
            Internal::GenCtx(mut c) => o.values(c.values_mut()),
            Internal::IterRecord(mut i) => {
                o.values(i.0.values_mut());
                o.value(&mut i.1);
            }
            Internal::TypedArray(v) | Internal::DataView(v) => o.obj(Some(v.buffer)),
            Internal::Binding(mut f) => o.value(&mut f),
            Internal::Promise(mut p) => o.values(p.values_mut()),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum Value {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    /// A String with an unpaired surrogate, as its UTF-16 code units (ECMA-262 §6.1.4 allows
    /// any sequence). Every well-formed string is a [`Value::Str`], the compact form hosts
    /// read; only [`Value::from_utf16`] makes this one, so the two never hold the same string.
    Str16(Rc<[u16]>),
    Obj(Obj),
    /// Reference to a QML object instance (index into the runtime arena).
    Item(usize),
    /// Native builtin function id.
    Native(u32),
    /// QML `color` value type.
    Color(crate::color::Color),
    /// The value of a `let`/`const`/`class` binding before its declaration runs (ECMA-262
    /// §9.1.1.1, the temporal dead zone). Only binding storage holds it: reading or writing
    /// such a binding throws instead of producing it.
    Empty,
    Symbol(Rc<JsSymbol>),
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    /// The String of the UTF-16 code units `u`: a [`Value::Str`] unless one is an unpaired
    /// surrogate.
    pub fn from_utf16(u: &[u16]) -> Value {
        match String::from_utf16(u) {
            Ok(s) => Value::Str(Rc::from(s)),
            Err(_) => Value::Str16(Rc::from(u)),
        }
    }

    /// The code units of a String value.
    #[inline]
    pub fn js_str(&self) -> Option<JsStr<'_>> {
        match self {
            Value::Str(s) => Some(JsStr::Utf8(s)),
            Value::Str16(u) => Some(JsStr::Utf16(u)),
            _ => None,
        }
    }

    /// Whether this is a String value (`typeof` "string").
    #[inline]
    /// A string's text as a host sees it (`QString::toUtf8`: each unpaired surrogate U+FFFD,
    /// so UTF-16 positions stay put); none for a non-string.
    pub fn host_str(&self) -> Option<std::borrow::Cow<'_, str>> {
        self.js_str().map(JsStr::lossy)
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Value::Str(_) | Value::Str16(_))
    }

    pub fn array(v: Vec<Value>) -> Value {
        let mut o = JsObject::default();
        o.array = Some(v);
        Value::Obj(reg(Rc::new(RefCell::new(o))))
    }

    /// The array a built-in makes element by element (`map`, `slice`, `Array(n)`): the
    /// elements at `holes` do not exist, and the cells end at its last element.
    pub fn array_made(v: Vec<Value>, holes: impl IntoIterator<Item = usize>) -> Value {
        let a = Value::array_holes(v, holes);
        if let Value::Obj(o) = &a {
            let b = &mut *o.borrow_mut();
            b.cells = true;
            if let Some(h) = &mut b.holes {
                h.slots = 0;
            }
        }
        a
    }

    /// An array whose elements at `holes` do not exist (see [`JsObject::holes`]).
    pub fn array_holes(v: Vec<Value>, holes: impl IntoIterator<Item = usize>) -> Value {
        let a = Value::array(v);
        if let Value::Obj(o) = &a {
            o.borrow_mut().add_holes(holes);
        }
        a
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub fn to_number(&self) -> f64 {
        match self {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => *b as u8 as f64,
            Value::Num(n) => *n,
            Value::Str(s) => str_to_number(s),
            Value::Obj(o) if let Some(p) = wrapped(o) => p.to_number(),
            // ToPrimitive is `join`: undefined, null and holes (undefined slots) join as "", and
            // a one-element array as its element's string, so `[true]` is NaN and `[-0]` is +0.
            // Walked without recursion; a chain deeper than `MAX_JS_DEPTH` (or a cycle) joins
            // as "" here, like `to_js_string`.
            Value::Obj(o) => {
                let mut o = o.clone();
                for _ in 0..crate::eval::MAX_JS_DEPTH {
                    let next = match &o.borrow().array {
                        Some(a) if a.len() == 1 => match &a[0] {
                            Value::Undefined | Value::Null => return 0.0,
                            Value::Obj(x) => x.clone(),
                            Value::Str(s) => return str_to_number(s),
                            v => return str_to_number(&v.to_js_string()),
                        },
                        Some(a) if a.is_empty() => return 0.0,
                        _ => return f64::NAN,
                    };
                    o = next;
                }
                0.0
            }
            _ => f64::NAN,
        }
    }

    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined | Value::Empty => "undefined",
            Value::Obj(o) if o.borrow().call.is_some() => "function",
            Value::Null | Value::Obj(_) | Value::Item(_) | Value::Color(_) => "object",
            Value::Bool(_) => "boolean",
            Value::Num(_) => "number",
            Value::Str(_) | Value::Str16(_) => "string",
            Value::Native(_) => "function",
            Value::Symbol(_) => "symbol",
        }
    }

    /// ToString without calling JS. An array nested deeper than [`crate::eval::MAX_JS_DEPTH`]
    /// gives `""` here; [`Self::try_js_string`] (behind `Interp::to_string`) reports it.
    pub fn to_js_string(&self) -> String {
        match self {
            Value::Undefined | Value::Empty => "undefined".into(),
            Value::Null => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) => num_to_string(*n),
            Value::Str(s) => s.to_string(),
            Value::Str16(u) => String::from_utf16_lossy(u),
            Value::Obj(ob) if let Some(p) = wrapped(ob) => p.to_js_string(),
            Value::Obj(ob) => {
                let o = ob.borrow();
                match &o.array {
                    Some(a) => join_array(ob, a, ",").unwrap_or_default(),
                    None if matches!(o.internal, Internal::Binding(_)) => {
                        let n = o.props.get("name").map(|v| v.to_js_string());
                        format!("function {}() {{ [native code] }}", n.unwrap_or_default())
                    }
                    None if let Some(c) = &o.call => {
                        let name = match o.props.get("name") {
                            Some(n @ (Value::Str(_) | Value::Str16(_))) => n.to_js_string(),
                            Some(_) => String::new(),
                            None if o.attr("name") & ABSENT != 0 => String::new(),
                            // V4 prints a signal or a C++ method of an object without a name.
                            None if matches!(c, crate::eval::Callable::Item(..)) => String::new(),
                            // V4 prints every bound function under this name.
                            None if matches!(c, crate::eval::Callable::Bound(_)) => {
                                "__bound function__".into()
                            }
                            None => c.name().to_string(),
                        };
                        function_string(&name)
                    }
                    // %TypedArray%.prototype.toString is Array.prototype.toString: the join.
                    None if let Internal::TypedArray(t) = &o.internal => {
                        let n = t.numbers();
                        n.into_iter()
                            .map(num_to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    }
                    None if let Some(s) = crate::valuetypes::display(&o) => s,
                    None if matches!(o.internal, Internal::Error) => {
                        let get =
                            |k: &str| o.props.get(k).map(|v| v.to_js_string()).unwrap_or_default();
                        let (n, m) = (get("name"), get("message"));
                        if m.is_empty() { n } else { format!("{n}: {m}") }
                    }
                    None => "[object Object]".into(),
                }
            }
            Value::Item(i) => format!("QObject({i})"),
            Value::Color(c) => crate::builtins::color_name(c),
            Value::Native(_) => "function() { [native code] }".into(),
            // What `String(symbol)` gives; implicit conversion throws (`Interp::to_string`).
            Value::Symbol(s) => s.descriptive(),
        }
    }

    /// [`Self::to_js_string`], failing with [`TooDeep`] where V4 throws `RangeError`.
    pub fn try_js_string(&self) -> Result<String, TooDeep> {
        if let Value::Obj(ob) = self {
            let o = ob.borrow();
            if let Some(a) = &o.array {
                return join_array(ob, a, ",");
            }
        }
        Ok(self.to_js_string())
    }

    /// `===`
    pub fn strict_eq(&self, o: &Value) -> bool {
        match (self, o) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Str16(a), Value::Str16(b)) => a == b,
            (Value::Obj(a), Value::Obj(b)) => {
                Rc::ptr_eq(a, b)
                    || crate::valuetypes::equal(a, b)
                        .or_else(|| crate::builtins::locale_equal(a, b))
                        .unwrap_or(false)
                    || crate::types::same_type(a, b)
            }
            (Value::Item(a), Value::Item(b)) => a == b,
            (Value::Native(a), Value::Native(b)) => a == b,
            (Value::Symbol(a), Value::Symbol(b)) => Rc::ptr_eq(a, b),
            (Value::Color(a), Value::Color(b)) => a.same_qcolor(*b),
            _ => false,
        }
    }

    /// `==` (abstract equality)
    pub fn loose_eq(&self, o: &Value) -> bool {
        use Value::*;
        match (self, o) {
            (Undefined | Null, Undefined | Null) => true,
            (Undefined | Null, _) | (_, Undefined | Null) => false,
            (Num(_), Str(_) | Str16(_))
            | (Str(_) | Str16(_), Num(_))
            | (Bool(_), _)
            | (_, Bool(_)) => {
                if matches!((self, o), (Obj(_), _) | (_, Obj(_))) {
                    Value::str(&self.to_js_string()).loose_eq(&Value::str(&o.to_js_string()))
                } else {
                    self.to_number() == o.to_number()
                }
            }
            (Color(_), Str(_)) | (Str(_), Color(_)) => self.to_js_string() == o.to_js_string(),
            (Obj(_), Num(_) | Str(_) | Str16(_)) => Value::str(&self.to_js_string()).loose_eq(o),
            (Num(_) | Str(_) | Str16(_), Obj(_)) => self.loose_eq(&Value::str(&o.to_js_string())),
            _ => self.strict_eq(o),
        }
    }
}

thread_local! {
    /// Arrays being joined on this thread (`Array.prototype.join`'s cycle check).
    static JOINING: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

struct Joining(usize);

impl Drop for Joining {
    fn drop(&mut self) {
        JOINING.with(|j| {
            let mut j = j.borrow_mut();
            if let Some(i) = j.iter().rposition(|&p| p == self.0) {
                j.remove(i);
            }
        });
    }
}

/// Arrays nested deeper than [`crate::eval::MAX_JS_DEPTH`] while converting to a string, where
/// V4 runs out of stack and throws `RangeError` (`Interp::stack_overflow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooDeep;

/// Runs `f` with array `arr` on this thread's join stack (`Array.prototype.join`'s cycle
/// check): like V8 (QV4 6.12 overflows instead), an array already being joined further up (a
/// cycle) gives `None`, joining as `""` instead of recursing until the stack overflows.
/// Nesting deeper than [`crate::eval::MAX_JS_DEPTH`] is [`TooDeep`].
pub fn joining<T>(arr: &Obj, f: impl FnOnce() -> T) -> Result<Option<T>, TooDeep> {
    let id = Rc::as_ptr(arr) as *const () as usize;
    let (cyclic, depth) = JOINING.with(|j| {
        let j = j.borrow();
        (j.contains(&id), j.len())
    });
    if cyclic {
        return Ok(None);
    }
    if depth >= crate::eval::MAX_JS_DEPTH as usize {
        return Err(TooDeep);
    }
    JOINING.with(|j| j.borrow_mut().push(id));
    let _joining = Joining(id);
    Ok(Some(crate::eval::on_js_stack(f)))
}

/// `items.join(sep)` for the array object `arr` without calling JS (see [`joining`]):
/// `undefined`/`null` become empty, other elements their [`Value::try_js_string`].
/// `Interp::join` is the ECMA-262 algorithm, running the elements' own conversions.
pub fn join_array(arr: &Obj, items: &[Value], sep: &str) -> Result<String, TooDeep> {
    joining(arr, || {
        let mut out = String::new();
        for (i, v) in items.iter().enumerate() {
            if i > 0 {
                out.push_str(sep);
            }
            if !matches!(v, Value::Undefined | Value::Null) {
                out.push_str(&v.try_js_string()?);
            }
        }
        Ok(out)
    })?
    .unwrap_or_else(|| Ok(String::new()))
}

/// Number::toString per ECMA-262 (shortest round-trip digits, exponent outside [1e-7, 1e21)).
/// What V4's `Function.prototype.toString` prints for a function whose `name` is `name`: the
/// NativeFunction form of ECMA-262 §20.2.3.5, without the source text.
pub fn function_string(name: &str) -> String {
    if name.is_empty() {
        "function() { [native code] }".into()
    } else {
        format!("function {name}() {{ [native code] }}")
    }
}

pub fn num_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if n == 0.0 {
        return "0".into();
    }
    let a = n.abs();
    if n.fract() == 0.0 && a < 9e15 {
        return (n as i64).to_string();
    }
    let (digits, point) = shortest_digits(a);
    let d = String::from_utf8_lossy(&digits);
    let (k, sign) = (digits.len() as i32, if n < 0.0 { "-" } else { "" });
    if (k..=21).contains(&point) {
        format!("{sign}{d}{}", "0".repeat((point - k) as usize))
    } else if (1..=21).contains(&point) {
        let (int, frac) = d.split_at(point as usize);
        format!("{sign}{int}.{frac}")
    } else if (-5..=0).contains(&point) {
        format!("{sign}0.{}{d}", "0".repeat(-point as usize))
    } else {
        let e = point - 1;
        let (first, rest) = d.split_at(1);
        let dot = if rest.is_empty() { "" } else { "." };
        format!(
            "{sign}{first}{dot}{rest}e{}{e}",
            if e >= 0 { "+" } else { "" }
        )
    }
}

/// The digits of ECMA-262 Number::toString for finite `a > 0` and the position of the decimal
/// point (`a = 0.DIGITS × 10^point`): the fewest digits that read back as `a`, and of two
/// equally near candidates the even one (note 2 of §6.1.6.1.20, which QV4's
/// double-conversion follows).
pub fn shortest_digits(a: f64) -> (Vec<u8>, i32) {
    let split = |s: String| {
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let digits: Vec<u8> = m.bytes().filter(u8::is_ascii_digit).collect();
        (digits, e.parse::<i32>().unwrap_or(0) + 1)
    };
    let (digits, point) = split(format!("{a:e}"));
    let k = digits.len();
    // Two k-digit decimals can only both read back as `a` when their spacing is within an
    // ulp, which takes 16 digits or more.
    if k < 16 {
        return (digits, point);
    }
    // A tie: the exact value has one more digit, a 5.
    let (near, _) = split(format!("{a:.w$e}", w = k + 1));
    if near[k] != b'5' || near[k + 1] != b'0' {
        return (digits, point);
    }
    let (mut exact, exact_point) = split(format!("{a:.800e}"));
    while exact.last() == Some(&b'0') {
        exact.pop();
    }
    if exact.len() != k + 1 || !(exact[k - 1] - b'0').is_multiple_of(2) {
        return (digits, point);
    }
    exact.truncate(k);
    let text = format!("0.{}e{exact_point}", String::from_utf8_lossy(&exact));
    if text.parse::<f64>() == Ok(a) {
        (exact, exact_point)
    } else {
        (digits, point)
    }
}

/// ECMAScript WhiteSpace or LineTerminator: Rust's `White_Space` without U+0085, plus U+FEFF.
pub fn is_js_space(c: char) -> bool {
    c == '\u{FEFF}' || (c.is_whitespace() && c != '\u{85}')
}

/// `String.prototype.trim`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// Digits `d` (all valid in `radix`) as a number, correctly rounded up to 128 bits of digits.
pub fn radix_digits(d: &str, radix: u32) -> f64 {
    let mut acc: u128 = 0;
    for (i, c) in d.char_indices() {
        let v = c.to_digit(radix).unwrap_or(0) as u128;
        match acc
            .checked_mul(radix as u128)
            .and_then(|a| a.checked_add(v))
        {
            Some(a) => acc = a,
            None => {
                return d[i..].chars().fold(acc as f64, |a, c| {
                    a * radix as f64 + c.to_digit(radix).unwrap_or(0) as f64
                });
            }
        }
    }
    acc as f64
}

/// ToNumber applied to a string (StringToNumber, ECMA-262 7.1.4.1.1).
pub fn str_to_number(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = match t.get(..2) {
        Some("0x" | "0X") => 16,
        Some("0o" | "0O") => 8,
        Some("0b" | "0B") => 2,
        _ => 0,
    };
    if radix != 0 {
        let d = &t[2..];
        if d.is_empty() || !d.chars().all(|c| c.is_digit(radix)) {
            return f64::NAN;
        }
        return radix_digits(d, radix);
    }
    let u = t.strip_prefix(['+', '-']).unwrap_or(t);
    if u == "Infinity" {
        return if t.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    // Rust's float grammar also takes `inf`/`nan`; past this filter it is JS's decimal literal.
    if !u
        .bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_stored_index_is_found_by_words() {
        let mut h = Holes::default();
        for i in [0, 3, 63, 64, 65, 127, 128, 190, 191, 199] {
            h.insert(i);
        }
        for from in [0, 1, 4, 63, 64, 66, 128, 129, 192, 200] {
            for len in [0, 1, 4, 64, 65, 66, 128, 129, 191, 192, 200, 201, 300] {
                let slow = (from..len).rev().find(|&i| !h.contains(i));
                assert_eq!(h.last_present(from, len), slow, "{from}..{len}");
            }
        }
    }

    #[test]
    fn deeply_nested_values_drop_without_recursion() {
        const DEPTH: usize = 100_000;
        // A recursive drop takes hundreds of bytes of stack per level: 256 KiB holds a few hundred.
        let small = std::thread::Builder::new().stack_size(256 << 10);
        small
            .spawn(|| {
                let mut v = Value::Num(1.0);
                for _ in 0..DEPTH {
                    v = Value::array(vec![v]);
                }
                drop(v);
                let mut v = Value::Null;
                for _ in 0..DEPTH {
                    let mut o = JsObject::default();
                    o.props.insert("next".into(), v);
                    v = Value::Obj(Rc::new(RefCell::new(o)));
                }
                drop(v);
                let mut proto: Option<Obj> = None;
                for _ in 0..DEPTH {
                    let mut o = JsObject::default();
                    o.proto = proto.take();
                    proto = Some(Rc::new(RefCell::new(o)));
                }
                drop(proto);
                let mut sc: crate::eval::Scope = Default::default();
                for _ in 0..DEPTH {
                    let mut f = crate::eval::Frame::default();
                    f.parent = Some(sc);
                    f.vars.insert("v".into(), Value::array(vec![]));
                    sc = Rc::new(RefCell::new(f));
                }
                drop(sc);
            })
            .unwrap()
            .join()
            .unwrap();
    }
    #[test]
    fn numbers() {
        assert_eq!(num_to_string(1.0), "1");
        assert_eq!(num_to_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(num_to_string(1e21), "1e+21");
        assert_eq!(num_to_string(1.5e-7), "1.5e-7");
        assert_eq!(num_to_string(-42.5), "-42.5");
        assert_eq!(num_to_string(-0.0), "0");
        assert_eq!(num_to_string(-1793217195000.0), "-1793217195000");
        assert_eq!(num_to_string(9007199254740992.0), "9007199254740992");
        assert_eq!(num_to_string(1e20), "100000000000000000000");
        assert!(Value::str("5").loose_eq(&Value::Num(5.0)));
        assert!(!Value::Null.loose_eq(&Value::Num(0.0)));
        assert_eq!(Value::str(" 0x1f ").to_number(), 31.0);
    }

    #[test]
    fn string_to_number_matches_v8() {
        let n = |s: &str| str_to_number(s);
        for s in [
            "infinity", "inf", "Inf", "-inf", "nan", "0x-1", "0x", "\u{85}5", "1_000", "12px",
            "e5", ".",
        ] {
            assert!(n(s).is_nan(), "{s:?}");
        }
        assert_eq!(n("0b101"), 5.0);
        assert_eq!(n("0O17"), 15.0);
        assert_eq!(n("0x10000000000000000"), 18446744073709551616.0);
        assert_eq!(n("0x1fffffffffffff1"), 144115188075855860.0);
        assert_eq!(n("\u{FEFF}5\u{3000}"), 5.0);
        assert_eq!(n(" -Infinity "), f64::NEG_INFINITY);
        assert_eq!(n("1."), 1.0);
        assert_eq!(n("+.5e3"), 500.0);
        assert!(n("-0x10").is_nan());
        assert_eq!(n("1e1000"), f64::INFINITY);
        assert_eq!(js_trim(" \u{a0}x\u{FEFF} "), "x");
        assert_eq!(js_trim("\u{85}x"), "\u{85}x");
    }
}
