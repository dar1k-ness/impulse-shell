import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    Timer { id: tm; interval: 1000 }
    Text { id: tx; maximumLineCount: 1; font.pixelSize: 10 }
    Grid { id: gr; columns: 1 }
    NumberAnimation { id: a1; target: tm; property: "interval"; from: 1000; to: 1010; duration: 400 }
    NumberAnimation { id: a2; target: tx; property: "maximumLineCount"; from: 1; to: 11; duration: 400 }
    NumberAnimation { id: a3; target: gr; property: "columns"; from: 1; to: 11; duration: 400 }
    NumberAnimation { id: a4; target: tx; property: "font.pixelSize"; from: 10; to: 20; duration: 400 }
    PropertyAnimation { id: a5; target: tx; property: "font.weight"; from: 100; to: 110; duration: 400 }
    Component.onCompleted: { a1.start(); a2.start(); a3.start(); a4.start(); a5.start() }
    function sample() { say("s", tm.interval, tx.maximumLineCount, gr.columns, tx.font.pixelSize, tx.font.weight) }
}
