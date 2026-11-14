import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Effects
import QtQuick.Shapes
Item {
    id: root
    width: 200; height: 200
    property string out: ""
    property bool flag: true
    Item { id: other; width: 11; height: 13 }
    Component { id: comp; Item {} }
    TextField {
        id: c
        hoverEnabled: undefined
        topInset: undefined
        leftInset: undefined
        rightInset: undefined
        bottomInset: undefined
        horizontalAlignment: undefined
        passwordMaskDelay: undefined
        padding: undefined
        topPadding: undefined
        leftPadding: undefined
        rightPadding: undefined
        bottomPadding: undefined
        width: undefined
        height: undefined
        palette: undefined
        anchors.left: undefined
        anchors.right: undefined
        anchors.horizontalCenter: undefined
        anchors.top: undefined
        anchors.bottom: undefined
        anchors.verticalCenter: undefined
        anchors.baseline: undefined
        anchors.leftMargin: undefined
        anchors.rightMargin: undefined
        anchors.topMargin: undefined
        anchors.bottomMargin: undefined
        anchors.fill: undefined
        anchors.centerIn: undefined
        antialiasing: undefined
    }
    TextField { id: a0 }
    TextField { id: b0; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a1 }
    TextField { id: b1; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a2 }
    TextField { id: b2; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a3 }
    TextField { id: b3; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a4 }
    TextField { id: b4; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a5 }
    TextField { id: b5; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a6 }
    TextField { id: b6; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a7 }
    TextField { id: b7; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a8 }
    TextField { id: b8; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a9 }
    TextField { id: b9; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a10 }
    TextField { id: b10; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a11 }
    TextField { id: b11; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a12 }
    TextField { id: b12; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a13 }
    TextField { id: b13; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a14 }
    TextField { id: b14; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a15 }
    TextField { id: b15; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a16 }
    TextField { id: b16; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a17 }
    TextField { id: b17; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a18 }
    TextField { id: b18; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a19 }
    TextField { id: b19; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a20 }
    TextField { id: b20; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a21 }
    TextField { id: b21; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a22 }
    TextField { id: b22; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a23 }
    TextField { id: b23; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a24 }
    TextField { id: b24; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a25 }
    TextField { id: b25; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a26 }
    TextField { id: b26; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a27 }
    TextField { id: b27; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    TextField { id: a28 }
    TextField { id: b28; padding: 5; implicitWidth: 31; implicitHeight: 32; text: "preset"; anchors.margins: 11 }
    function show(v) {
        if (v === undefined) return "undefined";
        if (v === null) return "null";
        if (v === other) return "<other>";
        if (v === comp) return "<comp>";
        let t = typeof v;
        if (t === "number") return String(Math.round(v * 1000) / 1000);
        if (t === "string") return JSON.stringify(v);
        if (t === "boolean") return String(v);
        let s;
        try { s = String(v) } catch (e) { s = "?" + e.message }
        return s.replace(/0x[0-9a-f]+/g, "PTR").replace(/_QML(TYPE)?_[0-9]+/g, "");
    }
    function geo(o, kind) {
        if (kind !== "line" && kind !== "item" || o.x === undefined) return "";
        return "@" + [o.x, o.y, o.width, o.height].join(",");
    }
    function get(o, path) {
        const top = o;
        try {
            for (const k of path.split(".")) o = o[k];
            return show(o) + geo(top, root.kind);
        } catch (e) { return "!" + e.message }
    }
    function set(o, path, v) {
        const ks = path.split(".");
        try {
            for (let i = 0; i + 1 < ks.length; i++) o = o[ks[i]];
            o[ks[ks.length - 1]] = v;
            return "";
        } catch (e) { return " !" + e.message }
    }
    function raw(o, path) {
        for (const k of path.split(".")) o = o[k];
        return o;
    }
    function alt(o, path, kind) {
        const d = raw(o, path);
        switch (kind) {
        case "num": return (typeof d === "number" && isFinite(d) ? d : 0) + 3;
        case "enum": return (typeof d === "number" ? d : 0) + 1;
        case "bool": return !d;
        case "str": return String(d === undefined ? "" : d) + "x";
        case "color": return "#123456";
        case "item": return other;
        case "line": return other[path.split(".").pop()];
        case "size": return Qt.size(3, 4);
        case "rect": return Qt.rect(1, 2, 3, 4);
        case "url": return "x.png";
        case "comp": return comp;
        }
        return undefined;
    }
    property string kind: ""
    property var list: []
    function probe(o, tag, path, kind) {
        list.push({ o: o, tag: tag, path: path, kind: kind, l: "", e: "", v: undefined, c: null });
    }
    function cline(path) { list.push({ c: path }); }
    // k even: writes of stage k/2; k odd: reads of stage (k-1)/2. Stages: 0 default, 1 set, 2 reset,
    // 3 bind, 4 flag false, 5 flag true, 6 reset.
    function step(k) {
        const st = Math.floor(k / 2), wr = k % 2 === 0;
        if (wr && st === 4) root.flag = false;
        if (wr && (st === 5 || st === 3)) root.flag = true;
        for (const p of list) {
            if (p.c !== null) continue;
            const skip = p.kind === "skip";
            if (skip && st !== 0 && st !== 2) continue;
            root.kind = p.path.startsWith("anchors.") ? p.kind : "";
            if (wr) {
                if (st === 1) p.e = set(p.o, p.path, alt(p.o, p.path, p.kind));
                else if (st === 2 || st === 6) p.e = set(p.o, p.path, undefined);
                else if (st === 3) { const v = alt(p.o, p.path, p.kind); p.e = set(p.o, p.path, Qt.binding(() => root.flag ? v : undefined)); }
            } else {
                const g = get(p.o, p.path);
                if (st === 0) p.l = p.tag + "." + p.path + " [" + p.kind + "]: " + g;
                else if (st === 1) p.l += " | set" + p.e + " " + g;
                else if (st === 2 || st === 6) p.l += " | reset" + p.e + " " + g;
                else if (st === 3) p.l += " | bind" + p.e + " " + g;
                else p.l += " -> " + g;
            }
        }
        root.kind = "";
    }
    function fin() {
        let s = "";
        for (const p of list) s += (p.c !== null ? "c." + p.c + ": " + get(c, p.c) : p.l) + "\n";
        root.out = s;
    }
    function go() {
        probe(a0, "a", "hoverEnabled", "bool"); probe(b0, "b", "hoverEnabled", "bool"); cline("hoverEnabled");
        probe(a1, "a", "topInset", "num"); probe(b1, "b", "topInset", "num"); cline("topInset");
        probe(a2, "a", "leftInset", "num"); probe(b2, "b", "leftInset", "num"); cline("leftInset");
        probe(a3, "a", "rightInset", "num"); probe(b3, "b", "rightInset", "num"); cline("rightInset");
        probe(a4, "a", "bottomInset", "num"); probe(b4, "b", "bottomInset", "num"); cline("bottomInset");
        probe(a5, "a", "horizontalAlignment", "enum"); probe(b5, "b", "horizontalAlignment", "enum"); cline("horizontalAlignment");
        probe(a6, "a", "passwordMaskDelay", "num"); probe(b6, "b", "passwordMaskDelay", "num"); cline("passwordMaskDelay");
        probe(a7, "a", "padding", "num"); probe(b7, "b", "padding", "num"); cline("padding");
        probe(a8, "a", "topPadding", "num"); probe(b8, "b", "topPadding", "num"); cline("topPadding");
        probe(a9, "a", "leftPadding", "num"); probe(b9, "b", "leftPadding", "num"); cline("leftPadding");
        probe(a10, "a", "rightPadding", "num"); probe(b10, "b", "rightPadding", "num"); cline("rightPadding");
        probe(a11, "a", "bottomPadding", "num"); probe(b11, "b", "bottomPadding", "num"); cline("bottomPadding");
        probe(a12, "a", "width", "num"); probe(b12, "b", "width", "num"); cline("width");
        probe(a13, "a", "height", "num"); probe(b13, "b", "height", "num"); cline("height");
        probe(a14, "a", "palette", "skip"); probe(b14, "b", "palette", "skip"); cline("palette");
        probe(a15, "a", "anchors.left", "line"); probe(b15, "b", "anchors.left", "line"); cline("anchors.left");
        probe(a16, "a", "anchors.right", "line"); probe(b16, "b", "anchors.right", "line"); cline("anchors.right");
        probe(a17, "a", "anchors.horizontalCenter", "line"); probe(b17, "b", "anchors.horizontalCenter", "line"); cline("anchors.horizontalCenter");
        probe(a18, "a", "anchors.top", "line"); probe(b18, "b", "anchors.top", "line"); cline("anchors.top");
        probe(a19, "a", "anchors.bottom", "line"); probe(b19, "b", "anchors.bottom", "line"); cline("anchors.bottom");
        probe(a20, "a", "anchors.verticalCenter", "line"); probe(b20, "b", "anchors.verticalCenter", "line"); cline("anchors.verticalCenter");
        probe(a21, "a", "anchors.baseline", "line"); probe(b21, "b", "anchors.baseline", "line"); cline("anchors.baseline");
        probe(a22, "a", "anchors.leftMargin", "num"); probe(b22, "b", "anchors.leftMargin", "num"); cline("anchors.leftMargin");
        probe(a23, "a", "anchors.rightMargin", "num"); probe(b23, "b", "anchors.rightMargin", "num"); cline("anchors.rightMargin");
        probe(a24, "a", "anchors.topMargin", "num"); probe(b24, "b", "anchors.topMargin", "num"); cline("anchors.topMargin");
        probe(a25, "a", "anchors.bottomMargin", "num"); probe(b25, "b", "anchors.bottomMargin", "num"); cline("anchors.bottomMargin");
        probe(a26, "a", "anchors.fill", "item"); probe(b26, "b", "anchors.fill", "item"); cline("anchors.fill");
        probe(a27, "a", "anchors.centerIn", "item"); probe(b27, "b", "anchors.centerIn", "item"); cline("anchors.centerIn");
        probe(a28, "a", "antialiasing", "bool"); probe(b28, "b", "antialiasing", "bool"); cline("antialiasing");
    }
}
