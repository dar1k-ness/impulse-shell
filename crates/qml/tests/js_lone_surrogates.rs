//! JS strings are sequences of UTF-16 code units (ECMA-262 §6.1.4), so QML code keeps an
//! unpaired surrogate like QV4's `QString` does: Encode throws URIError on one (§19.2.6.5),
//! JSON.parse keeps a `\uD800` escape (§25.5.1) and JSON.stringify writes it as an escape
//! (well-formed stringify, §25.5.2.3; QV4 6.12 copies it raw, the spec value is pinned here).
//! Other expected values are Qt 6.12's (PySide6 probe), checked against ECMA-262 by hand.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

fn run(body: &str) -> String {
    let mut rt = Runtime::new(None);
    let src = format!(
        r#"import QtQuick
Item {{
    id: root
    property string held: "a\ud800"
    property var any: String.fromCharCode(0xdc00)
    function run() {{
        function t(f) {{
            try {{
                var v = f();
                return typeof v === "string" ? escape(v) : String(v);
            }} catch (e) {{
                return "throw " + e.name;
            }}
        }}
        return [{body}].join(" ");
    }}
}}"#
    );
    let root = rt.load_str(&src).unwrap();
    rt.call_method(root, "run", vec![])
        .unwrap()
        .unwrap()
        .to_js_string()
}

#[test]
fn encode_uri_throws_uri_error_on_a_lone_surrogate() {
    assert_eq!(
        run(r#"t(function () { return encodeURI("\uDC00") }),
            t(function () { return encodeURIComponent("a\uD800b") }),
            t(function () { return encodeURI("\uD800\uDC00") }),
            t(function () { return encodeURIComponent("\ud83d" + "\ude00") })"#),
        "throw URIError throw URIError %25F0%2590%2580%2580 %25F0%259F%2598%2580"
    );
}

#[test]
fn json_stringify_escapes_lone_surrogates() {
    assert_eq!(
        run(r#"JSON.stringify("\uDEAD") === '"\\udead"',
            JSON.stringify(["\udc00x\ud800"]) === '["\\udc00x\\ud800"]',
            JSON.stringify({ k: "\ud83d" + "\ude00" }) === '{"k":"\ud83d\ude00"}',
            JSON.stringify(held) === '"a\\ud800"'"#),
        "true true true true"
    );
}

#[test]
fn json_parse_keeps_lone_surrogate_escapes() {
    assert_eq!(
        run(r#"JSON.parse('"\\ud800"').charCodeAt(0),
            JSON.parse('"\\ud800"').length,
            JSON.parse('"\\ud800\\udc00"') === "\ud800\udc00",
            t(function () { return JSON.parse('[{"v":"x\\uDFFFy"}]')[0].v })"#),
        "55296 1 true x%uDFFFy"
    );
}

#[test]
fn qml_properties_and_string_ops_keep_lone_surrogates() {
    assert_eq!(
        run(r#"held.charCodeAt(1), held.length, any.charCodeAt(0),
            t(function () { return held + any }),
            ("\ud83d" + "\ude00") === "\ud83d\ude00",
            "\ud800" == "\ufffd",
            t(function () { return "\ud83d\ude00".charAt(1) + `${"\ud800"}` })"#),
        "55296 2 56320 a%uD800%uDC00 true false %uDE00%uD800"
    );
}

/// String iteration is by code point (ECMA-262 §22.1.5.1): a pair is one element, an unpaired
/// surrogate one of its own (QV4 6.12 splits the pair in `Array.from`; the spec value is pinned).
#[test]
fn string_iteration_yields_lone_surrogates_and_pairs_whole() {
    assert_eq!(
        run(
            r#"Array.from("a\udc00\ud83d\ude00").map(function (c) { return escape(c) }).join(),
            [..."\ud800x"].length"#
        ),
        "a,%uDC00,%uD83D%uDE00 2"
    );
}

/// QQuickTextInput/QQuickTextEdit positions are QString indices: a position may split a pair,
/// whose halves selectedText, getText, insert and the maximumLength cut (`QString::left`) then
/// keep apart (Qt 6.12 PySide6 probe).
#[test]
fn text_editors_keep_positions_inside_a_pair() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            r#"import QtQuick
Item {
    TextInput { id: a; maximumLength: 1; text: "\ud83d\ude00" }
    TextInput { id: b; text: "a\ud800" }
    TextInput { id: c; text: "\ud83d\ude00b" }
    TextInput { id: d; text: "\ud83d\ude00" }
    TextEdit { id: f; text: "\ud83d\ude00b" }
    TextInput { id: g; text: "ab"; maximumLength: 5 }
    function run() {
        var r = [];
        r.push("maxlen1:" + a.text.length + ":" + escape(a.text));
        b.selectAll(); r.push("lone:" + escape(b.text) + ":" + escape(b.selectedText) + ":" + b.text.length);
        c.cursorPosition = 1; r.push("cur1:" + c.cursorPosition);
        c.select(0, 1); r.push("sel01:" + escape(c.selectedText) + ":" + c.selectionStart + "-" + c.selectionEnd);
        d.insert(1, "x"); r.push("ins:" + escape(d.text));
        f.cursorPosition = 1; r.push("te_cur1:" + f.cursorPosition);
        f.select(0, 1); r.push("te_sel01:" + escape(f.selectedText) + ":" + f.selectionStart + "-" + f.selectionEnd);
        r.push("te_get:" + escape(f.getText(0, 1)));
        g.text = "\ud83d\ude00\ud83d\ude00\ud83d\ude00"; r.push("maxlen5:" + escape(g.text));
        return r.join(" ");
    }
}"#,
        )
        .unwrap();
    rt.settle();
    assert_eq!(
        rt.call_method(root, "run", vec![])
            .unwrap()
            .unwrap()
            .to_js_string(),
        "maxlen1:1:%uD83D lone:a%uD800:a%uD800:2 cur1:1 sel01:%uD83D:0-1 ins:%uD83Dx%uDE00 \
         te_cur1:1 te_sel01:%uD83D:0-1 te_get:%uD83D \
         maxlen5:%uD83D%uDE00%uD83D%uDE00%uD83D"
    );
}
