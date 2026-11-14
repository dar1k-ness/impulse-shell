import QtQuick

/**
 * A repeating Timer on Config.options.resources.updateInterval for what is refreshed at that
 * period. It triggers at once when it starts, then together with every other running
 * UpdateTimer (UpdateClock).
 */
QtObject {
    id: root

    property bool running: false
    signal triggered()

    property Demand demand: Demand {
        source: UpdateClock
        active: root.running
        onCountedChanged: if (counted) root.triggered()
    }
    property Connections clock: Connections {
        target: UpdateClock
        enabled: root.demand.counted
        function onTick() {
            root.triggered();
        }
    }
}
