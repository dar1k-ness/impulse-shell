import QtQuick
Item {
    ListModel { id: lm; ListElement { zed: "a"; mid: 1; alpha: true } }
    ListModel { id: lm2 }
    Component.onCompleted: {
        console.log(JSON.stringify(lm.get(0)), Object.keys(lm.get(0)).join(","));
        lm.append({ b: 1, zz: 2, a: 3 });
        console.log(JSON.stringify(lm.get(1)), Object.keys(lm.get(1)).join(","));
        lm2.append({ q: 1, c: 2, x: 3, name: 4, n: 5 });
        console.log(JSON.stringify(lm2.get(0)), Object.keys(lm2.get(0)).join(","));
        console.log("END"); Qt.quit();
    }
}
