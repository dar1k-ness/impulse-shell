import QtQuick
import qs.modules.common as C

// What modules/common/models/WorkspaceModel.qml gives Workspaces.qml, with no Hyprland to
// ask: the first group, workspaces 1, 2 and 4 with windows, the first one active.
QtObject {
    property var monitor
    readonly property int activeWorkspace: 1
    readonly property int fakeWorkspace: -9999
    readonly property int shownCount: C.Config.options.bar.workspaces.shown
    readonly property int group: 0
    readonly property string specialWorkspaceName: ""
    readonly property bool specialWorkspaceActive: false
    property list<bool> occupied: Array.from({ length: shownCount }, (_, i) => i < 2 || i === 3)
    property list<var> biggestWindow: occupied.map(() => undefined)

    function getWorkspaceId(group, index) {
        return group * shownCount + index + 1;
    }
    function getWorkspaceIdAt(index) {
        return getWorkspaceId(group, index);
    }
}
