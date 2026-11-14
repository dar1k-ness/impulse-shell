// Item.transform as Qt 6.12: Translate, Scale, Rotation, Matrix4x4 and a list of several, with
// the item's own `rotation`, `scale` and `transformOrigin`. The item's matrix is its x and y,
// then the list from its last entry to its first, then scale and rotation about the origin
// (QQuickItemPrivate::itemToParentTransform). mapToItem, mapFromItem, mapToGlobal and
// mapFromGlobal of points and rectangles go through it, `contains` and `childAt` ask it, and a
// change of a transform or of the list moves the item at once.
// qt: a -9,27 -6,31 9,-27 -9,26 11,-24 -8,29,10,20 10,-25,10,20 14,-22 1 | b -20,37.5 -14,39.5 10,-75 -38,33 20.5,-63 -18,38.5,20,10 10.5,-71,5,40 12.5,-65 1 | c 75,5 71,8 -5,75 84,-4 6,84 53,6,20,10 -3,64,20,10 0,70 1 | d -10,57 -16,33 -5,9.5 8,111 4.5,19.33 -32,-75,20,120 -10.5,5.83,5,3.33 -7.5,8.67 4 | e 65,65 73,67.5 -14.44,-72.22 42.5,60.5 -4.22,-61.11 68,61.5,30,25 -18.89,-70.22,8.89,20 -13.33,-66.67 1 | f 39.3,29.47 40.9,33.64 -68.94,-25.85 34.44,19.96 -57.29,-15.01 39.83,31.56,5.3,20.68 -67.29,-24.71,16.79,18.8 -60.64,-21.6 1 | g 154,151.04 50.93,45 63.69,-4.19 145.33,-79 145.33,-79 86.29,222.83,1.06,4.24 | defaults 1 1 1 QVector3D(0, 0, 0) 0 QVector3D(0, 0, 1) QVector3D(0, 0, 0) 0 0 QMatrix4x4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1) | contains true false true false | childAt h2 null null h2 h2 null h2 | live 111,27 | 22,22 2 | 11,22 0 | 8,21 1 | 
import QtQuick
Item {
    id: root
    width: 400; height: 300
    property string result: ""
    function r(v) { return Math.round(v * 100) / 100 }
    function pt(p) { return r(p.x) + "," + r(p.y) }
    function rc(p) { return r(p.x) + "," + r(p.y) + "," + r(p.width) + "," + r(p.height) }
    function log(s) { result += s + " | " }

    // Translate alone.
    Item { id: a; x: 10; y: 20; width: 100; height: 50
        transform: Translate { x: -20; y: 5 }
        Item { id: a1; x: 1; y: 2; width: 10; height: 10 }
    }
    // Scale with an origin, and non-uniform.
    Item { id: b; x: 10; y: 20; width: 100; height: 50
        transform: Scale { origin.x: 50; origin.y: 25; xScale: 2; yScale: 0.5 }
        Item { id: b1; x: 10; y: 10; width: 10; height: 10 }
    }
    // Rotation about z with an origin.
    Item { id: c; x: 10; y: 20; width: 100; height: 50
        transform: Rotation { origin.x: 50; origin.y: 25; angle: 90 }
        Item { id: c1; x: 10; y: 10; width: 10; height: 10 }
    }
    // A list: the order of application, with the item's own rotation and scale.
    Item { id: d; x: 10; y: 20; width: 100; height: 50; rotation: 90; scale: 2; transformOrigin: Item.TopLeft
        transform: [ Translate { x: 30 }, Scale { xScale: 3 }, Rotation { angle: 90 }, Translate { y: 7 } ]
        Item { id: d1; x: 10; y: 10; width: 10; height: 10 }
    }
    // Matrix4x4, affine.
    Item { id: e; x: 10; y: 20; width: 100; height: 50
        transform: Matrix4x4 { matrix: Qt.matrix4x4(2, 0.5, 0, 30,  -0.5, 1, 0, 40,  0, 0, 1, 0,  0, 0, 0, 1) }
        Item { id: e1; x: 10; y: 10; width: 10; height: 10 }
    }
    // Rotation about the y axis: a projection.
    Item { id: f; x: 10; y: 20; width: 100; height: 50
        transform: Rotation { origin.x: 50; origin.y: 25; axis { x: 0; y: 1; z: 0 } angle: 60 }
        Item { id: f1; x: 10; y: 10; width: 10; height: 10 }
    }
    // Nested transforms.
    Item { id: g; x: 100; y: 100; width: 100; height: 100; scale: 0.5
        Item { id: g1; x: 20; y: 20; width: 40; height: 40
            transform: [ Rotation { angle: 180; origin.x: 20; origin.y: 20 }, Translate { x: 8; y: -4 } ]
            Item { id: g2; x: 5; y: 6; width: 10; height: 10; rotation: 45 }
        }
    }
    // Defaults and what the properties read.
    Scale { id: sc }
    Rotation { id: ro }
    Translate { id: tr }
    Matrix4x4 { id: mx }
    Item { id: h; width: 40; height: 40
        Item { id: h1; x: 30; y: 30; width: 20; height: 20; transform: Translate { x: -25; y: -25 } }
        Item { id: h2; x: 0; y: 0; width: 10; height: 10; scale: 3; transformOrigin: Item.TopLeft }
        Item { id: h3; x: 35; y: 0; width: 10; height: 10; visible: false }
    }

    Component.onCompleted: {
        for (const [n, it, kid] of [["a", a, a1], ["b", b, b1], ["c", c, c1], ["d", d, d1], ["e", e, e1], ["f", f, f1]]) {
            log(n + " " + pt(kid.mapToItem(root, 0, 0)) + " " + pt(kid.mapToItem(null, 3, 4)) + " " + pt(kid.mapFromItem(root, 0, 0))
                + " " + pt(it.mapToGlobal(1, 1)) + " " + pt(it.mapFromGlobal(1, 1))
                + " " + rc(kid.mapToItem(root, 1, 2, 10, 20)) + " " + rc(kid.mapFromItem(root, Qt.rect(1, 2, 10, 20)))
                + " " + pt(root.mapToItem(kid, Qt.point(5, 5))) + " " + it.transform.length)
        }
        log("g " + pt(g2.mapToItem(root, 0, 0)) + " " + pt(g2.mapToItem(g, 10, 0)) + " " + pt(root.mapToItem(g2, 130, 130))
            + " " + pt(g2.mapToItem(c1, 1, 1)) + " " + pt(c1.mapFromItem(g2, 1, 1)) + " " + rc(g2.mapToItem(b1, 0, 0, 4, 2)))
        log("defaults " + sc.xScale + " " + sc.yScale + " " + sc.zScale + " " + sc.origin + " " + ro.angle + " " + ro.axis + " " + ro.origin
            + " " + tr.x + " " + tr.y + " " + mx.matrix)
        log("contains " + h1.contains(Qt.point(5, 5)) + " " + h1.contains(Qt.point(-5, 5)) + " " + h2.contains(Qt.point(9, 9)) + " " + h2.contains(Qt.point(11, 9)))
        const who = (x, y) => { const k = h.childAt(x, y); return k === h1 ? "h1" : k === h2 ? "h2" : k === h3 ? "h3" : String(k) }
        log("childAt " + who(6, 6) + " " + who(31, 31) + " " + who(40, 40) + " " + who(29, 29) + " " + who(29, 4) + " " + who(36, 2) + " " + who(26, 2))
        // Changes on the fly.
        a.transform[0].x = 100
        log("live " + pt(a1.mapToItem(root, 0, 0)))
        a.transform = [tr, sc]
        tr.x = 5; sc.xScale = 2
        log(pt(a1.mapToItem(root, 0, 0)) + " " + a.transform.length)
        a.transform = []
        log(pt(a1.mapToItem(root, 0, 0)) + " " + a.transform.length)
        ro.angle = 90
        a.transform.push(ro)
        log(pt(a1.mapToItem(root, 0, 0)) + " " + a.transform.length)
    }
}
