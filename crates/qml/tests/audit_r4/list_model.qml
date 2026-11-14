import QtQuick
Item {
    id: root
    ListModel { id: lm; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } 
        onCountChanged: console.log("countChanged", count) }
    function dump(tag) { var s = []; for (var i = 0; i < lm.count; i++) s.push(lm.get(i).name + ":" + lm.get(i).n); console.log(tag, lm.count, s.join(",")) }
    Component.onCompleted: {
        dump("start");
        lm.append({ name: "c", n: 3 }); dump("append");
        lm.append([{ name: "d", n: 4 }, { name: "e", n: 5 }]); dump("append array");
        lm.insert(0, { name: "z", n: 0 }); dump("insert0");
        lm.move(0, 3, 2); dump("move(0,3,2)");
        lm.remove(1); dump("remove(1)");
        lm.remove(0, 2); dump("remove(0,2)");
        lm.set(0, { name: "S" }); dump("set0");
        lm.set(lm.count, { name: "new", n: 99 }); dump("set(count)");
        lm.setProperty(1, "n", 42); dump("setProperty");
        var g = lm.get(0); g.n = 1000; dump("get().n=1000");
        console.log("get(-1)", lm.get(-1), "get(99)", lm.get(99));
        lm.insert(99, { name: "bad" }); dump("insert(99)");
        lm.remove(99); dump("remove(99)");
        lm.move(0, 5, 1); dump("move bad");
        lm.append(5); dump("append(5)");
        lm.append({ other: true }); dump("append other role");
        console.log("typeof", typeof lm.dynamicRoles, JSON.stringify(lm.get(0)));
        lm.clear(); dump("clear");
        console.log("END"); Qt.quit();
    }
}
