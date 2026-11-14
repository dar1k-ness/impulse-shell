pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.modules.waffle.looks

StyledImage {
    id: avatar
    Layout.alignment: Qt.AlignTop
    sourceSize: Qt.size(32, 32)
    source: Directories.userAvatarPathAccountsService
    fallbacks: [Directories.userAvatarPathRicersAndWeirdSystems, Directories.userAvatarPathRicersAndWeirdSystems2]

    layer.enabled: true
    layer.effect: OpacityMask {
        maskSource: Circle {
            diameter: avatar.height
        }
    }
}
