import QtQuick

// The shape of qml/services/Hyprsunset.qml: what does the daemon get when the config becomes ready?
Item {
    id: root
    property string log: ""
    function say() { log += Array.prototype.join.call(arguments, " ") + "\n"; console.log(Array.prototype.join.call(arguments, " ")) }

    QtObject {
        id: config
        property bool ready: false
        property bool nightAutomatic: true
    }
    property bool night: true

    property bool configReady: config?.ready ?? true
    property bool automatic: config.nightAutomatic && configReady
    property int colorTemperature: 5000
    property int defaultColorTemperature: 6000
    property bool shouldBeOn
    property bool firstEvaluation: true
    property bool temperatureActive: false
    property var manualActive

    onAutomaticChanged: {
        root.manualActive = undefined;
        root.firstEvaluation = true;
        reEvaluate();
    }
    function reEvaluate() {
        root.shouldBeOn = night;
        if (firstEvaluation) {
            firstEvaluation = false;
            root.ensureState();
        }
    }
    onShouldBeOnChanged: root.ensureState()
    onConfigReadyChanged: if (!root.automatic) root.ensureState()
    function ensureState() {
        if (root.manualActive !== undefined)
            return;
        if (!root.automatic) {
            if (root.configReady)
                root.disableTemperature();
            return;
        }
        if (root.shouldBeOn) {
            root.enableTemperature();
        } else {
            root.disableTemperature();
        }
    }
    function enableTemperature() {
        root.temperatureActive = true;
        say("temperature", root.colorTemperature);
    }
    function disableTemperature() {
        root.temperatureActive = false;
        say("temperature", root.defaultColorTemperature);
    }

    property int frame: 0
    function sample() {
        frame++;
        if (frame === 2) { say("night, ready"); config.ready = true; say("active", temperatureActive) }
        if (frame === 4) { say("unready"); config.ready = false; say("active", temperatureActive) }
        if (frame === 5) { night = false; say("day, ready"); config.ready = true; say("active", temperatureActive) }
        if (frame === 7) { say("unready"); config.ready = false; say("active", temperatureActive) }
        if (frame === 8) { config.nightAutomatic = false; say("not automatic, ready"); config.ready = true; say("active", temperatureActive) }
    }
    Component.onCompleted: reEvaluate()
}
