import QtQuick
import QtQuick.Layouts
Item {
    id: root
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    Item { id: a; width: undefined }
    Item { id: b; width: ;
    }
    Item { id: c; implicitWidth: 7; width: undefined }
    Item { id: d; height: undefined; x: undefined; y: undefined; z: undefined }
    Item { id: e; opacity: undefined; visible: undefined; scale: undefined; rotation: undefined; clip: undefined; enabled: undefined }
    Rectangle { id: r1; color: undefined; radius: undefined; border.width: undefined; border.color: undefined }
    Text { id: g; text: undefined; font.pixelSize: undefined; font.family: undefined; color: undefined; width: undefined }
    Item { id: h; property real r: undefined; property int i: undefined; property string s: undefined; property bool bo: undefined; property var v: undefined; property color col: undefined; property url u: undefined; property Item it: undefined; property date da: undefined; property point pt: undefined }
    Item { id: i; anchors.margins: undefined; anchors.leftMargin: undefined; anchors.fill: undefined; anchors.left: undefined; anchors.centerIn: undefined }
    Item { id: j; width: 5; Component.onCompleted: width = undefined }
    Item { id: k; implicitWidth: 9; width: 5; Component.onCompleted: width = undefined }
    Item { id: l; property bool on: true; width: on ? 5 : undefined; implicitWidth: 3 }
    Item { id: m; implicitHeight: 4; height: ;
    }
    Item { id: n; Layout.preferredWidth: undefined; Layout.fillWidth: undefined; Layout.margins: undefined }
    Text { id: o; text: "abc"; width: undefined; height: undefined }
    Item { id: p; objectName: undefined; state: undefined }
    Item { id: q; implicitWidth: undefined; implicitHeight: undefined; baselineOffset: undefined; transformOrigin: undefined; activeFocusOnTab: undefined; focus: undefined; smooth: undefined; antialiasing: undefined }
    Image { id: r; source: undefined; fillMode: undefined; asynchronous: undefined }
    Timer { id: s; interval: undefined; running: undefined; repeat: undefined }
    ListView { id: t; model: undefined; delegate: undefined; spacing: undefined; currentIndex: undefined; cacheBuffer: undefined }
    MouseArea { id: u; hoverEnabled: undefined; acceptedButtons: undefined; cursorShape: undefined }
    Item { id: w2; property int pi: 5; property string lg; Component.onCompleted: { try { pi = undefined } catch (e) { lg = "" + e } } }
    Item { id: x2; property real pr: 5; property string lg; Component.onCompleted: { try { pr = undefined } catch (e) { lg = "" + e } } }
    Item { id: y2; width: 6; height: 6; Component.onCompleted: { height = undefined; x = 4; try { x = undefined } catch (e) { objectName = "" + e } } }
    Item { id: z2; implicitWidth: 11; width: 20; property bool on: true; Binding on width { when: z2.on; value: undefined } }
    Rectangle { id: a3; width: 8; implicitWidth: 2; Component.onCompleted: { width = Qt.binding(() => undefined); void 0 } }
    Item { id: d3; width: NaN; height: Infinity }
    Component.onCompleted: l.on = false
    property int tick: 0
    Timer { interval: 1; running: true; onTriggered: root.tick++ }
    property var cases: [
        () => a.width,
        () => b.width,
        () => c.width,
        () => d.height,
        () => d.x,
        () => d.y,
        () => d.z,
        () => e.opacity,
        () => e.visible,
        () => e.scale,
        () => e.rotation,
        () => e.clip,
        () => e.enabled,
        () => r1.color,
        () => r1.radius,
        () => r1.border.color,
        () => g.text,
        () => g.color,
        () => g.width,
        () => h.r,
        () => h.i,
        () => h.s,
        () => h.bo,
        () => h.v,
        () => h.col,
        () => h.u,
        () => h.it,
        () => h.pt,
        () => i.anchors.margins,
        () => i.anchors.leftMargin,
        () => i.anchors.fill,
        () => i.anchors.centerIn,
        () => j.width,
        () => k.width,
        () => l.width,
        () => m.height,
        () => n.Layout.preferredWidth,
        () => n.Layout.fillWidth,
        () => n.Layout.margins,
        () => o.width === o.implicitWidth,
        () => o.height === o.implicitHeight,
        () => p.objectName,
        () => p.state,
        () => q.implicitWidth,
        () => q.implicitHeight,
        () => q.baselineOffset,
        () => q.activeFocusOnTab,
        () => q.focus,
        () => q.antialiasing,
        () => r.fillMode,
        () => r.asynchronous,
        () => s.interval,
        () => s.running,
        () => s.repeat,
        () => t.model,
        () => t.delegate,
        () => t.spacing,
        () => u.hoverEnabled,
        () => u.cursorShape,
        () => w2.pi,
        () => w2.lg,
        () => x2.pr,
        () => x2.lg,
        () => y2.width,
        () => y2.height,
        () => y2.x,
        () => y2.objectName,
        () => z2.width,
        () => a3.width,
        () => d3.width,
        () => d3.height
    ]
    property string result: (tick, cases.map(f => tt(f)).join(" | "))
}
