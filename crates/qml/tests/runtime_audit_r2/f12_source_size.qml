import QtQuick
Item {
    id: r
    property alias sw: img.sourceSize.width
    property alias sh: img.sourceSize.height
    property alias ss: img.sourceSize
    property int n: 0
    property size bound: img.sourceSize
    property real boundW: img.sourceSize.width
    function s(z) { return z.width + "," + z.height }
    onSwChanged: console.log("  swChanged", sw)
    onShChanged: console.log("  shChanged", sh)
    onSsChanged: console.log("  ssChanged", s(ss))
    onBoundChanged: console.log("  boundChanged", s(bound))
    onBoundWChanged: console.log("  boundWChanged", boundW)
    Image { id: img; onSourceSizeChanged: { r.n++; console.log("  img.sourceSizeChanged", s(sourceSize)) } }
    Image { id: img2; sourceSize.width: 3; onSourceSizeChanged: console.log("  img2.sourceSizeChanged", s(sourceSize)) }
    Image { id: img3; sourceSize: Qt.size(4, 4); onSourceSizeChanged: console.log("  img3.sourceSizeChanged", s(sourceSize)) }
    Image { id: img4; sourceSize.width: 3; sourceSize.height: 6; onSourceSizeChanged: console.log("  img4.sourceSizeChanged", s(sourceSize)) }
    AnimatedImage { id: ani; onSourceSizeChanged: console.log("  ani.sourceSizeChanged", s(sourceSize)) }
    BorderImage { id: bi; onSourceSizeChanged: console.log("  bi.sourceSizeChanged", s(sourceSize)) }
    Component.onCompleted: {
        console.log("0", sw, sh, s(img.sourceSize), s(img2.sourceSize), s(img3.sourceSize), s(img4.sourceSize), s(ani.sourceSize), s(bi.sourceSize), n)
        sw = 5; console.log("1", sw, sh, s(bound), boundW, n)
        sw = 5; console.log("2", sw, sh, n)
        img.sourceSize = Qt.size(7, 8); console.log("3", sw, sh, s(bound), n)
        img.sourceSize = Qt.size(7, 8); console.log("4", sw, sh, n)
        img.sourceSize.height = 9; console.log("5", sw, sh, n)
        img.sourceSize.width = 7; console.log("6", sw, sh, n)
        img.sourceSize = Qt.size(7, 1); console.log("7", sw, sh, n)
        img.sourceSize = undefined; console.log("8", sw, sh, n)
        img.sourceSize = Qt.size(0, 0); console.log("9", sw, sh, n)
        img.sourceSize = Qt.size(-1, -1); console.log("10", sw, sh, n)
        sh = 2; console.log("11", sw, sh, n)
        var z = img.sourceSize; z.width = 12; console.log("12", sw, sh, z.width, n)
        img2.sourceSize.height = 2; console.log("13", s(img2.sourceSize))
        img3.sourceSize = Qt.size(4, 5); console.log("14", s(img3.sourceSize))
        ani.sourceSize = Qt.size(2, 2); console.log("15", s(ani.sourceSize))
        ani.sourceSize.width = 6; console.log("16", s(ani.sourceSize))
        try { bi.sourceSize = Qt.size(2, 2) } catch (e) { console.log("17", e) }
        console.log("18", s(bi.sourceSize))
    }
}
