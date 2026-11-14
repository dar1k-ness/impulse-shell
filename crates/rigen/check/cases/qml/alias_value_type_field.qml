// An alias to a property of a value type property (`t.font.pixelSize`, `img.sourceSize.width`,
// the properties of a `point`, `size`, `rect`, `color` and `font` property of one's own) shares
// that property's change signal, as in Qt 6.12: it is notified once for each signal the
// property sends, after it, and never for a write of the same value. A write of a property of
// the value, of the whole value and through the alias each send the one signal, and what is
// bound to the alias follows. An alias to a property of a grouped object (`border.width`,
// `anchors.margins`) has that property's own signal.
// Each step: the signals it sent, in order, then its value.
// An Image loads again for another `sourceSize` and sends `sourceSizeChanged` once more as
// the load ends (`image_source_size.qml`): two signals for one write, the alias follows both.
// qt: font tf fb fpx bfpx=[14,14];=14;font tf fb fpx=[14,true];font tf fb fpx bfpx=[20,20];=20;font tf fb fpx bfpx=false;=false;=false;ss ssw ss ssw=10;=10;ss ssw ss ssw=10;ss ssw ss ssw=3;=3;ss ssw ss ssw=8;bw bbw=[3,3];=3;=3;bw bbw=[4,4];am=2;=2;=2;am=5;color rcol="#ff0000";="#ff0000";pt pta ptx bptx=[5,5];=5;pt pta ptx=[5,5];pt pta ptx bptx=[8,8];=8;=8;pt pta ptx bptx=[9,9];=9;=9;pt pta ptx bptx=[3,3];sz szw=6;sz szw=6;=6;sz szw=7;rc rcx=6;rc rcx=6;=6;rc rcx=7;fnt fnpx=14;=14;fnt fnpx=14;fnt fnpx=20;=20;col colr bcolr=[0.5000076293945312,0.5000076293945312];col colr=0.5000076293945312;col colr bcolr=16;=16;col colr bcolr=["#ff2030",1];="#ff2030";
import QtQuick
Item {
    id: root
    property string result: ""
    property string log: ""
    Text { id: t; font.pixelSize: 11; onFontChanged: root.log += "font " }
    Image { id: img; sourceSize.width: 1; sourceSize.height: 2; onSourceSizeChanged: root.log += "ss " }
    Rectangle { id: r; onColorChanged: root.log += "color " }
    QtObject {
        id: o
        property point pt: Qt.point(1, 2)
        onPtChanged: root.log += "pt "
        property size sz: Qt.size(1, 2)
        onSzChanged: root.log += "sz "
        property rect rc: Qt.rect(1, 2, 3, 4)
        onRcChanged: root.log += "rc "
        property color col: "#102030"
        onColChanged: root.log += "col "
        property font fnt
        onFntChanged: root.log += "fnt "
    }
    property alias fpx: t.font.pixelSize
    onFpxChanged: log += "fpx "
    property alias fb: t.font.bold
    onFbChanged: log += "fb "
    property alias tf: t.font
    onTfChanged: log += "tf "
    property int bfpx: fpx
    onBfpxChanged: log += "bfpx "
    property alias ssw: img.sourceSize.width
    onSswChanged: log += "ssw "
    property alias bw: r.border.width
    onBwChanged: log += "bw "
    property alias am: r.anchors.margins
    onAmChanged: log += "am "
    property alias rcol: r.color
    onRcolChanged: log += "rcol "
    property int bbw: bw
    onBbwChanged: log += "bbw "
    property alias ptx: o.pt.x
    onPtxChanged: log += "ptx "
    property alias pta: o.pt
    onPtaChanged: log += "pta "
    property alias szw: o.sz.width
    onSzwChanged: log += "szw "
    property alias rcx: o.rc.x
    onRcxChanged: log += "rcx "
    property alias fnpx: o.fnt.pixelSize
    onFnpxChanged: log += "fnpx "
    property real bptx: ptx
    onBptxChanged: log += "bptx "
    property alias colr: o.col.r
    onColrChanged: log += "colr "
    property real bcolr: colr
    onBcolrChanged: log += "bcolr "
    function step(f) {
        log = ""
        var r = f()
        result += log.trim() + "=" + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step(() => { t.font.pixelSize = 14; return [fpx, bfpx] })
        step(() => { t.font.pixelSize = 14; return fpx })
        step(() => { t.font.bold = true; return [fpx, fb] })
        step(() => { fpx = 20; return [t.font.pixelSize, bfpx] })
        step(() => { fpx = 20; return t.font.pixelSize })
        step(() => { t.font = o.fnt; return fb })
        step(() => { t.font = o.fnt; return fb })
        step(() => { tf = t.font; return fb })
        step(() => { img.sourceSize.width = 10; return ssw })
        step(() => { img.sourceSize.width = 10; return ssw })
        step(() => { img.sourceSize.height = 5; return ssw })
        step(() => { img.sourceSize = Qt.size(3, 4); return ssw })
        step(() => { img.sourceSize = Qt.size(3, 4); return ssw })
        step(() => { ssw = 8; return img.sourceSize.width })
        step(() => { r.border.width = 3; return [bw, bbw] })
        step(() => { r.border.width = 3; return bw })
        step(() => { r.border.color = "red"; return bw })
        step(() => { bw = 4; return [r.border.width, bbw] })
        step(() => { r.anchors.margins = 2; return am })
        step(() => { r.anchors.leftMargin = 1; return am })
        step(() => { am = 2; return r.anchors.margins })
        step(() => { am = 5; return r.anchors.margins })
        step(() => { r.color = "red"; return "" + rcol })
        step(() => { rcol = "red"; return "" + r.color })
        step(() => { o.pt.x = 5; return [ptx, bptx] })
        step(() => { o.pt.x = 5; return ptx })
        step(() => { o.pt.y = 7; return [ptx, bptx] })
        step(() => { o.pt = Qt.point(8, 7); return [ptx, bptx] })
        step(() => { o.pt = Qt.point(8, 7); return ptx })
        step(() => { o.pt = o.pt; return ptx })
        step(() => { ptx = 9; return [o.pt.x, bptx] })
        step(() => { ptx = 9; return o.pt.x })
        step(() => { pta = Qt.point(9, 7); return ptx })
        step(() => { pta = Qt.point(3, 7); return [ptx, bptx] })
        step(() => { o.sz.width = 6; return szw })
        step(() => { o.sz.height = 6; return szw })
        step(() => { o.sz = Qt.size(6, 6); return szw })
        step(() => { szw = 7; return o.sz.width })
        step(() => { o.rc.x = 6; return rcx })
        step(() => { o.rc.width = 6; return rcx })
        step(() => { rcx = 6; return o.rc.x })
        step(() => { rcx = 7; return o.rc.x })
        step(() => { o.fnt.pixelSize = 14; return fnpx })
        step(() => { o.fnt.pixelSize = 14; return fnpx })
        step(() => { o.fnt.bold = true; return fnpx })
        step(() => { fnpx = 20; return o.fnt.pixelSize })
        step(() => { fnpx = 20; return o.fnt.pixelSize })
        step(() => { o.col.r = 0.5; return [colr, bcolr] })
        step(() => { o.col.g = 0.5; return colr })
        step(() => { o.col = "#102030"; return Math.round(colr * 255) })
        step(() => { o.col = "#102030"; return Math.round(colr * 255) })
        step(() => { colr = 1; return ["" + o.col, bcolr] })
        step(() => { colr = 1; return "" + o.col })
    }
}
