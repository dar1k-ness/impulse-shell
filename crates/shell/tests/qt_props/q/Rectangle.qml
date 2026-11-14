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
    Rectangle {
        id: c
        color: undefined
        gradient: undefined
        topLeftRadius: undefined
        topRightRadius: undefined
        bottomLeftRadius: undefined
        bottomRightRadius: undefined
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
    Rectangle { id: a0 }
    Rectangle { id: b0; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a1 }
    Rectangle { id: b1; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a2 }
    Rectangle { id: b2; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a3 }
    Rectangle { id: b3; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a4 }
    Rectangle { id: b4; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a5 }
    Rectangle { id: b5; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a6 }
    Rectangle { id: b6; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a7 }
    Rectangle { id: b7; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a8 }
    Rectangle { id: b8; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a9 }
    Rectangle { id: b9; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a10 }
    Rectangle { id: b10; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a11 }
    Rectangle { id: b11; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a12 }
    Rectangle { id: b12; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a13 }
    Rectangle { id: b13; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a14 }
    Rectangle { id: b14; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a15 }
    Rectangle { id: b15; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a16 }
    Rectangle { id: b16; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a17 }
    Rectangle { id: b17; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a18 }
    Rectangle { id: b18; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a19 }
    Rectangle { id: b19; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a20 }
    Rectangle { id: b20; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a21 }
    Rectangle { id: b21; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    Rectangle { id: a22 }
    Rectangle { id: b22; radius: 7; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
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
        probe(a0, "a", "color", "color"); probe(b0, "b", "color", "color"); cline("color");
        probe(a1, "a", "gradient", "skip"); probe(b1, "b", "gradient", "skip"); cline("gradient");
        probe(a2, "a", "topLeftRadius", "num"); probe(b2, "b", "topLeftRadius", "num"); cline("topLeftRadius");
        probe(a3, "a", "topRightRadius", "num"); probe(b3, "b", "topRightRadius", "num"); cline("topRightRadius");
        probe(a4, "a", "bottomLeftRadius", "num"); probe(b4, "b", "bottomLeftRadius", "num"); cline("bottomLeftRadius");
        probe(a5, "a", "bottomRightRadius", "num"); probe(b5, "b", "bottomRightRadius", "num"); cline("bottomRightRadius");
        probe(a6, "a", "width", "num"); probe(b6, "b", "width", "num"); cline("width");
        probe(a7, "a", "height", "num"); probe(b7, "b", "height", "num"); cline("height");
        probe(a8, "a", "palette", "skip"); probe(b8, "b", "palette", "skip"); cline("palette");
        probe(a9, "a", "anchors.left", "line"); probe(b9, "b", "anchors.left", "line"); cline("anchors.left");
        probe(a10, "a", "anchors.right", "line"); probe(b10, "b", "anchors.right", "line"); cline("anchors.right");
        probe(a11, "a", "anchors.horizontalCenter", "line"); probe(b11, "b", "anchors.horizontalCenter", "line"); cline("anchors.horizontalCenter");
        probe(a12, "a", "anchors.top", "line"); probe(b12, "b", "anchors.top", "line"); cline("anchors.top");
        probe(a13, "a", "anchors.bottom", "line"); probe(b13, "b", "anchors.bottom", "line"); cline("anchors.bottom");
        probe(a14, "a", "anchors.verticalCenter", "line"); probe(b14, "b", "anchors.verticalCenter", "line"); cline("anchors.verticalCenter");
        probe(a15, "a", "anchors.baseline", "line"); probe(b15, "b", "anchors.baseline", "line"); cline("anchors.baseline");
        probe(a16, "a", "anchors.leftMargin", "num"); probe(b16, "b", "anchors.leftMargin", "num"); cline("anchors.leftMargin");
        probe(a17, "a", "anchors.rightMargin", "num"); probe(b17, "b", "anchors.rightMargin", "num"); cline("anchors.rightMargin");
        probe(a18, "a", "anchors.topMargin", "num"); probe(b18, "b", "anchors.topMargin", "num"); cline("anchors.topMargin");
        probe(a19, "a", "anchors.bottomMargin", "num"); probe(b19, "b", "anchors.bottomMargin", "num"); cline("anchors.bottomMargin");
        probe(a20, "a", "anchors.fill", "item"); probe(b20, "b", "anchors.fill", "item"); cline("anchors.fill");
        probe(a21, "a", "anchors.centerIn", "item"); probe(b21, "b", "anchors.centerIn", "item"); cline("anchors.centerIn");
        probe(a22, "a", "antialiasing", "bool"); probe(b22, "b", "antialiasing", "bool"); cline("antialiasing");
    }
}
