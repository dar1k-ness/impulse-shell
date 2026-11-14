import QtQuick
Item {
    id: root
    property int v: Alpha.value
    property bool flag: false
    property int late: flag ? Delta.d : 0
    onVChanged: console.log("vChanged", v)
    function unused() { return Gamma.g }
    Component.onCompleted: {
        console.log("main completed v", v, "typeof Gamma", typeof Gamma, Alpha.hello());
        flag = true;
        console.log("after flag", late, Alpha.writes, Beta.back);
        console.log("same object", Alpha === Alpha, Beta.base, typeof Nope);
        console.log("END"); Qt.quit();
    }
}
