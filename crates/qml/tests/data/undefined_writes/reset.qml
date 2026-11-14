import QtQuick
import QtQuick.Controls
import QtQuick.Effects
Item {
    id: root
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    function un(o, p) { try { o[p] = undefined; return "" } catch (e) { return "!" + e } }
    // Written by an assignment.
    RectangularShadow { id: s1; radius: 5; topLeftRadius: 2; topRightRadius: 3; bottomLeftRadius: 4; bottomRightRadius: 6; antialiasing: false }
    Grid { id: g1; rowSpacing: 3; columnSpacing: 4; spacing: 2 }
    SwipeView { id: v1; contentWidth: 41; contentHeight: 42 }
    TabBar { id: b1; contentWidth: 41; contentHeight: 42 }
    Control { id: c1; padding: 5; horizontalPadding: 8; verticalPadding: 9; leftPadding: 1; rightPadding: 2; topPadding: 3; bottomPadding: 4 }
    Control { id: c2; padding: 5; horizontalPadding: 8; verticalPadding: 9 }
    Control { id: c3; padding: 5; spacing: 7; leftPadding: 1 }
    Popup { id: p1; margins: 6; leftMargin: 1; topMargin: 2; closePolicy: Popup.NoAutoClose; padding: 5; topPadding: 3 }
    Popup { id: p2; margins: 6; leftMargin: 1 }
    ToolTip { id: p3; closePolicy: Popup.NoAutoClose }
    Button { id: f1; font.pixelSize: 21; font.bold: true }
    Popup { id: f2; font.pixelSize: 23 }
    Text { id: f3; font.pixelSize: 22; padding: 5; leftPadding: 1 }
    Column { id: f4; padding: 5; topPadding: 2 }
    Pane { id: pn1; contentWidth: 41; contentHeight: 42 }
    TextEdit { id: te1; baseUrl: "http://x/" }
    Text { id: tx1; baseUrl: "http://x/" }
    // Written by a binding that gives `undefined` after a value.
    property bool on: true
    RectangularShadow { id: s2; radius: 5; topLeftRadius: root.on ? 2 : undefined; bottomRightRadius: root.on ? 6 : undefined; antialiasing: root.on ? false : undefined }
    Grid { id: g2; spacing: 2; rowSpacing: root.on ? 3 : undefined }
    SwipeView { id: v2; contentWidth: root.on ? 41 : undefined }
    TabBar { id: b2; contentWidth: root.on ? 41 : undefined }
    Control { id: c4; padding: 5; horizontalPadding: root.on ? 8 : undefined; topPadding: root.on ? 3 : undefined }
    Control { id: c5; padding: root.on ? 5 : undefined; spacing: root.on ? 7 : undefined }
    Popup { id: p4; margins: root.on ? 6 : undefined; closePolicy: root.on ? Popup.NoAutoClose : undefined }
    Popup { id: p5; margins: 6; leftMargin: root.on ? 1 : undefined }
    TextEdit { id: te2; baseUrl: root.on ? "http://x/" : undefined }
    property string done
    Component.onCompleted: {
        var r = [];
        for (var p of ["topLeftRadius", "topRightRadius", "bottomLeftRadius", "bottomRightRadius", "antialiasing"])
            r.push(un(s1, p));
        r.push(un(g1, "rowSpacing"), un(g1, "columnSpacing"));
        r.push(un(v1, "contentWidth"), un(v1, "contentHeight"), un(b1, "contentWidth"), un(b1, "contentHeight"));
        for (var q of ["leftPadding", "rightPadding", "topPadding", "bottomPadding"])
            r.push(un(c1, q));
        r.push(un(c2, "horizontalPadding"), un(c2, "verticalPadding"));
        r.push(un(c3, "padding"), un(c3, "spacing"));
        r.push(un(p1, "leftMargin"), un(p1, "topMargin"), un(p1, "closePolicy"), un(p1, "topPadding"));
        r.push(un(p2, "margins"), un(p3, "closePolicy"));
        r.push(un(f1, "font"), un(f2, "font"), un(f3, "font"), un(f3, "leftPadding"), un(f4, "padding"));
        r.push(un(pn1, "contentWidth"), un(pn1, "contentHeight"));
        r.push(un(te1, "baseUrl"), un(tx1, "baseUrl"));
        root.on = false;
        done = r.join(",");
    }
    property int tick: 0
    Timer { interval: 1; running: true; onTriggered: root.tick++ }
    property var cases: [
        () => root.done,
        () => [s1.topLeftRadius, s1.topRightRadius, s1.bottomLeftRadius, s1.bottomRightRadius, s1.antialiasing].join(" "),
        () => [g1.rowSpacing, g1.columnSpacing].join(" "),
        () => [v1.contentWidth, v1.contentHeight].join(" "),
        () => [b1.contentWidth, b1.contentHeight].join(" "),
        () => [c1.leftPadding, c1.rightPadding, c1.topPadding, c1.bottomPadding].join(" "),
        () => [c2.horizontalPadding, c2.verticalPadding, c2.leftPadding, c2.topPadding].join(" "),
        () => [c3.padding, c3.spacing, c3.leftPadding, c3.rightPadding, c3.horizontalPadding].join(" "),
        () => [p1.leftMargin, p1.topMargin, p1.rightMargin, p1.closePolicy, p1.topPadding].join(" "),
        () => [p2.margins, p2.leftMargin, p2.rightMargin].join(" "),
        () => p3.closePolicy,
        () => [f1.font.pixelSize === f2.font.pixelSize, f1.font.pixelSize !== 21, f1.font.bold].join(" "),
        () => [f3.font.pixelSize, f3.leftPadding, f4.padding, f4.topPadding].join(" "),
        () => [s2.topLeftRadius, s2.bottomRightRadius, s2.antialiasing].join(" "),
        () => g2.rowSpacing,
        () => v2.contentWidth,
        () => b2.contentWidth,
        () => [c4.horizontalPadding, c4.leftPadding, c4.topPadding].join(" "),
        () => [c5.padding, c5.spacing, c5.leftPadding].join(" "),
        () => [p4.margins, p4.leftMargin, p4.closePolicy].join(" "),
        () => [p5.margins, p5.leftMargin].join(" "),
        () => [pn1.contentWidth, pn1.contentHeight].join(" "),
        () => [te1, tx1, te2].map(t => String(t.baseUrl).replace(/.*\//, "")).join(" ")
    ]
    property string result: (tick, cases.map(f => tt(f)).join(" | "))
}
