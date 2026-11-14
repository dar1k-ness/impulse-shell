import QtQuick
Item {
    id: root
    property list<int> li: [1, 2]
    property list<string> ls: ["a", "b"]
    property list<var> lv: [1, "a", [2, 3]]
    property list<Item> lo: [Item {}, Item {}]
    property list<real> lr: [1.5]
    property list<url> lu: ["a.qml"]
    property list<point> lp: [Qt.point(1, 2)]
    property var va: [1, [2, 3]]
    property var vo: ({ a: [1] })
    property var vs: "s"
    property variant vt: [4, 5]
    Item { id: ch }
    Component.onCompleted: {
        console.log(li); console.log(ls); console.log(lv); console.log(lo.length, typeof lo);
        console.log(lr); console.log(lu.length); console.log(lp);
        console.log(va); console.log(vo.a); console.log(vs); console.log(vt);
        console.log(Object.keys({ a: 1, b: 2 }));
        console.log("a,b".split(","));
        console.log([li, ls]);
        console.log([[], [[]]]);
        console.log(Array.from("ab"), [..."cd"]);
        console.log("x", [1], "y");
        console.log(Qt.fontFamilies().length > 0);
        console.log([undefined], [null], [[undefined, null]]);
        console.log(["%1", "%s"], "%d");
        var holes = []; holes[2] = 1; console.log(holes);
        console.log(JSON.parse("[1,[2,{\"a\":3}]]"));
        console.log([1, 2].map(function(x) { return [x] }));
        console.log([true, "true", 1, "1"]);
        Qt.quit();
    }
}
