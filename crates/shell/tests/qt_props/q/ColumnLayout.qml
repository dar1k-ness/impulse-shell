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
    ColumnLayout {
        id: c
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
    ColumnLayout { id: a0 }
    ColumnLayout { id: b0; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a1 }
    ColumnLayout { id: b1; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a2 }
    ColumnLayout { id: b2; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a3 }
    ColumnLayout { id: b3; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a4 }
    ColumnLayout { id: b4; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a5 }
    ColumnLayout { id: b5; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a6 }
    ColumnLayout { id: b6; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a7 }
    ColumnLayout { id: b7; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a8 }
    ColumnLayout { id: b8; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a9 }
    ColumnLayout { id: b9; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a10 }
    ColumnLayout { id: b10; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a11 }
    ColumnLayout { id: b11; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a12 }
    ColumnLayout { id: b12; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a13 }
    ColumnLayout { id: b13; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a14 }
    ColumnLayout { id: b14; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a15 }
    ColumnLayout { id: b15; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
    ColumnLayout { id: a16 }
    ColumnLayout { id: b16; spacing: 4; implicitWidth: 31; implicitHeight: 32; anchors.margins: 11 }
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
        probe(a0, "a", "width", "num"); probe(b0, "b", "width", "num"); cline("width");
        probe(a1, "a", "height", "num"); probe(b1, "b", "height", "num"); cline("height");
        probe(a2, "a", "palette", "skip"); probe(b2, "b", "palette", "skip"); cline("palette");
        probe(a3, "a", "anchors.left", "line"); probe(b3, "b", "anchors.left", "line"); cline("anchors.left");
        probe(a4, "a", "anchors.right", "line"); probe(b4, "b", "anchors.right", "line"); cline("anchors.right");
        probe(a5, "a", "anchors.horizontalCenter", "line"); probe(b5, "b", "anchors.horizontalCenter", "line"); cline("anchors.horizontalCenter");
        probe(a6, "a", "anchors.top", "line"); probe(b6, "b", "anchors.top", "line"); cline("anchors.top");
        probe(a7, "a", "anchors.bottom", "line"); probe(b7, "b", "anchors.bottom", "line"); cline("anchors.bottom");
        probe(a8, "a", "anchors.verticalCenter", "line"); probe(b8, "b", "anchors.verticalCenter", "line"); cline("anchors.verticalCenter");
        probe(a9, "a", "anchors.baseline", "line"); probe(b9, "b", "anchors.baseline", "line"); cline("anchors.baseline");
        probe(a10, "a", "anchors.leftMargin", "num"); probe(b10, "b", "anchors.leftMargin", "num"); cline("anchors.leftMargin");
        probe(a11, "a", "anchors.rightMargin", "num"); probe(b11, "b", "anchors.rightMargin", "num"); cline("anchors.rightMargin");
        probe(a12, "a", "anchors.topMargin", "num"); probe(b12, "b", "anchors.topMargin", "num"); cline("anchors.topMargin");
        probe(a13, "a", "anchors.bottomMargin", "num"); probe(b13, "b", "anchors.bottomMargin", "num"); cline("anchors.bottomMargin");
        probe(a14, "a", "anchors.fill", "item"); probe(b14, "b", "anchors.fill", "item"); cline("anchors.fill");
        probe(a15, "a", "anchors.centerIn", "item"); probe(b15, "b", "anchors.centerIn", "item"); cline("anchors.centerIn");
        probe(a16, "a", "antialiasing", "bool"); probe(b16, "b", "antialiasing", "bool"); cline("antialiasing");
    }
}
