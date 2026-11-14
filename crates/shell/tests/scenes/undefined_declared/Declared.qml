import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
Item {
    id: root
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    // A property with RESET.
    Grid { id: g1; rowSpacing: undefined; columnSpacing: undefined }
    Grid { id: g2; spacing: 4; rowSpacing: undefined; columnSpacing: undefined }
    ScrollView { id: sv; contentWidth: undefined; contentHeight: undefined }
    SwipeView { id: sw; contentWidth: undefined; contentHeight: undefined }
    Pane { id: pn; contentWidth: undefined; contentHeight: undefined }
    TabBar { id: tb; contentWidth: undefined; contentHeight: undefined }
    Rectangle { id: r1; radius: 7; topLeftRadius: undefined; bottomRightRadius: undefined }
    Rectangle { id: r2; topLeftRadius: undefined }
    Column { id: co; padding: 3; leftPadding: undefined; topPadding: undefined }
    Text { id: tx; padding: 3; leftPadding: undefined; text: "a" }
    Button { id: bt; padding: 5; leftPadding: undefined; horizontalPadding: undefined; spacing: undefined; implicitWidth: undefined }
    Button { id: bt2; horizontalPadding: 8; leftPadding: undefined; topInset: undefined }
    Loader { id: ld; sourceComponent: undefined; width: undefined }
    Item { id: lm; LayoutMirroring.enabled: undefined }
    // A property with no RESET.
    Flickable { id: fl; contentWidth: undefined; contentHeight: undefined; contentX: undefined }
    Grid { id: g3; spacing: undefined; columns: undefined; rows: undefined }
    Button { id: bt3; text: undefined; checkable: undefined; down: undefined; flat: undefined }
    Slider { id: sl; from: undefined; value: undefined; stepSize: undefined }
    ScrollView { id: sv2; clip: undefined; focusPolicy: undefined }
    // A `var` property and the typed ones of a document.
    Item {
        id: h
        property var v: undefined
        property variant vv: undefined
        property int i: undefined
        property real r: undefined
        property double d: undefined
        property string s: undefined
        property bool bo: undefined
        property color col: undefined
        property url u: undefined
        property date dt: undefined
        property point pt: undefined
        property size sz: undefined
        property rect rc: undefined
        property font fn: undefined
        property Item it: undefined
        property QtObject qo: undefined
        property Component cmp: undefined
        property list<int> li: undefined
        property list<Item> lo: undefined
        property list<var> lv: undefined
        property var vl: [undefined]
        readonly property var rv: undefined
        readonly property int ri: undefined
        required property var qv
        qv: undefined
    }
    component Al: Item {
        property alias aw: inner.width
        property alias ah: inner.height
        property alias ax: inner.x
        property alias at: txt.text
        property alias av: inner.v
        property alias ai: inner.n
        property alias am: inner.anchors.leftMargin
        property alias ar: rect.topLeftRadius
        property alias ao: inner
        property alias aa: inner.aw2
        property alias inn: inner
        Item {
            id: inner
            implicitWidth: 13; implicitHeight: 14; width: 5; height: 6; x: 3
            property var v: 1
            property int n: 4
            property alias aw2: txt.width
            anchors.margins: 11
        }
        Text { id: txt; text: "t"; width: 9 }
        Rectangle { id: rect; radius: 7; topLeftRadius: 2 }
    }
    // An alias: the property it stands for decides. (Not here: an alias of `width` or `height`
    // bound to `undefined` where it is declared, which leaves an uninitialized number in Qt.)
    Al { id: al; ax: undefined; at: undefined; av: undefined; ai: undefined; am: undefined; ar: undefined; aa: undefined }
    Al { id: al2 }
    Item {
        id: sa
        property alias w: sai.width
        property alias o: sai.opacity
        Item { id: sai; implicitWidth: 21; width: 2; opacity: 0.5 }
    }
    // A grouped property.
    Item { id: an; anchors { margins: 11; leftMargin: undefined; topMargin: undefined; baselineOffset: undefined; horizontalCenterOffset: undefined; alignWhenCentered: undefined } }
    Item { id: an2; anchors.margins: undefined; anchors.rightMargin: undefined }
    Rectangle { id: bd; border { width: undefined; color: undefined; pixelAligned: undefined } }
    Text { id: ft; font { pixelSize: undefined; bold: undefined; family: undefined; weight: undefined; letterSpacing: undefined; capitalization: undefined } }
    Text { id: ft2; font: undefined }
    Button { id: ft3; font: undefined }
    Image { id: im; sourceSize.width: undefined; sourceSize.height: undefined }
    Item { id: ly; layer.enabled: undefined; layer.textureSize: undefined; layer.smooth: undefined }
    // An attached property.
    RowLayout {
        Item { id: l1; Layout.margins: 6; Layout.leftMargin: undefined; Layout.topMargin: undefined }
        Item { id: l2; Layout.leftMargin: undefined; Layout.margins: undefined }
        Item { id: l3; Layout.preferredWidth: undefined; Layout.minimumWidth: undefined; Layout.maximumWidth: undefined; Layout.fillWidth: undefined; Layout.fillHeight: undefined; Layout.alignment: undefined; Layout.row: undefined; Layout.columnSpan: undefined; Layout.horizontalStretchFactor: undefined }
        Text { id: l4; text: "abc"; Layout.fillWidth: undefined }
    }
    Item { id: ky; Keys.enabled: undefined; Keys.priority: undefined; KeyNavigation.tab: undefined; KeyNavigation.priority: undefined }
    Item { id: tt1; ToolTip.text: undefined; ToolTip.delay: undefined; ToolTip.timeout: undefined; ToolTip.visible: undefined }
    Item { id: dr; Drag.active: undefined; Drag.hotSpot: undefined; Drag.keys: undefined }
    Flickable { id: sb; ScrollBar.vertical: undefined }
    Item { id: lm2; LayoutMirroring.enabled: true; LayoutMirroring.childrenInherit: undefined }
    property int tick: 0
    Timer { interval: 1; running: true; onTriggered: root.tick++ }
    property var cases: [
        () => g1.rowSpacing,
        () => g1.columnSpacing,
        () => g2.rowSpacing,
        () => g2.columnSpacing,
        () => sv.contentWidth,
        () => sv.contentHeight,
        () => sw.contentWidth,
        () => sw.contentHeight,
        () => pn.contentWidth,
        () => pn.contentHeight,
        () => tb.contentWidth,
        () => tb.contentHeight,
        () => r1.topLeftRadius,
        () => r1.bottomRightRadius,
        () => r2.topLeftRadius,
        () => co.leftPadding,
        () => co.topPadding,
        () => tx.leftPadding,
        () => bt.leftPadding,
        () => bt.horizontalPadding,
        () => bt.spacing,
        () => bt.implicitWidth,
        () => bt2.leftPadding,
        () => bt2.topInset,
        () => ld.sourceComponent,
        () => ld.width,
        () => lm.LayoutMirroring.enabled,
        () => fl.contentWidth,
        () => fl.contentHeight,
        () => fl.contentX,
        () => g3.spacing,
        () => g3.columns,
        () => g3.rows,
        () => bt3.text,
        () => bt3.checkable,
        () => bt3.down,
        () => bt3.flat,
        () => sl.from,
        () => sl.value,
        () => sl.stepSize,
        () => sv2.clip,
        () => sv2.focusPolicy,
        () => h.v,
        () => h.vv,
        () => h.i,
        () => h.r,
        () => h.d,
        () => h.s,
        () => h.bo,
        () => h.col,
        () => h.u,
        () => h.pt,
        () => h.sz,
        () => h.rc,
        () => h.fn.family === Qt.application.font.family,
        () => h.it,
        () => h.qo,
        () => h.cmp,
        () => h.li.length,
        () => h.lo.length,
        () => h.lv.length,
        () => h.vl.length + " " + h.vl[0],
        () => h.rv,
        () => h.ri,
        () => h.qv,
        () => al.ax,
        () => al.at,
        () => al.av,
        () => al.ai,
        () => al.am,
        () => al.ar,
        () => al.aa,
        () => al.inn.x + " " + al.inn.v + " " + al.inn.n + " " + al.inn.anchors.leftMargin,
        () => { al2.aw = undefined; return al2.aw },
        () => al2.at,
        () => { al2.av = undefined; return al2.av },
        () => al2.ai,
        () => { al2.am = undefined; return al2.am },
        () => { al2.ar = undefined; return al2.ar },
        () => { sa.w = undefined; return sa.w },
        () => sa.o,
        () => an.anchors.leftMargin,
        () => an.anchors.topMargin,
        () => an.anchors.baselineOffset,
        () => an.anchors.horizontalCenterOffset,
        () => an.anchors.alignWhenCentered,
        () => an2.anchors.margins,
        () => an2.anchors.rightMargin,
        () => bd.border.width,
        () => bd.border.color,
        () => bd.border.pixelAligned,
        () => ft.font.pixelSize,
        () => ft.font.bold,
        () => ft.font.weight,
        () => ft.font.letterSpacing,
        () => ft.font.capitalization,
        () => ft2.font.family === Qt.application.font.family,
        () => ft3.font.pixelSize,
        () => im.sourceSize.width,
        () => im.sourceSize.height,
        () => ly.layer.enabled,
        () => ly.layer.textureSize,
        () => ly.layer.smooth,
        () => l1.Layout.leftMargin,
        () => l1.Layout.topMargin,
        () => l1.Layout.rightMargin,
        () => l2.Layout.leftMargin,
        () => l2.Layout.margins,
        () => l3.Layout.preferredWidth,
        () => l3.Layout.minimumWidth,
        () => l3.Layout.fillWidth,
        () => l3.Layout.fillHeight,
        () => l3.Layout.alignment,
        () => l3.Layout.row,
        () => l4.Layout.fillWidth,
        () => ky.Keys.priority,
        () => ky.KeyNavigation.tab,
        () => tt1.ToolTip.text,
        () => tt1.ToolTip.visible,
        () => dr.Drag.active,
        () => dr.Drag.hotSpot,
        () => dr.Drag.keys.length,
        () => sb.ScrollBar.vertical,
        () => lm2.LayoutMirroring.childrenInherit
    ]
    property string result: (tick, cases.map(f => tt(f)).join(" | "))
}
