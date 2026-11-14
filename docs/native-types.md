# Native QML types

The QML tree reaches Rust code through types of the `Impulse` module, by name, the way
Quickshell's QML reaches its C++ types:

```qml
import Impulse

Item {
    onActivated: Clipboard.copy(text)
}
```

## A singleton (a service)

1. Declare it in a file of its own, `crates/qml/qmltypes/Impulse/<name>.qmltypes`, in the format
   `qmltyperegistrar` writes (`files.qmltypes` is an example): a `Component` with
   `exports: ["Impulse/<Name> 1.0"]`, `isSingleton: true`, `isCreatable: false`, and a
   `Property`, `Signal` or `Method` for each member. The build reads every `.qmltypes` file under
   `crates/qml/qmltypes`.
2. Give it a `Type` in the Rust module that implements it, and add that to `impulse::TYPES`
   (`crates/qml/src/impulse.rs`), one line, sorted by name:

   ```rust
   pub static TYPE: crate::impulse::Type = crate::impulse::Type {
       name: "Clipboard",
       methods: &[("copy", copy), ("paste", paste)],
       init: Some(init),
   };
   ```

   - `methods`: the function behind each declared method, `fn(&mut Interp, &Value, &[Value]) ->
     R<Value>` (`this`, then the arguments). `crate::runtime::world_of(it)` gives the `World`.
   - `init`: runs once, when QML first names the singleton; sets the starting values of the
     properties (`w.write(id, "name", value)`). Before it runs every declared property already
     exists with the default value of its type.
3. Later changes go through `World::write` on the instance (`w.singletons.get("Clipboard")`),
   which runs the bindings that read the property, and signals through `crate::native::emit`.
   Work that waits (a socket, D-Bus, a file) runs on a thread and answers through
   `impulse::replies` (below): nothing polls.

## An element QML creates

A type instantiated in QML (`HyprlandRequest { }`) is declared the same way without
`isSingleton` and `isCreatable: false`. Its Rust side is an `Element`, added to
`impulse::ELEMENTS`, one line, sorted by name:

```rust
pub static ELEMENT: crate::impulse::Element = crate::impulse::Element {
    methods: &[("send", send)],
    ..crate::impulse::Element::new("HyprlandRequest")
};

fn send(w: &mut World, item: usize, args: &[Value]) -> Value { ... }
```

A method gets the instance it was called on: its properties are read with `w.read(item, ..)`
and written with `w.write(item, ..)`, its signals emitted with `crate::native::emit`. Being in
`ELEMENTS` also tells the build's type check (`typecheck.rs`) the type is implemented.

`Element` has three more fields, each `None` unless the type needs it; `..Element::new(name)`
fills the ones left out:

```rust
pub static ELEMENT: crate::impulse::Element = crate::impulse::Element {
    job: Some(job),
    ..crate::impulse::Element::new("Download")
};
```

- `job: Option<fn(&mut World, usize) -> Option<crate::jobs::Job>>`: the element is a `Process`
  whose work is a Rust function instead of a command. Its `.qmltypes` entry says
  `prototype: "Process"` and the QML file imports `Rustshell.Io`, so it has `running`,
  `stdout`, `stderr`, `onExited` and `signal()` as any `Process`. When `running` becomes true
  the function is called on the main thread with the instance, reads the job's inputs from its
  properties and returns the job; `None` starts nothing and `running` goes back to false. The
  job runs on a pool thread: what it writes with `pipes.out(..)` / `pipes.err(..)` arrives
  through `stdout` / `stderr`, its return value is the `exitCode` of `exited`, and
  `running: false` sets its `Stop`. Under a spawn hook (`native::set_spawn_hook`: tests and
  aotcheck, where commands do not run) the function is not called and no job starts: the hook
  gets the type's name followed by the values of its declared properties in `.qmltypes` order
  (the items of a list in place) and answers as for a command, its text arriving through
  `stdout` with code 0, or code 1 and no output if it has no answer.
- `running: Option<fn(&mut World, usize, bool)>`: for an element that has a `running` property
  of its own and is not a `Process` (an element with `job` leaves it `None`). Called with the
  value on every write of the property, also one that does not change it, and once at component
  completion if the element was created with `running: true`.
- `created: Option<fn(&mut World, usize)>`: called when an instance is created, before QML
  assigns its properties.

## An answer from a thread

A type that starts a thread (a D-Bus call, a file read) gets the way back from
`crate::impulse::replies(w)` on the main thread and moves it into the thread. It is `Clone` and
`Send`:

```rust
let back = crate::impulse::replies(w);
std::thread::spawn(move || {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    back.send(move |w| {
        let id = w.singletons["Files"];
        crate::native::emit(w, id, "answered", &["text"], vec![Value::str(&text)]);
    });
});
```

`send` wakes the shell's event loop, which runs the closure on the main thread at its next
turn; closures run in the order they were sent. The closure is the only place the thread's
result touches the `World`: it writes properties and emits signals there. A reply to an element
rather than a singleton checks `w.dead.contains(&item)` first, since the element may have been
destroyed meanwhile.

## Names

`Registry::resolve` finds a type by its exported name alone, whatever the file imports, and
the `Impulse` module comes first: an `Impulse` type named as one of Quickshell's or Qt's
(`Network`) replaces that one for the whole tree. So a name must not repeat a Qt or Quickshell
type the tree still uses (`Translate` is QtQuick's), nor a `.qml` file of the tree.
