import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

/**
 * A free-form block inside a SettingsGroup (previews, tile pickers), styled like SettingsRow.
 */
Rectangle {
    id: root
    property bool shown: true
    property real padding: 16
    default property alias cardData: inner.data

    readonly property real bigRadius: Appearance.rounding.normal
    readonly property real smallRadius: 4
    readonly property bool groupFirst: {
        const p = root.parent;
        if (!p)
            return true;
        const c = p.children;
        for (let i = 0; i < c.length; i++) {
            const s = c[i].effShown !== undefined ? c[i].effShown : c[i].shown;
            if (s === undefined)
                continue;
            if (s)
                return c[i] === root;
        }
        return true;
    }
    readonly property bool groupLast: {
        const p = root.parent;
        if (!p)
            return true;
        const c = p.children;
        for (let i = c.length - 1; i >= 0; i--) {
            const s = c[i].effShown !== undefined ? c[i].effShown : c[i].shown;
            if (s === undefined)
                continue;
            if (s)
                return c[i] === root;
        }
        return true;
    }

    visible: shown
    Layout.fillWidth: true
    implicitHeight: inner.implicitHeight + root.padding * 2
    topLeftRadius: groupFirst ? bigRadius : smallRadius
    topRightRadius: groupFirst ? bigRadius : smallRadius
    bottomLeftRadius: groupLast ? bigRadius : smallRadius
    bottomRightRadius: groupLast ? bigRadius : smallRadius
    color: Appearance.colors.colLayer2

    ColumnLayout {
        id: inner
        anchors {
            left: parent.left
            right: parent.right
            top: parent.top
            margins: root.padding
        }
        spacing: 10
    }
}
