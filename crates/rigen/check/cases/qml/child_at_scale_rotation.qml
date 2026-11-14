// Item.childAt(x, y) maps the point into each child with the child's own `scale`,
// `rotation` and `transformOrigin` (no `transform` list here) before asking whether the
// child contains it.
// qt: double null,double,double,double,double,null,null;half null,null,half,half,half,null;corner null,fromCorner,fromCorner,fromCorner,null;zero null,null,null;mirror mirror,mirror,null,mirror,null,null;quarter null,quarter,quarter,quarter,null,null,quarter,quarter,null;diamond diamond,null,null,diamond,diamond,null,diamond,null,null;swung null,swung,swung,null,swung,null;both both,both,both,null,null,null,null,null;parentScale inScaled,null,null;stacked plain,grown,plain,grown,grown,grown
import QtQuick
Item {
    id: root
    width: 200; height: 200
    property string result: ""
    function n(i) { return i === null ? "null" : i === undefined ? "undefined" : i.objectName }
    function probe(parent, points) {
        var s = []
        for (var i = 0; i < points.length; i++)
            s.push(n(parent.childAt(points[i][0], points[i][1])))
        return s.join(",")
    }
    Item {
        id: scaled; width: 200; height: 200
        // 40×40 at (40, 40), twice the size about its centre: covers 20..100.
        Item { objectName: "double"; x: 40; y: 40; width: 40; height: 40; scale: 2 }
    }
    Item {
        id: shrunk; width: 200; height: 200
        // Half the size about its centre: covers 50..70.
        Item { objectName: "half"; x: 40; y: 40; width: 40; height: 40; scale: 0.5 }
    }
    Item {
        id: corner; width: 200; height: 200
        // Twice the size about its top left corner: covers 40..120.
        Item { objectName: "fromCorner"; x: 40; y: 40; width: 40; height: 40; scale: 2; transformOrigin: Item.TopLeft }
    }
    Item {
        id: none; width: 200; height: 200
        Item { objectName: "scaleZero"; x: 40; y: 40; width: 40; height: 40; scale: 0 }
    }
    Item {
        id: mirrored; width: 200; height: 200
        // Mirrored about its top left corner: covers 0..40.
        Item { objectName: "mirror"; x: 40; y: 40; width: 40; height: 40; scale: -1; transformOrigin: Item.TopLeft }
    }
    Item {
        id: turned; width: 200; height: 200
        // 80×20 at (40, 70) turned a quarter about its centre: covers x 70..90, y 40..120.
        Item { objectName: "quarter"; x: 40; y: 70; width: 80; height: 20; rotation: 90 }
    }
    Item {
        id: diamond; width: 200; height: 200
        // 40×40 at (40, 40) turned 45° about its centre (60, 60): a diamond of half diagonal 28.28.
        Item { objectName: "diamond"; x: 40; y: 40; width: 40; height: 40; rotation: 45 }
    }
    Item {
        id: swung; width: 200; height: 200
        // 40×20 at (100, 100) turned 90° about its top left corner: covers x 80..100, y 100..140.
        Item { objectName: "swung"; x: 100; y: 100; width: 40; height: 20; rotation: 90; transformOrigin: Item.TopLeft }
    }
    Item {
        id: both; width: 200; height: 200
        // 40×20 at (80, 90), twice the size and turned 90° about its centre (100, 100): x 80..120, y 60..140.
        Item { objectName: "both"; x: 80; y: 90; width: 40; height: 20; scale: 2; rotation: 90 }
    }
    Item {
        id: nestedScale; width: 200; height: 200; scale: 2
        // The parent's own scale does not count: the point is in the parent's coordinates.
        Item { objectName: "inScaled"; x: 40; y: 40; width: 40; height: 40 }
    }
    Item {
        id: stacked; width: 200; height: 200
        Item { objectName: "plain"; x: 40; y: 40; width: 40; height: 40 }
        Item { objectName: "grown"; x: 90; y: 40; width: 20; height: 40; scale: 3 }
    }
    Component.onCompleted: {
        var out = []
        out.push("double " + probe(scaled, [[19, 60], [20, 60], [21, 21], [60, 60], [99, 99], [100, 60], [45, 100]]))
        out.push("half " + probe(shrunk, [[45, 45], [49, 60], [50, 50], [60, 60], [69, 69], [70, 60]]))
        out.push("corner " + probe(corner, [[39, 39], [40, 40], [100, 100], [119, 119], [120, 100]]))
        out.push("zero " + probe(none, [[60, 60], [45, 45], [40, 40]]))
        out.push("mirror " + probe(mirrored, [[20, 20], [1, 1], [0, 0], [40, 40], [41, 41], [60, 60]]))
        out.push("quarter " + probe(turned, [[45, 80], [80, 80], [80, 45], [80, 119], [80, 121], [69, 80], [71, 80], [89, 80], [91, 80]]))
        out.push("diamond " + probe(diamond, [[60, 60], [41, 41], [45, 45], [47, 47], [60, 33], [60, 31], [87, 60], [89, 60], [75, 75]]))
        out.push("swung " + probe(swung, [[110, 110], [90, 120], [81, 139], [79, 120], [99, 101], [101, 101]]))
        out.push("both " + probe(both, [[100, 100], [81, 61], [119, 139], [79, 100], [121, 100], [100, 59], [100, 141], [70, 100]]))
        out.push("parentScale " + probe(nestedScale, [[45, 45], [85, 85], [20, 20]]))
        out.push("stacked " + probe(stacked, [[60, 60], [75, 60], [69, 60], [71, 60], [100, 5], [100, 115]]))
        result = out.join(";")
    }
}
