import QtQuick
Item {
    ListModel { id: lm; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }
    ListModel { id: l2; ListElement { n: 1; name: "a" } }
    ListModel { id: l3; ListElement { n: 1; name: "a"; b: 3; a: 4 } }
    ListModel { id: l4; ListElement { b: 1; a: 2; c: 3 } ListElement { d: 1; a: 5 } }
    ListModel { id: l5 }
    function d(t) { console.log(t, JSON.stringify(lm.get(0))) }
    Component.onCompleted: {
        console.log(JSON.stringify(l2.get(0)), JSON.stringify(l3.get(0)));
        console.log(Object.keys(l4.get(0)).join(","), Object.keys(l4.get(1)).join(","));
        lm.insert(0, { name: "z", n: 0 }); d("insert")
        lm.move(0, 1, 2); d("move");
        lm.set(0, { name: "S" }); d("set")
        lm.setProperty(0, "n", 42); d("setProperty")
        var g = lm.get(0); g.n = 1000; d("row write")
        lm.append({ other: true }); console.log(JSON.stringify(lm.get(0)), JSON.stringify(lm.get(3)));
        l5.append({ name: "c", n: 3 }); l5.append({ zz: 1, n: 4, aa: 2 });
        console.log(JSON.stringify(l5.get(0)), JSON.stringify(l5.get(1)));
        l4.append({ zz: 1, q: 4, aa: 2 }); console.log(JSON.stringify(l4.get(0)));
        console.log("END"); Qt.quit();
    }
}
