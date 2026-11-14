import QtQuick
import Rustshell

import ri.modules.common

LazyLoader {
    property bool extraCondition: true
    active: Config.ready && extraCondition
}
