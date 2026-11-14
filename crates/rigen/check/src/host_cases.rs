// QML-scope cases for `host.rs`, shared with build.rs (which compiles them with rigen).
// Mock world (ctx 0): item 0 `root` {w: 3, h: 4, name: "r", list: [1, 2], model: {arr: [7]}},
// item 1 `child` {x: 10, list2: [], size: 5 (read-only)}; natives root.reload(..), root.sum(a, b), child.describe(),
// signal root.fire(a, b) (throws `Insufficient arguments` with fewer than 2 like QV4).

/// (item, source) of QML `function` members installed on the mock items.
pub const HOST_FUNCS: &[(usize, &str)] = &[
    (0, "function area() { return this.w * this.h }"),
    (0, "function self() { return this }"),
    (0, "function arrowThis() { const f = () => this.w; return f() }"),
    (0, "function freeArea() { return w * h }"),
    (0, "function setW(v) { w = v; return w }"),
    (1, "function bump(n) { x = x + n; return x }"),
    (1, "function getX() { return x + root.w }"),
    (1, "function addAll(...xs) { for (const v of xs) list2.push(v); return list2.length }"),
    (1, "function nested() { function inner() { return this } return inner() === undefined }"),
    (1, "function callsArea() { return root.area() + area2() }"),
    (1, "function area2() { return this === child ? 1 : 0 }"),
];

/// (name, kind, scope item, source, handler params). Handler args are 2, 3, "s".
pub const HOST_CASES: &[(&str, &str, usize, &str, &[&str])] = &[
    // free-name lookup
    ("lookup_props", "binding", 0, "w * h + child.x", &[]),
    ("lookup_ids_and_root", "binding", 1, "x + w + root.name + child.x", &[]),
    ("lookup_typeof", "binding", 0, "typeof nope + typeof w + typeof child + typeof Math", &[]),
    ("lookup_missing", "binding", 0, "nope + 1", &[]),
    ("lookup_global_first", "binding", 0, "Math.max(w, h) + parseInt('5')", &[]),
    ("lookup_block", "binding", 1, "{ var t = x * 2; if (t > 5) return t + w; return 0 }", &[]),
    ("lookup_closure", "binding", 0, "[1, 2].map(v => v * w).concat([h])", &[]),
    ("lookup_catch", "binding", 0, "{ try { return nope } catch (e) { return e.name + ':' + e.message } }", &[]),
    ("lookup_nullish", "binding", 0, "model?.arr?.[0] ?? w", &[]),
    // free-name assignment
    ("assign_props", "handler", 0, "w = 5; h = w * 2", &[]),
    ("assign_root_from_child", "handler", 1, "w = 8; x = 1", &[]),
    ("assign_compound", "handler", 0, "w += 1; h++; ++h; w *= h", &[]),
    ("assign_global", "handler", 0, "{ undeclaredGlobal = 3; undeclaredGlobal += w; }", &[]),
    ("assign_member", "handler", 0, "child.x = 42; root.name = 'q'", &[]),
    ("assign_read_only", "handler", 1, "{ var r = ''; for (const f of [() => { size = 1 }, () => { child.size = 2 }, () => { 'use strict'; child.size++ }, () => { size += 1 }, () => Reflect.set(child, 'size', 3)]) { try { f(); r += 'ok;' } catch (e) { r += e.name + ':' + e.message + ';' } } x = r + size }", &[]),
    ("assign_local_shadow", "handler", 0, "{ var w = 100; w = 101; h = w }", &[]),
    ("assign_logical", "handler", 0, "{ w ||= 9; name &&= 'z'; nothing ??= 1 }", &[]),
    ("assign_in_closure", "handler", 0, "{ const f = () => { w = 11 }; f(); h = w }", &[]),
    // host.call
    ("call_free_native", "binding", 0, "reload(1, 2)", &[]),
    ("call_member_native", "binding", 0, "root.reload() + child.describe('a')", &[]),
    ("call_native_args", "binding", 0, "sum(w, h) + root.sum(1, 2)", &[]),
    ("call_native_from_child", "binding", 1, "describe(x)", &[]),
    ("call_missing", "binding", 0, "{ try { return nosuch() } catch (e) { return e.message } }", &[]),
    ("call_missing_member", "binding", 0, "{ try { return child.nosuch() } catch (e) { return e.message } }", &[]),
    ("call_throws", "binding", 0, "{ var r = ''; for (const f of [() => root.fire(1), () => fire(), () => root?.fire(1), () => root.fire(1, 2, 3)]) { try { f(); r += 'ok;' } catch (e) { r += e.name + ':' + e.message + ':' + (e instanceof Error) + ';' } } return r }", &[]),
    ("call_optional", "binding", 0, "child.nosuch?.() ?? 'none'", &[]),
    // ECMA-262 OptionalChain: a native callee exists (arguments run); a missing one skips them.
    ("call_optional_native_args", "binding", 0, "{ var n = 0; var r = root?.reload(n++, n++); return r + ':' + n }", &[]),
    ("call_optional_free_native", "binding", 0, "{ var n = 0; var r = reload?.(n++); return r + ':' + n }", &[]),
    ("call_optional_child_native", "binding", 0, "{ var n = 0; var r = child?.describe(n++); return r + ':' + n }", &[]),
    ("call_optional_missing_skips_args", "binding", 1, "{ var n = 0; var r = root?.nosuch(n++); return String(r) + ':' + n }", &[]),
    // array write-back
    ("wb_free_push", "handler", 0, "list.push(3)", &[]),
    ("wb_member_push", "handler", 0, "root.list.push(4)", &[]),
    ("wb_this_push", "handler", 0, "this.list.push(4)", &[]),
    ("wb_id_push", "handler", 1, "root.list.unshift(0); list2.push(x)", &[]),
    ("wb_local_alias", "handler", 0, "{ var l = root.list; l.push(5); l.reverse() }", &[]),
    ("wb_nested_owner", "handler", 0, "model.arr.push(8)", &[]),
    ("wb_index_set", "handler", 0, "{ list[0] = 9; root.list[1] = 8 }", &[]),
    ("wb_sort_splice", "handler", 0, "{ list.push(5, 0); list.sort((a, b) => b - a); list.splice(1, 1) }", &[]),
    ("wb_loop", "handler", 1, "{ for (var i = 0; i < 3; i++) list2.push(i * x) }", &[]),
    ("wb_optional", "handler", 0, "list?.push(1)", &[]),
    ("wb_nonmutating", "handler", 0, "{ var s = list.slice(); s.push(1); h = list.length + s.length }", &[]),
    // handler params
    ("params_read", "handler", 0, "w = a + b", &["a", "b"]),
    ("params_closure", "handler", 0, "{ h = [1].map(v => v + a)[0] + c.length }", &["a", "b", "c"]),
    ("params_shadow_prop", "handler", 0, "{ var r = w + a; w = r }", &["a", "w"]),
    ("params_arrow_handler", "handler", 0, "(a, b) => { w = a * b; h = this.w }", &["a", "b"]),
    ("params_function_handler", "handler", 1, "function(a) { x = a; return this === child }", &["a"]),
    // `this` for scope methods
    ("this_binding", "binding", 0, "this.w + (this === root)", &[]),
    ("this_free_call", "binding", 0, "area()", &[]),
    ("this_member_call", "binding", 1, "root.area() + root.freeArea()", &[]),
    ("this_self", "binding", 0, "(self() === root) + ':' + (root.self() === root) + ':' + (child.nested())", &[]),
    ("this_arrow", "binding", 0, "arrowThis()", &[]),
    ("this_detached", "binding", 0, "{ var f = area; try { return f() } catch (e) { return e.message } }", &[]),
    ("this_func_assign", "handler", 0, "h = setW(6) + w", &[]),
    ("this_child_methods", "handler", 1, "{ var n = bump(2) + child.bump(3); w = n + getX() }", &[]),
    ("this_rest_push", "binding", 0, "child.addAll(1, 2, 3)", &[]),
    ("this_cross", "binding", 1, "callsArea()", &[]),
    ("this_call_apply", "binding", 0, "area.call(child) + ':' + self.call(5)", &[]),
];
