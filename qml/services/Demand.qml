import QtQuick

/**
 * Counts itself into `source.users` while `active`, for a service that works only while
 * something uses it:
 *
 *     Demand { source: ResourceUsage; active: root.visible }
 */
QtObject {
    id: root

    required property QtObject source
    property bool active: true
    // Whether `source.users` includes this one now.
    property bool counted: false
    property bool alive: false

    function count(on) {
        if (on === counted)
            return;
        counted = on;
        source.users += on ? 1 : -1;
    }

    onActiveChanged: if (alive) count(active)
    Component.onCompleted: {
        alive = true;
        count(active);
    }
    Component.onDestruction: {
        alive = false;
        count(false);
    }
}
