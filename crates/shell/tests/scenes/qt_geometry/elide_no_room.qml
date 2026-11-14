import QtQuick
import QtQuick.Layouts
Item {
    id: root; width: 400; height: 330
    property real probe: 0
    property alias kaW: ka.implicitWidth
    property alias kcH: kc.implicitHeight
    property alias kdW: kd.implicitWidth
    property alias keW: ke.implicitWidth
    property alias kfW: kf.implicitWidth
    property alias kgW: kg.implicitWidth
    property alias khW: kh.implicitWidth
    property alias khW2: root.khW
    property alias kiT: ki
    property alias kjW: kj.implicitWidth
    function look(t) { root.probe = t.implicitWidth }
    Item { id: box; objectName: "box"; x: 100; width: 120; height: 20
        Text { id: a; objectName: "anchored"; anchors.left: parent.left; anchors.right: parent.right; anchors.leftMargin: 30; anchors.rightMargin: 30; text: "MMMM"; color: "#ff0000"; elide: Text.ElideRight }
    }
    Text { id: b; objectName: "setNeg"; y: 30; width: 100; text: "MMMM"; color: "#00ff00"; elide: Text.ElideLeft }
    Text { id: c; objectName: "setZero"; y: 60; width: 100; text: "MMMM"; color: "#0000ff"; elide: Text.ElideMiddle }
    Text { id: d; objectName: "padded"; y: 90; width: 100; padding: 4; text: "MMMM"; color: "#ff00ff"; elide: Text.ElideRight }
    Text { id: e; objectName: "noHeight"; y: 120; width: 100; height: 20; text: "MMMM"; color: "#00ffff"; elide: Text.ElideRight }
    Text { id: f; objectName: "noHeightLeft"; y: 150; width: 100; height: 20; text: "MMMM"; color: "#ff8000"; elide: Text.ElideLeft }
    Text { id: g; objectName: "styled"; y: 180; width: 100; text: "<b>MMMM</b>"; textFormat: Text.StyledText; color: "#8000ff"; elide: Text.ElideRight }
    Text { id: h; objectName: "noElide"; y: 210; width: 100; text: "MMMM"; color: "#ffff00" }
    Text { id: k; objectName: "back"; y: 240; width: 100; text: "MMMM"; color: "#80ff80"; elide: Text.ElideRight }
    RowLayout { objectName: "row"; y: 270; width: 60; spacing: 0
        Text { id: m; objectName: "inRow"; Layout.fillWidth: true; text: "MMMM"; color: "#ff80ff"; elide: Text.ElideRight }
        Item { objectName: "rest"; implicitWidth: 60; implicitHeight: 10 }
    }
    Text { id: n; objectName: "rich"; y: 300; width: 100; text: "<b>MMMM</b>"; textFormat: Text.RichText; color: "#808000"; elide: Text.ElideRight }
    // What follows is laid out in full although it has no room: its implicit size is used (by
    // a layout or a binding) and is not known for this text, which changed without room or
    // never had any. The unnamed Texts are not read by the dump, which would do the same.
    RowLayout { y: 270; x: 200; width: 60; spacing: 0
        Text { id: p; objectName: "rowChanged"; Layout.fillWidth: true; text: "MMMM"; elide: Text.ElideRight }
        Item { implicitWidth: 60; implicitHeight: 10 }
    }
    Text { id: q; objectName: "readChanged"; x: 200; y: 30; width: 100; text: "MMMM"; elide: Text.ElideRight }
    Item { objectName: "readChangedUser"; width: q.implicitWidth }
    Text { id: r; x: 200; y: 60; width: 100; text: "MMMM"; elide: Text.ElideRight }
    Text { id: s; objectName: "readFromStart"; x: 200; y: 90; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Item { objectName: "readFromStartUser"; width: s.implicitWidth }
    Text { id: t; x: 200; y: 120; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Text { id: u; objectName: "fullThenRoom"; x: 200; y: 150; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Item { objectName: "fullThenRoomUser"; width: u.implicitWidth }
    Text { id: v; objectName: "fullThenMode"; x: 200; y: 180; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Item { objectName: "fullThenModeUser"; width: v.implicitWidth }
    Item { objectName: "rowChangedState"; x: p.lineCount; y: p.truncated ? 1 : 0; width: p.contentWidth; height: p.contentHeight }
    Item { objectName: "readChangedState"; x: q.lineCount; y: q.truncated ? 1 : 0; width: q.contentWidth; height: q.contentHeight }
    Item { objectName: "unreadChangedState"; x: r.lineCount; y: r.truncated ? 1 : 0; width: r.contentWidth; height: r.contentHeight }
    Item { objectName: "readFromStartState"; x: s.lineCount; y: s.truncated ? 1 : 0; width: s.contentWidth; height: s.contentHeight }
    Item { objectName: "unreadFromStartState"; x: t.lineCount; y: t.truncated ? 1 : 0; width: t.contentWidth; height: t.contentHeight }
    Item { objectName: "fullThenRoomState"; x: u.lineCount; y: u.truncated ? 1 : 0; width: u.contentWidth; height: u.contentHeight }
    Item { objectName: "fullThenModeState"; x: v.lineCount; y: v.truncated ? 1 : 0; width: v.contentWidth; height: v.contentHeight }
    Timer { interval: 50; running: true; onTriggered: { q.width = -3; r.width = -3; u.width = 100; v.elide = Text.ElideLeft } }
    Timer { interval: 150; running: true; onTriggered: { p.text = "WWWW"; q.text = "WWWW"; r.text = "WWWW"; u.width = -5 } }
    Item { objectName: "anchoredState"; x: a.lineCount; y: a.truncated ? 1 : 0; width: a.contentWidth; height: a.contentHeight }
    Item { objectName: "setNegState"; x: b.lineCount; y: b.truncated ? 1 : 0; width: b.contentWidth; height: b.contentHeight }
    Item { objectName: "setZeroState"; x: c.lineCount; y: c.truncated ? 1 : 0; width: c.contentWidth; height: c.contentHeight }
    Item { objectName: "paddedState"; x: d.lineCount; y: d.truncated ? 1 : 0; width: d.contentWidth; height: d.contentHeight }
    Item { objectName: "noHeightState"; x: e.lineCount; y: e.truncated ? 1 : 0; width: e.contentWidth; height: e.contentHeight }
    Item { objectName: "noHeightLeftState"; x: f.lineCount; y: f.truncated ? 1 : 0; width: f.contentWidth; height: f.contentHeight }
    Item { objectName: "styledState"; x: g.lineCount; y: g.truncated ? 1 : 0; width: g.contentWidth; height: g.contentHeight }
    Item { objectName: "noElideState"; x: h.lineCount; y: h.truncated ? 1 : 0; width: h.contentWidth; height: h.contentHeight }
    Item { objectName: "backState"; x: k.lineCount; y: k.truncated ? 1 : 0; width: k.contentWidth; height: k.contentHeight }
    Item { objectName: "inRowState"; x: m.lineCount; y: m.truncated ? 1 : 0; width: m.contentWidth; height: m.contentHeight }
    Item { objectName: "richState"; x: n.lineCount; y: n.truncated ? 1 : 0; width: n.contentWidth; height: n.contentHeight }
    Timer { interval: 50; running: true; onTriggered: {
        box.width = 20
        b.width = -3
        c.width = 0
        d.width = 8
        e.height = 0
        f.height = 0
        g.width = -1
        h.width = -3
        k.width = -3
        n.width = -3
    } }
    Timer { interval: 150; running: true; onTriggered: k.width = 100 }
    Text { id: ja; objectName: "jsDoneStart"; x: 300; y: 0; width: -3; text: "MMMM"; elide: Text.ElideRight
        Component.onCompleted: root.probe = ja.implicitWidth }
    Text { id: jb; objectName: "jsOwnStart"; x: 300; y: 30; width: -3; text: "MMMM"; elide: Text.ElideRight
        Component.onCompleted: root.probe = implicitHeight }
    Text { id: jc; objectName: "jsHandlerStart"; x: 300; y: 60; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Text { id: jd; objectName: "jsFunctionStart"; x: 300; y: 90; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Text { id: je; objectName: "jsRoomThenRead"; x: 300; y: 120; width: 100; text: "MMMM"; elide: Text.ElideRight }
    Text { id: jf; objectName: "jsReadThenChanged"; x: 300; y: 150; width: 100; text: "MMMM"; elide: Text.ElideRight
        Component.onCompleted: root.probe = jf.implicitWidth }
    Text { id: jg; objectName: "jsLateReadChanged"; x: 300; y: 180; width: 100; text: "MMMM"; elide: Text.ElideRight }
    Text { id: jh; objectName: "jsChangedThenRead"; x: 300; y: 210; width: 100; text: "MMMM"; elide: Text.ElideRight }
    Item { objectName: "jsDoneStartState"; x: ja.lineCount; y: ja.truncated ? 1 : 0; width: ja.contentWidth; height: ja.contentHeight }
    Item { objectName: "jsOwnStartState"; x: jb.lineCount; y: jb.truncated ? 1 : 0; width: jb.contentWidth; height: jb.contentHeight }
    Item { objectName: "jsHandlerStartState"; x: jc.lineCount; y: jc.truncated ? 1 : 0; width: jc.contentWidth; height: jc.contentHeight }
    Item { objectName: "jsFunctionStartState"; x: jd.lineCount; y: jd.truncated ? 1 : 0; width: jd.contentWidth; height: jd.contentHeight }
    Item { objectName: "jsRoomThenReadState"; x: je.lineCount; y: je.truncated ? 1 : 0; width: je.contentWidth; height: je.contentHeight }
    Item { objectName: "jsReadThenChangedState"; x: jf.lineCount; y: jf.truncated ? 1 : 0; width: jf.contentWidth; height: jf.contentHeight }
    Item { objectName: "jsLateReadChangedState"; x: jg.lineCount; y: jg.truncated ? 1 : 0; width: jg.contentWidth; height: jg.contentHeight }
    Item { objectName: "jsChangedThenReadState"; x: jh.lineCount; y: jh.truncated ? 1 : 0; width: jh.contentWidth; height: jh.contentHeight }
    Timer { interval: 50; running: true; onTriggered: { je.width = -3; jf.width = -3; jg.width = -3; jh.width = -3 } }
    Timer { interval: 100; running: true; onTriggered: { root.probe = jc.implicitWidth; root.look(jd); root.probe = je.implicitWidth; root.probe = jg.implicitHeight } }
    Timer { interval: 150; running: true; onTriggered: { jf.text = "WWWW"; jg.text = "WWWW"; jh.text = "WWWW" } }
    Timer { interval: 200; running: true; onTriggered: root.probe = jh.implicitWidth }
    Text { id: ka; objectName: "aliasDoneStart"; x: 200; y: 0; width: -3; text: "MMMM"; elide: Text.ElideRight
        Component.onCompleted: root.probe = root.kaW }
    Text { id: kb; objectName: "aliasOwnStart"; x: 200; y: 30; width: -3; text: "MMMM"; elide: Text.ElideRight
        property alias own: kb.implicitHeight
        Component.onCompleted: root.probe = own }
    Text { id: kc; objectName: "aliasHandlerStart"; x: 200; y: 60; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Text { id: kd; x: 200; y: 90; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Text { id: ke; objectName: "aliasRoomThenRead"; x: 200; y: 120; width: 100; text: "MMMM"; elide: Text.ElideRight }
    Text { id: kf; objectName: "aliasChangedThenRead"; x: 200; y: 150; width: 100; text: "MMMM"; elide: Text.ElideRight }
    Text { id: kg; objectName: "aliasBound"; x: 200; y: 180; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Item { objectName: "aliasBoundUser"; width: root.kgW }
    Text { id: kh; objectName: "aliasOfAlias"; x: 200; y: 210; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Text { id: ki; objectName: "aliasToItem"; x: 200; y: 240; width: -3; text: "MMMM"; elide: Text.ElideRight }
    Text { id: kj; x: 200; y: 270; width: 100; text: "MMMM"; elide: Text.ElideRight }
    Item { objectName: "aliasDoneStartState"; x: ka.lineCount; y: ka.truncated ? 1 : 0; width: ka.contentWidth; height: ka.contentHeight }
    Item { objectName: "aliasOwnStartState"; x: kb.lineCount; y: kb.truncated ? 1 : 0; width: kb.contentWidth; height: kb.contentHeight }
    Item { objectName: "aliasHandlerStartState"; x: kc.lineCount; y: kc.truncated ? 1 : 0; width: kc.contentWidth; height: kc.contentHeight }
    Item { objectName: "aliasUnreadState"; x: kd.lineCount; y: kd.truncated ? 1 : 0; width: kd.contentWidth; height: kd.contentHeight }
    Item { objectName: "aliasRoomThenReadState"; x: ke.lineCount; y: ke.truncated ? 1 : 0; width: ke.contentWidth; height: ke.contentHeight }
    Item { objectName: "aliasChangedThenReadState"; x: kf.lineCount; y: kf.truncated ? 1 : 0; width: kf.contentWidth; height: kf.contentHeight }
    Item { objectName: "aliasBoundState"; x: kg.lineCount; y: kg.truncated ? 1 : 0; width: kg.contentWidth; height: kg.contentHeight }
    Item { objectName: "aliasOfAliasState"; x: kh.lineCount; y: kh.truncated ? 1 : 0; width: kh.contentWidth; height: kh.contentHeight }
    Item { objectName: "aliasToItemState"; x: ki.lineCount; y: ki.truncated ? 1 : 0; width: ki.contentWidth; height: ki.contentHeight }
    Item { objectName: "aliasUnreadChangedState"; x: kj.lineCount; y: kj.truncated ? 1 : 0; width: kj.contentWidth; height: kj.contentHeight }
    Timer { interval: 50; running: true; onTriggered: { ke.width = -3; kf.width = -3; kj.width = -3 } }
    Timer { interval: 100; running: true; onTriggered: { root.probe = root.kcH; root.probe = root.keW; root.probe = root.khW2; root.probe = root.kiT.implicitWidth } }
    Timer { interval: 150; running: true; onTriggered: { kf.text = "WWWW"; kj.text = "WWWW" } }
    Timer { interval: 200; running: true; onTriggered: root.probe = root.kfW }
}
